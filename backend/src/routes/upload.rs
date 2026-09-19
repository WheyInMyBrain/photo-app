use axum::{
    body::Bytes,
    extract::{Multipart, Query, State},
    response::Json,
};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::io::SeekFrom;
use tokio::fs::{self, create_dir_all, File, OpenOptions, metadata};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

use crate::error::AppError;
use crate::services::queue::{QueueService};
use crate::middleware::auth::AuthUser;
use crate::AppState;

use media_processing::StorageService;

use db::domain::{
    BatchUploadReceipt, CandidateItem, ChunkUploadQuery, ChunkUploadResponse,
    CommitLinkRequest, FinalizeChunkQuery, InspectLinkRequest,
    InspectLinkResponse, InspectResult, RawUploadQuery, StagedFile, UploadItemResult,
};
use db::domain::job_repo::{DbJob, JobPayload};
use db::AssetRepo;

use db::scrapes_repo::{ScrapedMediaItemRecord, ScrapedVariantRecord, ScrapesRepo};
use media_downloader::{download_media, extract_media, ExtractedMediaMetadata, MediaType};

/// Pipeline for raw in-memory byte uploads (direct small files, single web uploads)
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
    if let Ok(Some(existing_id)) =
        AssetRepo::find_user_asset_by_sha256(&state.db, user_id, &sha256).await
    {
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

    let rel_path = if sanitized_folder.is_empty() || sanitized_folder == "root" {
        disk_filename
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
        job_type: "thumbnail".to_string(),
        file_size_bytes: bytes.len() as i64,
        payload: None, // No scraped context for generic file uploads
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

/// Core pipeline for files on disk: hashes, deduplicates, moves, and enqueues
async fn persist_and_enqueue_staged_file(
    state: &AppState,
    user_id: &str,
    part_path: &Path,
    file_name: &str,
    folder: &str,
    payload: Option<JobPayload>,
) -> Result<UploadItemResult, AppError> {
    if !part_path.exists() {
        return Err(AppError::NotFound("Temporary file not found".into()));
    }

    // 1. Stream-hash using 64KB buffers
    let mut file_to_hash = File::open(part_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed opening file: {e}")))?;

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

    // 2. Duplicate Check (Scoped to authenticated user)
    if let Ok(Some(existing_id)) =
        AssetRepo::find_user_asset_by_sha256(&state.db, user_id, &sha256_hash).await
    {
        let _ = fs::remove_file(part_path).await;
        return Ok(UploadItemResult {
            file_name: file_name.to_string(),
            status: "duplicate".to_string(),
            id: Some(existing_id),
            relative_path: None,
            message: Some("Duplicate file exists in your library".to_string()),
        });
    }

    // 3. Resolve destination & move
    let sanitized_folder = StorageService::sanitize_folder_path(folder);
    let target_dir = StorageService::resolve_upload_dir(
        &state.config.storage_root,
        user_id,
        &sanitized_folder,
    );

    fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed creating directory: {e}")))?;

    let asset_id = Uuid::new_v4().to_string();
    let disk_file_name = StorageService::generate_disk_filename(&asset_id, file_name);
    let destination_path = target_dir.join(&disk_file_name);

    fs::rename(part_path, &destination_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed moving asset: {e}")))?;

    let relative_path = if sanitized_folder.is_empty() || sanitized_folder == "root" {
        disk_file_name
    } else {
        format!("{}/{}", sanitized_folder, disk_file_name)
    };

    // 4. Enqueue into DB-backed QueueService
    let job = DbJob {
        id: Uuid::new_v4().to_string(),
        user_id: user_id.to_string(),
        asset_id: asset_id.clone(),
        file_name: file_name.to_string(),
        rel_path: relative_path.clone(),
        folder_path: sanitized_folder,
        disk_path: destination_path,
        sha256: sha256_hash,
        job_type: "thumbnail".to_string(),
        file_size_bytes: bytes_len,
        payload,
    };

    QueueService::enqueue(&state.db, &state.queue_notify, job)
        .await
        .map_err(|e| AppError::Internal(format!("Failed enqueuing job: {e}")))?;

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
    // 1. Initialize from URL query parameter fallback to "root"
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
    let clean_url = payload.url.trim();

    // 1. Check if post is already cached
    let manifest = if let Some(post) =
        ScrapesRepo::find_post_by_url(&state.db, &auth_user.id, clean_url)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
    {
        let items = ScrapesRepo::fetch_items_for_post(&state.db, &post.id)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        CandidateManifest {
            platform: post.platform.clone(),
            author: post.author.clone(),
            caption: post.caption.unwrap_or_default(),
            suggested_folder: format!("{}/{}", post.platform, post.author),
            items: items
                .into_iter()
                .map(|i| CandidateItem {
                    id: i.id,
                    media_type: i.media_type,
                    thumbnail_url: i.thumbnail_url.unwrap_or_default(),
                    thumbnail_base64: None,
                    high_res_url: i.cdn_url,
                    audio_url: i.audio_url,
                    suggested_filename: i.suggested_filename,
                })
                .collect(),
        }
    } else {
        // 2. Extract media from link
        let extracted: ExtractedMediaMetadata = extract_media(clean_url)
            .await
            .map_err(|e| AppError::BadRequest(format!("Link extraction failed: {e}")))?;

        let post_uuid = Uuid::new_v4().to_string();
        let total_items = extracted.items.len();

        let db_items: Vec<ScrapedMediaItemRecord> = extracted
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let media_type_str = match item.media_type {
                    MediaType::Video => "video",
                    MediaType::Image => "image",
                };

                let ext = if media_type_str == "video" {
                    "mp4"
                } else if item.high_res_url.contains(".png") {
                    "png"
                } else if item.high_res_url.contains(".webp") {
                    "webp"
                } else {
                    "jpg"
                };

                let prefix = extracted
                    .caption
                    .chars()
                    .take(24)
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect::<String>()
                    .trim_matches('_')
                    .to_lowercase();

                let safe_prefix = if prefix.is_empty() { "media" } else { &prefix };
                let suggested_filename = if total_items > 1 {
                    format!("{}_{}.{}", safe_prefix, idx + 1, ext)
                } else {
                    format!("{}.{}", safe_prefix, ext)
                };

                let (w, h) = item
                    .dimensions
                    .as_ref()
                    .map(|d| (Some(d.width as i64), Some(d.height as i64)))
                    .unwrap_or((None, None));

                let variants = item
                    .variants
                    .iter()
                    .map(|v| {
                        let (vw, vh) = v
                            .dimensions
                            .as_ref()
                            .map(|d| (Some(d.width as i64), Some(d.height as i64)))
                            .unwrap_or((None, None));

                        ScrapedVariantRecord {
                            url: v.url.clone(),
                            width: vw,
                            height: vh,
                            label: v.label.clone(),
                            file_size_bytes: v.file_size_bytes.map(|s| s as i64),
                            is_master: v.url == item.high_res_url,
                        }
                    })
                    .collect();

                ScrapedMediaItemRecord {
                    id: Uuid::new_v4().to_string(),
                    media_type: media_type_str.to_string(),
                    cdn_url: item.high_res_url.clone(),
                    audio_url: item.audio_url.clone(),
                    thumbnail_url: item.thumbnail_url.clone(),
                    suggested_filename,
                    width: w,
                    height: h,
                    variants,
                }
            })
            .collect();

        // 3. Persist post and items
        ScrapesRepo::save_scraped_post_and_items(
            &state.db,
            &post_uuid,
            &auth_user.id,
            &extracted.platform,
            clean_url,
            clean_url,
            &extracted.author,
            Some(&extracted.caption),
            &extracted.tags,
            &extracted.discovered_post_urls,
            extracted.next_page_url.as_deref(),
            &db_items,
        )
        .await
        .map_err(|e| AppError::Internal(format!("Failed saving scrape: {e}")))?;

        let suggested_folder = format!("{}/{}", extracted.platform, extracted.author);

        CandidateManifest {
            platform: extracted.platform,
            author: extracted.author,
            caption: extracted.caption,
            suggested_folder,
            items: db_items
                .into_iter()
                .map(|i| CandidateItem {
                    id: i.id,
                    media_type: i.media_type,
                    thumbnail_url: i.thumbnail_url.unwrap_or_default(),
                    thumbnail_base64: None,
                    high_res_url: i.cdn_url,
                    audio_url: i.audio_url,
                    suggested_filename: i.suggested_filename,
                })
                .collect(),
        }
    };

    let target_folder = StorageService::sanitize_folder_path(&manifest.suggested_folder);

    if manifest.items.len() == 1 {
        let commit_req = CommitLinkRequest {
            platform: manifest.platform,
            folder: Some(target_folder),
            selected_items: manifest.items,
        };
        let Json(receipt) = commit_link_download(State(state), auth_user, Json(commit_req)).await?;
        return Ok(Json(InspectResult::Committed(receipt)));
    }

    Ok(Json(InspectResult::Preview(InspectLinkResponse {
        suggested_folder: target_folder,
        platform: manifest.platform,
        author: manifest.author,
        caption: manifest.caption,
        total_items: manifest.items.len(),
        items: manifest.items,
    })))
}

pub async fn commit_link_download(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<CommitLinkRequest>,
) -> Result<Json<BatchUploadReceipt>, AppError> {
    if payload.selected_items.is_empty() {
        return Err(AppError::BadRequest("No items selected for download".to_string()));
    }

    let raw_folder = payload.folder.unwrap_or(payload.platform.clone());
    let sanitized_folder = StorageService::sanitize_folder_path(&raw_folder);
    let mut results = Vec::new();

    // Use a temp directory for initial download stream
    let temp_download_dir = state.config.storage_root.join("temp").join("downloads");
    tokio::fs::create_dir_all(&temp_download_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed creating temp dir: {e}")))?;

    for item in payload.selected_items {
        let temp_file_path = temp_download_dir.join(format!("{}_{}", Uuid::new_v4(), item.suggested_filename));

        // 1. Download asset directly to disk
        if let Err(e) = download_media(
            &item.high_res_url,
            item.audio_url.as_deref(),
            &temp_file_path,
            None,
        )
        .await
        {
            results.push(UploadItemResult {
                file_name: item.suggested_filename,
                status: "error".to_string(),
                id: None,
                relative_path: None,
                message: Some(format!("Download failed: {e}")),
            });
            continue;
        }

        // 2. Query context from ScrapesRepo
        let ctx = ScrapesRepo::get_item_context(&state.db, &item.id)
            .await
            .unwrap_or(None);

        let (platform, author, caption, tags, source_url) = if let Some(c) = ctx {
            (Some(c.platform), Some(c.author), c.caption, c.tags, Some(c.source_url))
        } else {
            (Some(payload.platform.clone()), None, None, Vec::new(), None)
        };

        let job_payload = JobPayload {
            author,
            platform,
            source_url,
            source_post_id: None,
            caption,
            tags,
            scraped_item_id: Some(item.id.clone()),
        };

        // 3. Delegate hashing, deduplication, moving, and queueing to persist_and_enqueue_staged_file
        match persist_and_enqueue_staged_file(
            &state,
            &auth_user.id,
            &temp_file_path,
            &item.suggested_filename,
            &sanitized_folder,
            Some(job_payload),
        )
        .await
        {
            Ok(res) => results.push(res),
            Err(e) => results.push(UploadItemResult {
                file_name: item.suggested_filename,
                status: "error".to_string(),
                id: None,
                relative_path: None,
                message: Some(format!("{e:?}")),
            }),
        }
    }

    let success_count = results.iter().filter(|r| r.status == "queued").count();

    Ok(Json(BatchUploadReceipt {
        total_uploaded: success_count,
        folder: sanitized_folder,
        items: results,
    }))
}

struct CandidateManifest {
    pub platform: String,
    pub author: String,
    pub caption: String,
    pub suggested_folder: String,
    pub items: Vec<CandidateItem>,
}

/// POST /api/upload/chunk
pub async fn upload_chunk(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(query): Query<ChunkUploadQuery>,
    body: Bytes,
) -> Result<Json<ChunkUploadResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("Chunk payload is empty".into()));
    }

    if query.total_chunks == 0 {
        return Err(AppError::BadRequest("total_chunks must be greater than 0".into()));
    }

    if query.chunk_index >= query.total_chunks {
        return Err(AppError::BadRequest(format!(
            "chunk_index {} out of bounds for total_chunks {}",
            query.chunk_index, query.total_chunks
        )));
    }

    // Tenant-isolated staging directory
    let temp_dir = state
        .config
        .storage_root
        .join("temp_chunks")
        .join(&auth_user.id);

    create_dir_all(&temp_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create temp chunk dir: {e}")))?;

    let part_path = temp_dir.join(format!("{}.part", query.upload_id));

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&part_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to open chunk file: {e}")))?;

    let offset = (query.chunk_index as u64) * query.chunk_size;
    file.seek(SeekFrom::Start(offset))
        .await
        .map_err(|e| AppError::Internal(format!("Failed to seek to byte offset {offset}: {e}")))?;

    file.write_all(&body)
        .await
        .map_err(|e| AppError::Internal(format!("Failed writing chunk bytes at offset {offset}: {e}")))?;

    // Flush memory buffers without forcing a full physical drive barrier sync on every chunk
    file.flush()
        .await
        .map_err(|e| AppError::Internal(format!("Failed flushing chunk buffer: {e}")))?;

    Ok(Json(ChunkUploadResponse {
        upload_id: query.upload_id,
        chunk_index: query.chunk_index,
        received: true,
    }))
}

/// POST /api/upload/chunk/finalize
pub async fn finalize_chunk(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(query): Query<FinalizeChunkQuery>,
) -> Result<Json<UploadItemResult>, AppError> {
    let temp_dir = state
        .config
        .storage_root
        .join("temp_chunks")
        .join(&auth_user.id);

    let part_path = temp_dir.join(format!("{}.part", query.upload_id));

    // Validate that file actually exists and contains data before moving to queue
    let meta = metadata(&part_path).await.map_err(|_| {
        AppError::NotFound("Upload session not found or chunks missing".to_string())
    })?;

    if meta.len() == 0 {
        return Err(AppError::BadRequest("Finalized file cannot be empty".to_string()));
    }

    let raw_folder = query.folder.unwrap_or_else(|| "root".to_string());

    let result = persist_and_enqueue_staged_file(
        &state,
        &auth_user.id,
        &part_path,
        &query.file_name,
        &raw_folder,
        None,
    )
    .await?;

    Ok(Json(result))
}