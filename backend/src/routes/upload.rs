use axum::{
    body::Bytes,
    extract::{Multipart, Query, State},
    http::HeaderMap,
    response::Json,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs::{create_dir_all, File};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::AssetRepo;
use crate::domain::upload::{BatchUploadReceipt, UploadItemResult}; 
use crate::error::AppError;
use crate::services::queue::ProcessJob;
use crate::services::storage::StorageService;
use crate::AppState;

// ==========================================
// 1. FRONTEND HANDLER (Multipart Form-Data)
// ==========================================

struct StagedFile {
    file_name: String,
    bytes: Bytes,
}

pub async fn upload_photo(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<BatchUploadReceipt>, AppError> {
    let mut raw_folder = String::from("root");
    let mut is_private = false;
    let mut staged_files: Vec<StagedFile> = Vec::new();
    let mut results = Vec::new();

    while let Some(field) = multipart.next_field().await? {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "folder" => {
                let txt = field.text().await.unwrap_or_default();
                if !txt.trim().is_empty() {
                    raw_folder = txt;
                }
            }
            "is_private" => {
                let val = field.text().await.unwrap_or_default();
                is_private = val.eq_ignore_ascii_case("true") || val == "1";
            }
            "file" | "files" => {
                let file_name = field
                    .file_name()
                    .unwrap_or("media.raw")
                    .to_string();

                match field.bytes().await {
                    Ok(b) => staged_files.push(StagedFile { file_name, bytes: b }),
                    Err(err) => {
                        results.push(UploadItemResult {
                            file_name,
                            status: "error".to_string(),
                            id: None,
                            relative_path: None,
                            message: Some(format!("Failed reading bytes: {}", err)),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    let sanitized_folder = StorageService::sanitize_folder_path(&raw_folder);
    let target_dir = StorageService::resolve_upload_dir(
        &state.config.storage_root,
        is_private,
        &sanitized_folder,
    );

    if let Err(e) = create_dir_all(&target_dir).await {
        return Err(AppError::Internal(format!("Failed to create destination path: {}", e)));
    }

    for staged in staged_files {
        let bytes_len = staged.bytes.len() as i64;

        let mut hasher = Sha256::new();
        hasher.update(&staged.bytes);
        let sha256_hash = hex::encode(hasher.finalize());

        match AssetRepo::find_id_by_sha256(&state.db, &sha256_hash).await {
            Ok(Some(existing_id)) => {
                warn!(file = %staged.file_name, id = %existing_id, "Skipping duplicate");
                results.push(UploadItemResult {
                    file_name: staged.file_name,
                    status: "duplicate".to_string(),
                    id: Some(existing_id),
                    relative_path: None,
                    message: Some("File with identical checksum already exists".to_string()),
                });
                continue;
            }
            Err(e) => {
                results.push(UploadItemResult {
                    file_name: staged.file_name,
                    status: "error".to_string(),
                    id: None,
                    relative_path: None,
                    message: Some(format!("Duplicate check failure: {}", e)),
                });
                continue;
            }
            Ok(None) => {}
        }

        let asset_id = Uuid::new_v4().to_string();
        let destination_path = target_dir.join(&staged.file_name);

        let mut file = match File::create(&destination_path).await {
            Ok(f) => f,
            Err(e) => {
                results.push(UploadItemResult {
                    file_name: staged.file_name,
                    status: "error".to_string(),
                    id: None,
                    relative_path: None,
                    message: Some(format!("Disk write failure: {}", e)),
                });
                continue;
            }
        };

        if let Err(e) = file.write_all(&staged.bytes).await {
            results.push(UploadItemResult {
                file_name: staged.file_name,
                status: "error".to_string(),
                id: None,
                relative_path: None,
                message: Some(format!("Failed writing buffer: {}", e)),
            });
            continue;
        }

        let relative_path = format!("{}/{}", sanitized_folder, staged.file_name);

        let job_enqueued = state
            .job_sender
            .send(ProcessJob {
                asset_id: asset_id.clone(),
                file_name: staged.file_name.clone(),
                rel_path: relative_path.clone(),
                folder_path: sanitized_folder.clone(),
                disk_path: destination_path,
                sha256: sha256_hash,
                file_size_bytes: bytes_len,
                is_private,
            })
            .await;

        if let Err(e) = job_enqueued {
            results.push(UploadItemResult {
                file_name: staged.file_name,
                status: "error".to_string(),
                id: None,
                relative_path: None,
                message: Some(format!("Background pipeline refused job: {}", e)),
            });
            continue;
        }

        info!(id = %asset_id, file = %staged.file_name, "Media enqueued successfully");
        results.push(UploadItemResult {
            file_name: staged.file_name,
            status: "queued".to_string(),
            id: Some(asset_id),
            relative_path: Some(relative_path),
            message: None,
        });
    }

    let success_count = results.iter().filter(|r| r.status == "queued").count();

    Ok(Json(BatchUploadReceipt {
        total_uploaded: success_count,
        folder: sanitized_folder,
        is_private,
        items: results,
    }))
}

// ==========================================
// 2. SHORTCUTS HANDLER (Raw Binary Stream)
// ==========================================

fn detect_extension_from_magic_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 {
        return None;
    }

    // JPEG: FF D8 FF
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }

    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png");
    }

    // GIF: GIF87a or GIF89a
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }

    // WebP: RIFF....WEBP
    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }

    // ISO Base Media File Format (MP4, MOV, HEIC, HEIF)
    // Box structure: 4 bytes length, then 'ftyp'
    if &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        return match brand {
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => Some("heic"),
            b"qt  " => Some("mov"),
            _ => Some("mp4"),
        };
    }

    None
}

#[derive(Deserialize)]
pub struct RawUploadQuery {
    pub folder: Option<String>,
    pub is_private: Option<bool>,
    pub file_name: Option<String>,
    pub ext: Option<String>,
}

pub async fn upload_raw_binary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RawUploadQuery>,
    body: Bytes,
) -> Result<Json<UploadItemResult>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("Upload body cannot be empty".into()));
    }

    // 1. Resolve raw base filename
    let mut file_name = query
        .file_name
        .or_else(|| {
            headers
                .get("X-File-Name")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // 2. Check if file name already contains an extension
    let has_extension = Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| !e.is_empty())
        .unwrap_or(false);

    if !has_extension {
        // Fallbacks: Query ?ext= -> Header X-File-Ext -> Magic Bytes -> Default to "jpg"
        let ext = query
            .ext
            .or_else(|| {
                headers
                    .get("X-File-Ext")
                    .and_then(|h| h.to_str().ok())
                    .map(|s| s.trim_start_matches('.').to_string())
            })
            .or_else(|| detect_extension_from_magic_bytes(&body).map(|s| s.to_string()))
            .unwrap_or_else(|| "jpg".to_string());

        file_name = format!("{}.{}", file_name, ext);
    }

    let raw_folder = query.folder.unwrap_or_else(|| "root".to_string());
    let is_private = query.is_private.unwrap_or(false);
    let bytes_len = body.len() as i64;

    // 3. SHA-256 Checksum
    let mut hasher = Sha256::new();
    hasher.update(&body);
    let sha256_hash = hex::encode(hasher.finalize());

    // 4. Duplicate Check
    if let Ok(Some(existing_id)) = AssetRepo::find_id_by_sha256(&state.db, &sha256_hash).await {
        warn!(file = %file_name, id = %existing_id, "Skipping duplicate");
        return Ok(Json(UploadItemResult {
            file_name,
            status: "duplicate".to_string(),
            id: Some(existing_id),
            relative_path: None,
            message: Some("Identical file already exists".to_string()),
        }));
    }

    // 5. Target Directory & Write File
    let sanitized_folder = StorageService::sanitize_folder_path(&raw_folder);
    let target_dir = StorageService::resolve_upload_dir(
        &state.config.storage_root,
        is_private,
        &sanitized_folder,
    );

    create_dir_all(&target_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create directory: {}", e)))?;

    let asset_id = Uuid::new_v4().to_string();
    let destination_path = target_dir.join(&file_name);

    let mut file = File::create(&destination_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create file: {}", e)))?;

    file.write_all(&body)
        .await
        .map_err(|e| AppError::Internal(format!("Failed writing buffer: {}", e)))?;

    let relative_path = format!("{}/{}", sanitized_folder, file_name);

    // 6. Enqueue ProcessJob with extension-bearing name and path
    state
        .job_sender
        .send(ProcessJob {
            asset_id: asset_id.clone(),
            file_name: file_name.clone(),
            rel_path: relative_path.clone(),
            folder_path: sanitized_folder.clone(),
            disk_path: destination_path,
            sha256: sha256_hash,
            file_size_bytes: bytes_len,
            is_private,
        })
        .await
        .map_err(|e| AppError::Internal(format!("Queue error: {}", e)))?;

    info!(id = %asset_id, file = %file_name, "Raw stream uploaded & enqueued");

    Ok(Json(UploadItemResult {
        file_name,
        status: "queued".to_string(),
        id: Some(asset_id),
        relative_path: Some(relative_path),
        message: None,
    }))
}