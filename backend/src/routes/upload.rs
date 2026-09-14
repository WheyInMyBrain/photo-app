use axum::{
    body::Bytes,
    extract::{Multipart, Query, State},
    response::Json,
};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::io::SeekFrom;
use tokio::fs::{self, create_dir_all, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

use crate::db::AssetRepo;
use crate::domain::upload::{StagedFile, BatchUploadReceipt, UploadItemResult, InspectLinkRequest, InspectResult, InspectLinkResponse, CandidateItem, CommitLinkRequest, RawUploadQuery, ChunkUploadQuery, ChunkUploadResponse, FinalizeChunkQuery}; 
use crate::error::AppError;
use crate::services::queue::{ProcessJob, QueueService};
use crate::middleware::auth::AuthUser;
use crate::AppState;

use media_processing::StorageService;
use crate::domain::job_repo::DbJob;

async fn persist_and_enqueue_bytes(
    state: &AppState,
    user_id: &str,
    file_name: &str,
    bytes: &[u8],
    folder: &str,
) -> UploadItemResult {
    let sanitized_folder = StorageService::sanitize_folder_path(folder);
    let sha256 = hex::encode(Sha256::digest(bytes));

    // Deduplication check scoped to this user
    if let Ok(Some(existing_id)) = AssetRepo::find_user_asset_by_sha256(&state.db, user_id, &sha256).await {
        return UploadItemResult {
            file_name: file_name.to_string(),
            status: "duplicate".to_string(),
            id: Some(existing_id),
            relative_path: None,
            message: Some("File already exists in library".into()),
        };
    }

    let asset_id = Uuid::new_v4().to_string();
    let target_dir = StorageService::resolve_upload_dir(
        &state.config.storage_root,
        user_id,
        &sanitized_folder,
    );

    if let Err(e) = tokio::fs::create_dir_all(&target_dir).await {
        return UploadItemResult {
            file_name: file_name.to_string(),
            status: "error".to_string(),
            id: None,
            relative_path: None,
            message: Some(format!("Failed creating directory: {e}")),
        };
    }

    let disk_filename = StorageService::generate_disk_filename(&asset_id, file_name);
    let disk_path = target_dir.join(&disk_filename);

    // rel_path must include the folder if not "root"
    let rel_path = if sanitized_folder == "root" {
        disk_filename.clone()
    } else {
        format!("{}/{}", sanitized_folder, disk_filename)
    };

    if let Err(e) = tokio::fs::write(&disk_path, bytes).await {
        return UploadItemResult {
            file_name: file_name.to_string(),
            status: "error".to_string(),
            id: None,
            relative_path: None,
            message: Some(format!("Failed writing file to disk: {e}")),
        };
    }

    let job = DbJob {
        id: Uuid::new_v4().to_string(),
        user_id: user_id.to_string(),
        asset_id: asset_id.clone(),
        file_name: file_name.to_string(),
        rel_path: rel_path.clone(),
        folder_path: sanitized_folder,
        disk_path,
        sha256,
        file_size_bytes: bytes.len() as i64,
    };

    if let Err(e) = QueueService::enqueue(&state.db, &state.queue_notify, job).await {
        return UploadItemResult {
            file_name: file_name.to_string(),
            status: "error".to_string(),
            id: None,
            relative_path: None,
            message: Some(format!("Failed enqueuing job: {e}")),
        };
    }

    UploadItemResult {
        file_name: file_name.to_string(),
        status: "queued".to_string(),
        id: Some(asset_id),
        relative_path: Some(rel_path),
        message: None,
    }
}

/// Core pipeline for stitched chunk files already present on disk
async fn persist_and_enqueue_staged_file(
    state: &AppState,
    user_id: &str,
    part_path: &Path,
    file_name: &str,
    folder: &str,
) -> Result<UploadItemResult, AppError> {
    if !part_path.exists() {
        return Err(AppError::NotFound("Temporary chunk file not found".into()));
    }

    // 1. Flush & Stream-hash using 64KB buffers
    let mut file_to_hash = File::open(part_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed opening part file: {e}")))?;

    file_to_hash.sync_all().await.ok();

    let metadata = file_to_hash
        .metadata()
        .await
        .map_err(|e| AppError::Internal(format!("Failed reading metadata: {e}")))?;
    let bytes_len = metadata.len() as i64;

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];

    loop {
        let bytes_read = file_to_hash
            .read(&mut buffer)
            .await
            .map_err(|e| AppError::Internal(format!("Failed reading bytes: {e}")))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    drop(file_to_hash);

    let sha256_hash = hex::encode(hasher.finalize());

    // 2. Duplicate Check (Scoped to the authenticated user)
    if let Ok(Some(existing_id)) = AssetRepo::find_user_asset_by_sha256(&state.db, user_id, &sha256_hash).await {
        let _ = fs::remove_file(part_path).await;
        return Ok(UploadItemResult {
            file_name: file_name.to_string(),
            status: "duplicate".to_string(),
            id: Some(existing_id),
            relative_path: None,
            message: Some("Duplicate file exists in your library".to_string()),
        });
    }

    // 3. Resolve destination & move -> storage_root/originals/<user_id>/<folder>
    let sanitized_folder = StorageService::sanitize_folder_path(folder);
    let target_dir = StorageService::resolve_upload_dir(
        &state.config.storage_root,
        user_id,
        &sanitized_folder,
    );

    fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create folder: {e}")))?;

    let asset_id = Uuid::new_v4().to_string();
    let disk_file_name = StorageService::generate_disk_filename(&asset_id, file_name);
    let destination_path = target_dir.join(&disk_file_name);

    fs::rename(part_path, &destination_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to move asset to storage: {e}")))?;

    let relative_path = if sanitized_folder.is_empty() || sanitized_folder == "root" {
        disk_file_name
    } else {
        format!("{}/{}", sanitized_folder, disk_file_name)
    };

    // 4. Enqueue into DB with user_id
    let job = ProcessJob {
        id: Uuid::new_v4().to_string(),
        user_id: user_id.to_string(),
        asset_id: asset_id.clone(),
        file_name: file_name.to_string(),
        rel_path: relative_path.clone(),
        folder_path: sanitized_folder,
        disk_path: destination_path,
        sha256: sha256_hash,
        file_size_bytes: bytes_len,
    };

    QueueService::enqueue(&state.db, &state.queue_notify, job)
        .await
        .map_err(|e| AppError::Internal(format!("Failed enqueuing job into database: {e}")))?;

    Ok(UploadItemResult {
        file_name: file_name.to_string(),
        status: "queued".to_string(),
        id: Some(asset_id),
        relative_path: Some(relative_path),
        message: None,
    })
}

// ==========================================
// 1. FRONTEND HANDLER (Multipart Form-Data)
// ==========================================

pub async fn upload_photo(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(query): Query<RawUploadQuery>,
    mut multipart: Multipart,
) -> Result<Json<BatchUploadReceipt>, AppError> {
    // 1. Initialize from URL query parameter (?folder=weapons), fallback to "root"
    let mut raw_folder = query
        .folder
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "root".to_string());

    let mut staged_files: Vec<StagedFile> = Vec::new();
    let mut results = Vec::new();

    while let Some(field) = multipart.next_field().await? {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            // Form body field can also set/override the folder
            "folder" => {
                let txt = field.text().await.unwrap_or_default();
                if !txt.trim().is_empty() {
                    raw_folder = txt;
                }
            }
            "file" | "files" => {
                let file_name = field.file_name().unwrap_or("media.raw").to_string();

                match field.bytes().await {
                    Ok(b) => staged_files.push(StagedFile { file_name, bytes: b }),
                    Err(err) => {
                        results.push(UploadItemResult {
                            file_name,
                            status: "error".to_string(),
                            id: None,
                            relative_path: None,
                            message: Some(format!("Failed reading bytes: {err}")),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    let sanitized_folder = StorageService::sanitize_folder_path(&raw_folder);

    for staged in staged_files {
        let res = persist_and_enqueue_bytes(
            &state,
            &auth_user.id,
            &staged.file_name,
            &staged.bytes,
            &sanitized_folder,
        )
        .await;
        results.push(res);
    }

    let success_count = results.iter().filter(|r| r.status == "queued").count();

    Ok(Json(BatchUploadReceipt {
        total_uploaded: success_count,
        folder: sanitized_folder,
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

pub async fn upload_raw_binary(
    State(state): State<AppState>,
    auth_user: AuthUser,
    headers: axum::http::HeaderMap,
    Query(query): Query<RawUploadQuery>,
    body: Bytes,
) -> Result<Json<UploadItemResult>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("Upload body cannot be empty".into()));
    }

    let mut file_name = query
        .file_name
        .or_else(|| {
            headers
                .get("X-File-Name")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let has_extension = Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| !e.is_empty())
        .unwrap_or(false);

    if !has_extension {
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

    let result = persist_and_enqueue_bytes(
        &state,
        &auth_user.id,
        &file_name,
        &body,
        &raw_folder,
    )
    .await;

    Ok(Json(result))
}

pub async fn inspect_link(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<InspectLinkRequest>,
) -> Result<Json<InspectResult>, AppError> {
    let meta = media_downloader::extract_links(&payload.url)
        .await
        .map_err(|e| AppError::BadRequest(format!("Link inspection failed: {e}")))?;

    let clean_caption: String = meta
        .caption
        .chars()
        .take(30)
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase();

    let prefix = if clean_caption.is_empty() { "post" } else { &clean_caption };
    let total = meta.items.len();

    let candidate_items: Vec<CandidateItem> = meta
        .items
        .into_iter()
        .enumerate()
        .map(|(idx, item)| {
            let media_type_str = match item.media_type {
                media_downloader::MediaType::Video => "video".to_string(),
                media_downloader::MediaType::Image => "image".to_string(),
            };

            let ext = if media_type_str == "video" { "mp4" } else { "jpg" };
            let suggested_filename = if total > 1 {
                format!("{prefix}_{}.{ext}", idx + 1)
            } else {
                format!("{prefix}.{ext}")
            };

            CandidateItem {
                id: format!("item_{idx}"),
                media_type: media_type_str,
                thumbnail_url: item.thumbnail_url,
                thumbnail_base64: item.thumbnail_base64,
                high_res_url: item.high_res_url,
                audio_url: item.audio_url,
                suggested_filename,
            }
        })
        .collect();

    // Single item downloads and enqueues immediately
    if total == 1 {
        let commit_req = CommitLinkRequest {
            platform: meta.platform.clone(),
            folder: Some(format!("{}/{}", meta.platform, meta.author)),
            selected_items: candidate_items,
        };

        let Json(receipt) = commit_link_download(State(state), auth_user, Json(commit_req)).await?;
        return Ok(Json(InspectResult::Committed(receipt)));
    }

    Ok(Json(InspectResult::Preview(InspectLinkResponse {
        suggested_folder: format!("{}/{}", meta.platform, meta.author),
        platform: meta.platform,
        author: meta.author,
        caption: meta.caption,
        total_items: total,
        items: candidate_items,
    })))
}

/// POST /api/media/commit-link
/// Fetches user-selected items from CDN and enqueues them for processing
pub async fn commit_link_download(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<CommitLinkRequest>,
) -> Result<Json<BatchUploadReceipt>, AppError> {
    if payload.selected_items.is_empty() {
        return Err(AppError::BadRequest("No items selected for download".to_string()));
    }

    let raw_folder = payload.folder.unwrap_or(payload.platform);
    let mut results = Vec::new();

    for item in payload.selected_items {
        let bytes = match media_downloader::download_asset(
            &item.high_res_url,
            item.audio_url.as_deref(),
            &item.media_type,
        )
        .await
        {
            Ok(b) => b,
            Err(e) => {
                results.push(UploadItemResult {
                    file_name: item.suggested_filename,
                    status: "error".to_string(),
                    id: None,
                    relative_path: None,
                    message: Some(format!("Asset download failed: {e}")),
                });
                continue;
            }
        };

        let res = persist_and_enqueue_bytes(
            &state,
            &auth_user.id,
            &item.suggested_filename,
            &bytes,
            &raw_folder,
        )
        .await;

        results.push(res);
    }

    let success_count = results.iter().filter(|r| r.status == "queued").count();

    Ok(Json(BatchUploadReceipt {
        total_uploaded: success_count,
        folder: StorageService::sanitize_folder_path(&raw_folder),
        items: results,
    }))
}

/// POST /api/upload/chunk
/// Writes a chunk directly to its byte offset in staging
pub async fn upload_chunk(
    State(state): State<AppState>,
    _auth_user: AuthUser, // Validates authenticated session or API key
    Query(query): Query<ChunkUploadQuery>,
    body: Bytes,
) -> Result<Json<ChunkUploadResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("Chunk payload is empty".into()));
    }

    let temp_dir = state.config.storage_root.join("temp_chunks");
    create_dir_all(&temp_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create temp chunk dir: {}", e)))?;

    let part_path = temp_dir.join(format!("{}.part", query.upload_id));

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&part_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to open chunk file: {}", e)))?;

    let offset = (query.chunk_index as u64) * query.chunk_size;
    file.seek(SeekFrom::Start(offset))
        .await
        .map_err(|e| AppError::Internal(format!("Failed to seek to byte offset {}: {}", offset, e)))?;

    file.write_all(&body)
        .await
        .map_err(|e| AppError::Internal(format!("Failed writing chunk bytes at offset {}: {}", offset, e)))?;

    file.sync_all().await?;

    tracing::debug!(
        upload_id = %query.upload_id,
        part = query.chunk_index + 1,
        total = query.total_chunks,
        offset = offset,
        "Received and aligned chunk"
    );

    Ok(Json(ChunkUploadResponse {
        upload_id: query.upload_id,
        chunk_index: query.chunk_index,
        received: true,
    }))
}

/// POST /api/upload/chunk/finalize
/// Stitches and commits chunked uploads into user storage
pub async fn finalize_chunk(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(query): Query<FinalizeChunkQuery>,
) -> Result<Json<UploadItemResult>, AppError> {
    let temp_dir = state.config.storage_root.join("temp_chunks");
    let part_path = temp_dir.join(format!("{}.part", query.upload_id));
    let raw_folder = query.folder.unwrap_or_else(|| "root".to_string());

    let result = persist_and_enqueue_staged_file(
        &state,
        &auth_user.id,
        &part_path,
        &query.file_name,
        &raw_folder,
    )
    .await?;

    Ok(Json(result))
}