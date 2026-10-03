use axum::{
    body::{Body, Bytes},
    extract::{Multipart, Query, State},
    http::HeaderMap,
    response::Json,
};
use chrono::Local;
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use tokio::fs::{self, create_dir_all, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;

use media_processing::StorageService;

use db::domain::{
    BatchUploadReceipt, CandidateItem, CandidateManifest, CheckUploadRequest, CheckUploadResponse,
    ChunkUploadQuery, ChunkUploadResponse, CommitLinkRequest, DbJob, FinalizeChunkQuery,
    FinishBatchRequest, FinishBatchResponse, IngestResponse, InspectLinkRequest,
    InspectLinkResponse, InspectResult, JobPayload, RawUploadQuery, StartBatchResponse,
    UploadItemResult,
};
use db::scrapes_repo::{ScrapedMediaItemRecord, ScrapedVariantRecord, ScrapesRepo};
use db::{AlbumRepo, AssetRepo, JobRepo, UploadRepo};
use media_downloader::{
    download_media, extract_media, inject_metadata, stream_thumbnail_base64,
    ExtractedMediaMetadata, MediaMetadataPayload, MediaType,
};

// ==========================================
// 1. FRONTEND HANDLER (Multipart Form-Data)
// ==========================================

/// POST /api/upload/batch/start
pub async fn start_batch(
    State(state): State<AppState>,
    _auth_user: AuthUser,
) -> Result<Json<StartBatchResponse>, AppError> {
    let batch_id = UploadRepo::new_batch_id();

    // Pause all background workers to free full I/O and CPU for network upload
    state.pause_processing();

    Ok(Json(StartBatchResponse { batch_id }))
}

/// POST /api/upload/batch/finish
pub async fn finish_batch(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<FinishBatchRequest>,
) -> Result<Json<FinishBatchResponse>, AppError> {
    // 1. Atomically promote all 'staged' jobs of this batch to 'pending' in SQLite
    let released_job_ids = JobRepo::commit_staged_batch(&state.db, &auth_user.id, &payload.batch_id)
        .await
        .map_err(|e| {
            // Guarantee we don't leave workers permanently paused on error
            state.resume_processing_if_idle();
            AppError::Internal(format!("Failed committing batch: {e}"))
        })?;

    let count = released_job_ids.len() as u64;

    // 2. Push each promoted job directly into the in-memory assemble channel
    for job_id in released_job_ids {
        if let Ok(Some(job)) = JobRepo::acquire_job_by_id(&state.db, &job_id).await {
            let _ = state.channels.assemble_tx.send(job).await;
        }
    }

    // 3. Decrement active uploads; if no other batches are transferring, workers resume immediately
    state.resume_processing_if_idle();

    Ok(Json(FinishBatchResponse {
        released_jobs: count,
        status: "processing_started".to_string(),
    }))
}

/// POST /api/upload/check
pub async fn check_upload(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<CheckUploadRequest>,
) -> Result<Json<CheckUploadResponse>, AppError> {
    let sha256 = payload.sha256.trim().to_lowercase();
    if sha256.is_empty() {
        return Err(AppError::BadRequest("sha256 hash cannot be empty".into()));
    }

    // 1. O(1) B-tree lookup on UNIQUE(user_id, sha256)
    let check_res = AssetRepo::check_duplicate_by_sha256(&state.db, &auth_user.id, &sha256)
        .await
        .map_err(|e| AppError::Internal(format!("Database lookup error: {e}")))?;

    // 2. If it exists and is active, link it into the destination folder/album if specified
    if check_res.exists && !check_res.is_deleted {
        if let Some(ref asset_id) = check_res.asset_id {
            let folder_candidate = payload.folder.unwrap_or_default();
            let sanitized_folder = StorageService::sanitize_folder_path(&folder_candidate);

            if !sanitized_folder.is_empty() && sanitized_folder != "root" {
                if let Ok(mut tx) = state.db.begin().await {
                    if AlbumRepo::link_asset_to_folder_albums_tx(
                        &mut tx,
                        &auth_user.id,
                        asset_id,
                        &sanitized_folder,
                    )
                    .await
                    .is_ok()
                    {
                        let _ = tx.commit().await;
                    }
                }
            }
        }
    }

    Ok(Json(check_res))
}

pub async fn upload_photo(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(query): Query<RawUploadQuery>,
    mut multipart: Multipart,
) -> Result<Json<BatchUploadReceipt>, AppError> {
    let mut raw_folder = query
        .folder
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_camera_folder);

    let mut results = Vec::new();
    let staging_root = resolve_user_temp_dir(&state, &auth_user.id, "staging").await?;

    while let Some(mut field) = multipart.next_field().await? {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "folder" => {
                let txt = field.text().await.unwrap_or_default();
                if !txt.trim().is_empty() {
                    raw_folder = txt;
                }
            }
            "file" | "files" => {
                let file_name = field.file_name().unwrap_or("media.raw").to_string();
                let asset_id = Uuid::new_v4().to_string();
                let staged_path = staging_root.join(format!("{}.staged", asset_id));

                let mut file = match OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&staged_path)
                    .await
                {
                    Ok(f) => f,
                    Err(err) => {
                        results.push(UploadItemResult {
                            file_name,
                            status: "error".to_string(),
                            id: None,
                            relative_path: None,
                            message: Some(format!("Failed allocating disk file: {err}")),
                        });
                        continue;
                    }
                };

                let mut total_bytes = 0i64;
                let mut stream_error = false;

                while let Ok(Some(chunk)) = field.chunk().await {
                    if let Err(err) = file.write_all(&chunk).await {
                        results.push(UploadItemResult {
                            file_name: file_name.clone(),
                            status: "error".to_string(),
                            id: None,
                            relative_path: None,
                            message: Some(format!("Failed writing stream to disk: {err}")),
                        });
                        stream_error = true;
                        break;
                    }
                    total_bytes += chunk.len() as i64;
                }

                if stream_error {
                    let _ = fs::remove_file(&staged_path).await;
                    continue;
                }

                if total_bytes == 0 {
                    let _ = fs::remove_file(&staged_path).await;
                    results.push(UploadItemResult {
                        file_name,
                        status: "error".to_string(),
                        id: None,
                        relative_path: None,
                        message: Some("Uploaded file was empty".to_string()),
                    });
                    continue;
                }

                let _ = file.flush().await;
                drop(file);

                let sanitized_folder = StorageService::sanitize_folder_path(&raw_folder);

                let job = DbJob {
                    id: Uuid::new_v4().to_string(),
                    user_id: auth_user.id.clone(),
                    asset_id: asset_id.clone(),
                    file_name: file_name.clone(),
                    rel_path: String::new(),
                    folder_path: sanitized_folder,
                    disk_path: staged_path.clone(),
                    sha256: String::new(),
                    job_type: "assemble".to_string(),
                    file_size_bytes: total_bytes,
                    payload: None,
                    ai_faces_done: 0,
                    ai_clip_done: 0,
                    ai_tags_done: 0,
                    ai_poses_done: 0,
                };

                let batch_id_opt = query.batch_id.as_deref();
                let initial_status = if batch_id_opt.is_some() { "staged" } else { "pending" };

                // 1. Write durable WAL record in SQLite
                if let Err(e) = JobRepo::enqueue_with_status(&state.db, &job, initial_status, batch_id_opt).await {
                    let _ = fs::remove_file(&staged_path).await;
                    results.push(UploadItemResult {
                        file_name,
                        status: "error".to_string(),
                        id: None,
                        relative_path: None,
                        message: Some(format!("Failed enqueuing job: {e}")),
                    });
                    continue;
                }

                // 2. If unbatched, push directly into Worker 0's channel
                if batch_id_opt.is_none() {
                    let _ = state.channels.assemble_tx.send(job).await;
                }

                results.push(UploadItemResult {
                    file_name,
                    status: "queued".to_string(),
                    id: Some(asset_id),
                    relative_path: None,
                    message: Some("File saved, queued for background processing".to_string()),
                });
            }
            _ => {}
        }
    }

    let success_count = results.iter().filter(|r| r.status == "queued").count();

    Ok(Json(BatchUploadReceipt {
        total_uploaded: success_count,
        folder: StorageService::sanitize_folder_path(&raw_folder),
        items: results,
    }))
}

