// photo-app/backend/src/routes/upload/mod.rs

pub mod batch;
pub mod downloader;
pub mod files;
pub mod utils;

pub use batch::{check_upload, finish_batch, start_batch};
pub use files::{finalize_chunk, upload_chunk, upload_photo};

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::HeaderMap,
    response::Json,
};
use media_processing::StorageService;
use uuid::Uuid;
use tracing::{error, info};
use chrono::Utc;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;
use crate::services::link_ingest_service::LinkIngestService;

use db::domain::{
    CommitLinkRequest, DbJob, IngestResponse, InspectLinkRequest,
    InspectLinkResponse, InspectResult, RawUploadQuery, UploadItemResult,
    DirectManifestRequest, CandidateItem,
};
use db::scrapes_repo::{ScrapedMediaItemRecord, ScrapesRepo};
use db::JobRepo;
use downloader::{execute_item_downloads, resolve_or_scrape_manifest};
use media_downloader::stream_thumbnail_base64;
use utils::{default_camera_folder, resolve_incoming_filename, resolve_user_temp_dir, build_suggested_filename};

/// POST /api/upload/inspect
pub async fn inspect_link(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<InspectLinkRequest>,
) -> Result<Json<InspectResult>, AppError> {
    let mut manifest = resolve_or_scrape_manifest(&state, &auth_user, &payload.url).await?;
    let target_folder = StorageService::sanitize_folder_path(&manifest.suggested_folder);

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

    // =========================================================================
    // Case 1: JSON Payloads (Apple Shortcuts or Selective Imports)
    // =========================================================================
    if content_type.starts_with("application/json") || body.starts_with(b"{") {
        // Sub-case 1A: Selective commit of already resolved media items
        if let Ok(commit_req) = serde_json::from_slice::<CommitLinkRequest>(&body) {
            let target_folder = commit_req
                .folder
                .unwrap_or_else(|| commit_req.platform.clone());

            let state_clone = state.clone();
            let user_clone = auth_user.clone();
            let items = commit_req.selected_items;
            let platform = commit_req.platform;

            // Direct file downloads execute sequentially via execute_item_downloads
            tokio::spawn(async move {
                if let Err(e) = execute_item_downloads(
                    &state_clone,
                    &user_clone,
                    items,
                    &target_folder,
                    &platform,
                )
                .await
                {
                    error!(error = %e, "Item downloads failed in background");
                }
            });

            return Ok(Json(IngestResponse::File(UploadItemResult {
                file_name: "batch_download".to_string(),
                status: "queued".to_string(),
                id: None,
                relative_path: None,
                message: Some("Items accepted and queued for download".to_string()),
            })));
        }

        // Sub-case 1B: JSON containing a raw link (e.g., {"url": "https://..."})
        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&body) {
            let extracted_url = val
                .get("url")
                .or_else(|| val.get("link"))
                .or_else(|| val.get("target"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string());

            if let Some(target_url) = extracted_url {
                if target_url.starts_with("http://") || target_url.starts_with("https://") {
                    let enqueued = LinkIngestService::enqueue(
                        &state.db,
                        auth_user.id,
                        &target_url,
                        query.folder.as_deref(),
                    )
                    .await
                    .map_err(|e| AppError::Internal(format!("Failed to enqueue link: {e}")))?;

                    info!(
                        url = %target_url,
                        user_id = %auth_user.id,
                        enqueued,
                        "JSON link recorded to persistent queue"
                    );

                    return Ok(Json(IngestResponse::File(UploadItemResult {
                        file_name: target_url,
                        status: "queued".to_string(),
                        id: None,
                        relative_path: None,
                        message: Some(if enqueued {
                            "Link safely queued for sequential download".to_string()
                        } else {
                            "Link already present in queue, processing will continue".to_string()
                        }),
                    })));
                }
            }
        }
    }

    // =========================================================================
    // Case 2: URL Auto-Commit (Raw text link via curl or Shortcuts)
    // =========================================================================
    let is_text_or_url = content_type.starts_with("text/")
        || (body.len() < 2048
            && std::str::from_utf8(&body)
                .map(|s| s.trim().starts_with("http"))
                .unwrap_or(false));

    if is_text_or_url {
        if let Ok(raw_str) = std::str::from_utf8(&body) {
            let trimmed = raw_str.trim().to_string();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                let enqueued = LinkIngestService::enqueue(
                    &state.db,
                    auth_user.id,
                    &trimmed,
                    query.folder.as_deref(),
                )
                .await
                .map_err(|e| AppError::Internal(format!("Failed to enqueue link: {e}")))?;

                info!(
                    url = %trimmed,
                    user_id = %auth_user.id,
                    enqueued,
                    "Raw text link recorded to persistent queue"
                );

                return Ok(Json(IngestResponse::File(UploadItemResult {
                    file_name: trimmed,
                    status: "queued".to_string(),
                    id: None,
                    relative_path: None,
                    message: Some(if enqueued {
                        "Link safely queued for sequential download".to_string()
                    } else {
                        "Link already present in queue, processing will continue".to_string()
                    }),
                })));
            }
        }
    }

    // =========================================================================
    // Case 3: Binary Media Upload (Directly queued to single-row pipeline)
    // =========================================================================
    let file_name = resolve_incoming_filename(&query, &headers, &body);
    let final_folder = query.folder.unwrap_or_else(default_camera_folder);
    let sanitized_folder = StorageService::sanitize_folder_path(&final_folder);

    let staging_root = resolve_user_temp_dir(&state, auth_user.id, "staging").await?;
    let asset_id = Uuid::new_v4();
    let staged_path = staging_root.join(format!("{}.staged", asset_id));

    tokio::fs::write(&staged_path, &body).await.map_err(|e| {
        AppError::Internal(format!("Failed writing staged ingest file: {e}"))
    })?;

    let batch_id_opt = query.batch_id.as_deref();
    let initial_status = if batch_id_opt.is_some() { "staged" } else { "pending" };

    let job = DbJob {
        id: Uuid::new_v4(),
        user_id: auth_user.id,
        asset_id,
        file_name: file_name.clone(),
        rel_path: String::new(),
        folder_path: sanitized_folder,
        disk_path: staged_path,
        sha256: String::new(),
        file_size_bytes: body.len() as i64,
        status: initial_status.to_string(),
        current_stage: "assemble".to_string(),
        payload: None,
        assemble_done: false,
        thumb_done: false,
        ai_faces_done: false,
        ai_clip_done: false,
        ai_tags_done: false,
        ai_poses_done: false,
    };

    JobRepo::enqueue_with_status(&state.db, &job, initial_status, batch_id_opt)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to enqueue ingest job: {e}")))?;

    if batch_id_opt.is_none() {
        let _ = state.channels.assemble_tx.send(job).await;
    }

    Ok(Json(IngestResponse::File(UploadItemResult {
        file_name,
        status: "queued".to_string(),
        id: Some(asset_id.to_string()),
        relative_path: None,
        message: Some("Queued for background processing".to_string()),
    })))
}