/// POST /api/upload/inspect
pub async fn inspect_link(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<InspectLinkRequest>,
) -> Result<Json<InspectResult>, AppError> {
    let mut manifest = resolve_or_scrape_manifest(&state, &auth_user, &payload.url).await?;
    let target_folder = StorageService::sanitize_folder_path(&manifest.suggested_folder);

    // Fast-path: single item posts are committed immediately without computing previews
    if manifest.items.len() == 1 {
        let receipt = execute_item_downloads(
            &state,
            &auth_user,
            manifest.items,
            &target_folder,
            &manifest.platform,
        )
        .await?;
        return Ok(Json(InspectResult::Committed(receipt)));
    }

    // Carousel path: stream lightweight base64 thumbnails in memory for user selection
    for item in &mut manifest.items {
        if item.thumbnail_base64.is_none() {
            let thumb_target = if !item.thumbnail_url.is_empty() {
                &item.thumbnail_url
            } else {
                &item.high_res_url
            };
            item.thumbnail_base64 = stream_thumbnail_base64(
                thumb_target,
                item.referer.as_deref().or(Some(&payload.url)),
            )
            .await;
        }
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

/// POST /api/upload/ingest
pub async fn upload_ingest(
    State(state): State<AppState>,
    auth_user: AuthUser,
    headers: HeaderMap,
    Query(query): Query<RawUploadQuery>,
    body: Bytes,
) -> Result<Json<IngestResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("Upload body cannot be empty".into()));
    }

    let content_type = headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    // Case 1: JSON Payloads (Handles both CommitLinkRequest AND Apple Shortcut {"url": "..."})
    if content_type.starts_with("application/json") || body.starts_with(b"{") {
        // 1a: Try CommitLinkRequest (web app selective import)
        if let Ok(commit_req) = serde_json::from_slice::<CommitLinkRequest>(&body) {
            let target_folder = commit_req
                .folder
                .unwrap_or_else(|| commit_req.platform.clone());

            let receipt = execute_item_downloads(
                &state,
                &auth_user,
                commit_req.selected_items,
                &target_folder,
                &commit_req.platform,
            )
            .await?;

            return Ok(Json(IngestResponse::Batch(receipt)));
        }

        // 1b: Catch Apple Shortcut JSON: {"url": "https://..."} or {"link": "..."}
        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&body) {
            let extracted_url = val
                .get("url")
                .or_else(|| val.get("link"))
                .or_else(|| val.get("target"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim());

            if let Some(target_url) = extracted_url {
                if target_url.starts_with("http://") || target_url.starts_with("https://") {
                    let manifest =
                        resolve_or_scrape_manifest(&state, &auth_user, target_url).await?;
                    let target_folder = query.folder.unwrap_or(manifest.suggested_folder);

                    let receipt = execute_item_downloads(
                        &state,
                        &auth_user,
                        manifest.items,
                        &target_folder,
                        &manifest.platform,
                    )
                    .await?;

                    return Ok(Json(IngestResponse::Batch(receipt)));
                }
            }
        }
    }

    // Case 2: URL Auto-Commit (Raw text link)
    let is_text_or_url = content_type.starts_with("text/")
        || (body.len() < 2048
            && std::str::from_utf8(&body)
                .map(|s| s.trim().starts_with("http"))
                .unwrap_or(false));

    if is_text_or_url {
        if let Ok(raw_str) = std::str::from_utf8(&body) {
            let trimmed = raw_str.trim();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                let manifest = resolve_or_scrape_manifest(&state, &auth_user, trimmed).await?;
                let target_folder = query.folder.unwrap_or(manifest.suggested_folder);

                let receipt = execute_item_downloads(
                    &state,
                    &auth_user,
                    manifest.items,
                    &target_folder,
                    &manifest.platform,
                )
                .await?;

                return Ok(Json(IngestResponse::Batch(receipt)));
            }
        }
    }

    // Case 3: Binary Media Upload (Pushed directly to staging disk and queued)
    let file_name = resolve_incoming_filename(&query, &headers, &body);
    let final_folder = query.folder.unwrap_or_else(default_camera_folder);
    let sanitized_folder = StorageService::sanitize_folder_path(&final_folder);

    let staging_root = resolve_user_temp_dir(&state, &auth_user.id, "staging").await?;
    let asset_id = Uuid::new_v4().to_string();
    let staged_path = staging_root.join(format!("{}.staged", asset_id));

    fs::write(&staged_path, &body).await.map_err(|e| {
        AppError::Internal(format!("Failed writing staged ingest file: {e}"))
    })?;

    let job = DbJob {
        id: Uuid::new_v4().to_string(),
        user_id: auth_user.id.clone(),
        asset_id: asset_id.clone(),
        file_name: file_name.clone(),
        rel_path: String::new(),
        folder_path: sanitized_folder,
        disk_path: staged_path,
        sha256: String::new(),
        job_type: "assemble".to_string(),
        file_size_bytes: body.len() as i64,
        payload: None,
        ai_faces_done: 0,
        ai_clip_done: 0,
        ai_tags_done: 0,
        ai_poses_done: 0,
    };

    let batch_id_opt = query.batch_id.as_deref();
    let initial_status = if batch_id_opt.is_some() { "staged" } else { "pending" };

    JobRepo::enqueue_with_status(&state.db, &job, initial_status, batch_id_opt)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to enqueue ingest job: {e}")))?;

    if batch_id_opt.is_none() {
        let _ = state.channels.assemble_tx.send(job).await;
    }

    Ok(Json(IngestResponse::File(UploadItemResult {
        file_name,
        status: "queued".to_string(),
        id: Some(asset_id),
        relative_path: None,
        message: Some("Queued for background processing".to_string()),
    })))
}

/// POST /api/upload/chunk
pub async fn upload_chunk(
    State(_state): State<AppState>,
    auth_user: AuthUser,
    Query(query): Query<ChunkUploadQuery>,
    body: Body,
) -> Result<Json<ChunkUploadResponse>, AppError> {
    if query.total_chunks == 0 {
        return Err(AppError::BadRequest("total_chunks must be greater than 0".into()));
    }
    if query.chunk_index >= query.total_chunks {
        return Err(AppError::BadRequest(format!(
            "chunk_index {} out of bounds for total_chunks {}",
            query.chunk_index, query.total_chunks
        )));
    }

    let temp_root = resolve_user_temp_dir(&_state, &auth_user.id, "chunks").await?;
    let session_dir = temp_root.join(&query.upload_id);
    fs::create_dir_all(&session_dir).await.map_err(|e| {
        AppError::Internal(format!("Failed to create chunk session dir: {e}"))
    })?;

    // Record total_chunks once
    let meta_path = session_dir.join("total_chunks");
    if !meta_path.exists() {
        let _ = fs::write(&meta_path, query.total_chunks.to_string()).await;
    }

    let chunk_path = session_dir.join(format!("{:06}.part", query.chunk_index));
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&chunk_path)
        .await
        .map_err(|e| AppError::Internal(format!("Failed creating chunk file: {e}")))?;

    // Stream incoming bytes directly to disk
    let mut stream = body.into_data_stream();
    let mut total_written = 0usize;

    while let Some(chunk_result) = stream.next().await {
        let data = chunk_result.map_err(|e| {
            AppError::BadRequest(format!("Network stream read error: {e}"))
        })?;

        file.write_all(&data).await.map_err(|e| {
            AppError::Internal(format!("Disk write error: {e}"))
        })?;

        total_written += data.len();
    }

    if total_written == 0 {
        let _ = fs::remove_file(&chunk_path).await;
        return Err(AppError::BadRequest("Chunk payload is empty".into()));
    }

    file.flush()
        .await
        .map_err(|e| AppError::Internal(format!("Failed flushing chunk file: {e}")))?;

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
    let temp_root = resolve_user_temp_dir(&state, &auth_user.id, "chunks").await?;
    let session_dir = temp_root.join(&query.upload_id);

    if !session_dir.exists() {
        return Err(AppError::NotFound(
            "Upload session directory not found or expired".into(),
        ));
    }

    let meta_path = session_dir.join("total_chunks");
    let total_chunks_str = fs::read_to_string(&meta_path).await.map_err(|_| {
        AppError::BadRequest("Upload session missing metadata; no chunks received".into())
    })?;

    let total_chunks: u32 = total_chunks_str.trim().parse().map_err(|_| {
        AppError::Internal("Corrupted total_chunks metadata".into())
    })?;

    for idx in 0..total_chunks {
        let chunk_path = session_dir.join(format!("{:06}.part", idx));
        if !chunk_path.exists() {
            return Err(AppError::BadRequest(format!(
                "Incomplete upload: chunk {} of {} is missing",
                idx, total_chunks
            )));
        }
    }

    let asset_id = Uuid::new_v4().to_string();
    let final_folder = query.folder.unwrap_or_else(default_camera_folder);
    let sanitized_folder = StorageService::sanitize_folder_path(&final_folder);

    let job = DbJob {
        id: Uuid::new_v4().to_string(),
        user_id: auth_user.id.clone(),
        asset_id: asset_id.clone(),
        file_name: query.file_name.clone(),
        rel_path: String::new(),
        folder_path: sanitized_folder,
        disk_path: session_dir,
        sha256: String::new(),
        job_type: "assemble".to_string(),
        file_size_bytes: 0,
        payload: None,
        ai_faces_done: 0,
        ai_clip_done: 0,
        ai_tags_done: 0,
        ai_poses_done: 0,
    };

    let batch_id_opt = query.batch_id.as_deref();
    let initial_status = if batch_id_opt.is_some() { "staged" } else { "pending" };

    // 1. Write durable WAL entry
    JobRepo::enqueue_with_status(&state.db, &job, initial_status, batch_id_opt)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to enqueue assemble job: {e}")))?;

    // 2. If unbatched, push directly into Worker 0 channel
    if batch_id_opt.is_none() {
        let _ = state.channels.assemble_tx.send(job).await;
    }

    Ok(Json(UploadItemResult {
        file_name: query.file_name,
        status: "queued".to_string(),
        id: Some(asset_id),
        relative_path: None,
        message: Some("Upload verified; assembling in background".to_string()),
    }))
}