/// POST /api/upload/direct-manifest
pub async fn upload_direct_manifest(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<DirectManifestRequest>,
) -> Result<Json<IngestResponse>, AppError> {
    if payload.items.is_empty() {
        return Err(AppError::BadRequest("No media items supplied in manifest".into()));
    }

    let target_folder = payload.folder.unwrap_or_else(|| {
        format!("{}/{}", payload.platform, payload.author)
    });
    let sanitized_folder = StorageService::sanitize_folder_path(&target_folder);

    let post_uuid = Uuid::new_v4();
    let total_items = payload.items.len();
    let mut candidate_items = Vec::with_capacity(total_items);
    let mut db_items = Vec::with_capacity(total_items);

    let parsed_published_at = payload.published_at.as_deref().and_then(|p| {
        chrono::DateTime::parse_from_rfc3339(p).ok().map(|dt| dt.with_timezone(&Utc))
    });

    for (idx, item) in payload.items.into_iter().enumerate() {
        let item_id = Uuid::new_v4();
        let mime = item.mime_type.unwrap_or_else(|| {
            if item.media_type == "video" {
                "video/mp4".to_string()
            } else {
                "image/jpeg".to_string()
            }
        });

        // Use your existing filename helper
        let ext = if item.media_type == "video" { "mp4" } else { "jpg" };
        let caption_for_filename = item.caption.as_deref().unwrap_or(payload.caption.as_deref().unwrap_or(""));
        let suggested_filename = build_suggested_filename(caption_for_filename, idx, total_items, ext);

        let source_post = item.source_post_url.clone().unwrap_or_else(|| payload.source_url.clone());

        // Prepare DB scrape record to maintain historical cache & metadata injection
        db_items.push(ScrapedMediaItemRecord {
            id: item_id,
            media_type: item.media_type.clone(),
            cdn_url: item.high_res_url.clone(),
            audio_url: item.audio_url.clone(),
            thumbnail_url: item.thumbnail_url.clone(),
            suggested_filename: suggested_filename.clone(),
            width: item.width,
            height: item.height,
            variants: Vec::new(),
            caption: item.caption.clone().or_else(|| payload.caption.clone()),
            published_at: parsed_published_at,
            location_name: payload.location_name.clone(),
            latitude: payload.latitude,
            longitude: payload.longitude,
            tags: payload.tags.clone(),
            source_url: Some(source_post.clone()),
        });

        // Candidate item passed straight to execute_item_downloads
        candidate_items.push(CandidateItem {
            id: item_id.to_string(),
            media_type: item.media_type,
            mime_type: mime,
            thumbnail_url: item.thumbnail_url.unwrap_or_default(),
            thumbnail_base64: None,
            high_res_url: item.high_res_url,
            audio_url: item.audio_url,
            suggested_filename,
            referer: Some("https://www.instagram.com/".to_string()),
            caption: item.caption.or_else(|| payload.caption.clone()),
            published_at: payload.published_at.clone(),
            location_name: payload.location_name.clone(),
            latitude: payload.latitude,
            longitude: payload.longitude,
            tags: payload.tags.clone(),
            source_post_url: Some(source_post),
        });
    }

    // Persist post and items to PostgreSQL (preserving deduplication and asset lineage)
    if let Err(e) = ScrapesRepo::save_scraped_post_and_items(
        &state.db,
        post_uuid,
        auth_user.id,
        &payload.platform,
        &payload.source_url,
        &payload.source_url,
        &payload.author,
        payload.caption.as_deref(),
        &payload.tags,
        payload.location_name.as_deref(),
        payload.latitude,
        payload.longitude,
        parsed_published_at,
        &[payload.source_url.clone()],
        None,
        &db_items,
    )
    .await {
        error!(error = %e, "Failed saving direct manifest scrape record to database");
    }

    // Spawn download worker directly
    let state_clone = state.clone();
    let user_clone = auth_user.clone();
    let folder_clone = sanitized_folder.clone();
    let platform_clone = payload.platform.clone();

    tokio::spawn(async move {
        if let Err(e) = execute_item_downloads(
            &state_clone,
            &user_clone,
            candidate_items,
            &folder_clone,
            &platform_clone,
        )
        .await {
            error!(error = %e, "Background item download failed for direct manifest");
        }
    });

    Ok(Json(IngestResponse::File(UploadItemResult {
        file_name: format!("{}_manifest", payload.author),
        status: "queued".to_string(),
        id: Some(post_uuid.to_string()),
        relative_path: Some(sanitized_folder),
        message: Some("Direct manifest received and downloads initiated".to_string()),
    })))
}