// ==========================================
// 2. SHORTCUTS & HELPERS (Raw Binary Stream)
// ==========================================

fn detect_extension_from_magic_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 {
        return None;
    }

    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }

    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png");
    }

    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }

    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }

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

async fn resolve_user_temp_dir(state: &AppState, user_id: &str, sub: &str) -> Result<PathBuf, AppError> {
    let dir = state.config.storage_root.join("temp").join(sub).join(user_id);
    create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed creating temp dir {dir:?}: {e}")))?;
    Ok(dir)
}

fn default_camera_folder() -> String {
    format!("Camera Roll/{}", Local::now().format("%Y-%m"))
}

fn resolve_incoming_filename(
    query: &RawUploadQuery,
    headers: &HeaderMap,
    body: &[u8],
) -> String {
    let mut file_name = query
        .file_name
        .clone()
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
            .clone()
            .or_else(|| {
                headers
                    .get("X-File-Ext")
                    .and_then(|h| h.to_str().ok())
                    .map(|s| s.trim_start_matches('.').to_string())
            })
            .or_else(|| detect_extension_from_magic_bytes(body).map(|s| s.to_string()))
            .unwrap_or_else(|| "jpg".to_string());

        file_name = format!("{}.{}", file_name, ext);
    }

    file_name
}

fn ext_from_mime(mime: &str, media_type: &MediaType) -> &'static str {
    let clean = mime.split(';').next().unwrap_or(mime).trim();
    match clean {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/heic" | "image/heif" => "heic",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/x-matroska" => "mkv",
        "application/x-mpegurl" | "application/vnd.apple.mpegurl" => "mp4",
        _ => match media_type {
            MediaType::Video => "mp4",
            MediaType::Image => "jpg",
        },
    }
}

fn build_suggested_filename(caption: &str, index: usize, total_items: usize, ext: &str) -> String {
    let prefix = caption
        .chars()
        .take(30)
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase();

    let safe_prefix = if prefix.is_empty() { "media" } else { &prefix };

    if total_items > 1 {
        format!("{}_{}.{}", safe_prefix, index + 1, ext)
    } else {
        format!("{}.{}", safe_prefix, ext)
    }
}

// ============================================================================
// Shared Core Scraper Pipeline
// ============================================================================

async fn resolve_or_scrape_manifest(
    state: &AppState,
    auth_user: &AuthUser,
    url: &str,
) -> Result<CandidateManifest, AppError> {
    let clean_url = url.trim();

    if let Some(post) = ScrapesRepo::find_post_by_url(&state.db, &auth_user.id, clean_url)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    {
        let items = ScrapesRepo::fetch_items_for_post(&state.db, &post.id)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        return Ok(CandidateManifest {
            platform: post.platform.clone(),
            author: post.author.clone(),
            caption: post.caption.unwrap_or_default(),
            suggested_folder: format!("{}/{}", post.platform, post.author),
            items: items
                .into_iter()
                .map(|i| CandidateItem {
                    id: i.id,
                    media_type: i.media_type,
                    mime_type: "application/octet-stream".to_string(),
                    thumbnail_url: i.thumbnail_url.unwrap_or_default(),
                    thumbnail_base64: None,
                    high_res_url: i.cdn_url,
                    audio_url: i.audio_url,
                    suggested_filename: i.suggested_filename,
                    referer: Some(clean_url.to_string()),
                })
                .collect(),
        });
    }

    let extracted: ExtractedMediaMetadata = extract_media(clean_url, Some(&state.config.downloader))
        .await
        .map_err(|e| AppError::BadRequest(format!("Link extraction failed: {e}")))?;

    let post_uuid = Uuid::new_v4().to_string();
    let total_items = extracted.items.len();

    let mut db_items: Vec<ScrapedMediaItemRecord> = Vec::with_capacity(total_items);
    let mut candidate_items: Vec<CandidateItem> = Vec::with_capacity(total_items);

    for (idx, item) in extracted.items.iter().enumerate() {
        let media_type_str = match item.media_type {
            MediaType::Video => "video",
            MediaType::Image => {
                if item.mime_type == "image/gif" {
                    "gif"
                } else {
                    "image"
                }
            }
        };

        let ext = ext_from_mime(&item.mime_type, &item.media_type);
        let suggested_filename = build_suggested_filename(&extracted.caption, idx, total_items, ext);
        let item_id = Uuid::new_v4().to_string();

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

        db_items.push(ScrapedMediaItemRecord {
            id: item_id.clone(),
            media_type: media_type_str.to_string(),
            cdn_url: item.high_res_url.clone(),
            audio_url: item.audio_url.clone(),
            thumbnail_url: item.thumbnail_url.clone(),
            suggested_filename: suggested_filename.clone(),
            width: w,
            height: h,
            variants,
        });

        candidate_items.push(CandidateItem {
            id: item_id,
            media_type: media_type_str.to_string(),
            mime_type: item.mime_type.clone(),
            thumbnail_url: item.thumbnail_url.clone().unwrap_or_default(),
            thumbnail_base64: None,
            high_res_url: item.high_res_url.clone(),
            audio_url: item.audio_url.clone(),
            suggested_filename,
            referer: item.referer_required.clone().or_else(|| Some(clean_url.to_string())),
        });
    }

    let location_name = extracted.location.as_ref().map(|l| l.name.as_str());
    let latitude = extracted.location.as_ref().and_then(|l| l.latitude);
    let longitude = extracted.location.as_ref().and_then(|l| l.longitude);

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
        location_name,
        latitude,
        longitude,
        extracted.published_at.as_deref(),
        &extracted.discovered_post_urls,
        extracted.next_page_url.as_deref(),
        &db_items,
    )
    .await
    .map_err(|e| AppError::Internal(format!("Failed saving scrape: {e}")))?;

    let suggested_folder = format!("{}/{}", extracted.platform, extracted.author);

    Ok(CandidateManifest {
        platform: extracted.platform,
        author: extracted.author,
        caption: extracted.caption,
        suggested_folder,
        items: candidate_items,
    })
}

// ============================================================================
// Shared Download & Ingestion Engine (Enqueues into Stage 0 "assemble")
// ============================================================================

async fn execute_item_downloads(
    state: &AppState,
    auth_user: &AuthUser,
    items: Vec<CandidateItem>,
    target_folder: &str,
    platform: &str,
) -> Result<BatchUploadReceipt, AppError> {
    if items.is_empty() {
        return Err(AppError::BadRequest("No items selected for download".to_string()));
    }

    let sanitized_folder = StorageService::sanitize_folder_path(target_folder);
    let mut results = Vec::new();

    let temp_download_dir = state.config.storage_root.join("temp").join("downloads");
    tokio::fs::create_dir_all(&temp_download_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed creating temp dir: {e}")))?;

    for item in items {
        let temp_file_path = temp_download_dir.join(format!("{}_{}", Uuid::new_v4(), item.suggested_filename));

        let referer = item.referer.as_deref().or_else(|| match platform {
            "instagram" => Some("https://www.instagram.com/"),
            "reddit" => Some("https://www.reddit.com/"),
            _ => None,
        });

        // 1. Download asset directly to disk
        if let Err(e) = download_media(
            &item.high_res_url,
            item.audio_url.as_deref(),
            &temp_file_path,
            referer,
        )
        .await
        {
            let _ = tokio::fs::remove_file(&temp_file_path).await;
            results.push(UploadItemResult {
                file_name: item.suggested_filename,
                status: "error".to_string(),
                id: None,
                relative_path: None,
                message: Some(format!("Download failed: {e}")),
            });
            continue;
        }

        // 2. Query contextual metadata from DB
        let ctx = ScrapesRepo::get_item_context(&state.db, &item.id)
            .await
            .unwrap_or(None);

        let (platform_val, author, caption, tags, source_url, published_at, location_name, latitude, longitude) =
            if let Some(c) = ctx {
                (
                    Some(c.platform),
                    Some(c.author),
                    c.caption,
                    c.tags,
                    Some(c.source_url),
                    c.published_at,
                    c.location_name,
                    c.latitude,
                    c.longitude,
                )
            } else {
                (
                    Some(platform.to_string()),
                    None,
                    None,
                    Vec::new(),
                    item.referer.clone(),
                    None,
                    None,
                    None,
                    None,
                )
            };

        // 3. Inject EXIF / QuickTime/MP4 tags into file headers
        let meta_payload = MediaMetadataPayload {
            author: author.as_deref(),
            caption: caption.as_deref(),
            source_url: source_url.as_deref(),
            tags: &tags,
            published_at: published_at.as_deref(),
            location_name: location_name.as_deref(),
            latitude,
            longitude,
        };

        if let Err(err) = inject_metadata(&temp_file_path, &meta_payload).await {
            tracing::warn!(
                file = %item.suggested_filename,
                error = %err,
                "Metadata injection skipped or failed"
            );
        }

        let asset_id = Uuid::new_v4().to_string();
        let file_size_bytes = match tokio::fs::metadata(&temp_file_path).await {
            Ok(m) => m.len() as i64,
            Err(_) => 0,
        };

        let job_payload = JobPayload {
            author,
            platform: platform_val,
            source_url,
            source_post_id: None,
            caption,
            tags,
            scraped_item_id: Some(item.id.clone()),
            latitude,
            longitude,
        };

        // 4. Enqueue into Stage 0 "assemble" job (worker handles hash, duplicate check, and move)
        let job = DbJob {
            id: Uuid::new_v4().to_string(),
            user_id: auth_user.id.clone(),
            asset_id: asset_id.clone(),
            file_name: item.suggested_filename.clone(),
            rel_path: String::new(),
            folder_path: sanitized_folder.clone(),
            disk_path: temp_file_path.clone(),
            sha256: String::new(),
            job_type: "assemble".to_string(),
            file_size_bytes,
            payload: Some(job_payload),
            ai_faces_done: 0,
            ai_clip_done: 0,
            ai_tags_done: 0,
            ai_poses_done: 0,
        };

        // Write durable WAL entry in SQLite
        if let Err(e) = JobRepo::enqueue_with_status(&state.db, &job, "pending", None).await {
            let _ = tokio::fs::remove_file(&temp_file_path).await;
            results.push(UploadItemResult {
                file_name: item.suggested_filename,
                status: "error".to_string(),
                id: None,
                relative_path: None,
                message: Some(format!("Failed to enqueue downloaded media: {e}")),
            });
        } else {
            // Push directly to Worker 0 channel
            let _ = state.channels.assemble_tx.send(job).await;

            results.push(UploadItemResult {
                file_name: item.suggested_filename,
                status: "queued".to_string(),
                id: Some(asset_id),
                relative_path: None,
                message: Some("Download landed, queued for background processing".to_string()),
            });
        }
    }

    let success_count = results
        .iter()
        .filter(|r| r.status == "queued")
        .count();

    Ok(BatchUploadReceipt {
        total_uploaded: success_count,
        folder: sanitized_folder,
        items: results,
    })
}