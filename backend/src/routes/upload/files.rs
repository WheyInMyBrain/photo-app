// photo-app/backend/src/routes/upload/files.rs

use axum::{
    body::Body,
    extract::{Multipart, Query, State},
    response::Json,
};
use futures_util::StreamExt;
use media_processing::StorageService;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;

use super::utils::{default_camera_folder, resolve_user_temp_dir};
use db::domain::{
    BatchUploadReceipt, ChunkUploadQuery, ChunkUploadResponse, DbJob,
    FinalizeChunkQuery, RawUploadQuery, UploadItemResult,
};
use db::JobRepo;

/// POST /api/upload
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
    let staging_root = resolve_user_temp_dir(&state, auth_user.id, "staging").await?;

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
                let asset_id = Uuid::new_v4();
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
                let batch_id_opt = query.batch_id.as_deref();
                let initial_status = if batch_id_opt.is_some() { "staged" } else { "pending" };

                let job = DbJob {
                    id: Uuid::new_v4(),
                    user_id: auth_user.id,
                    asset_id,
                    file_name: file_name.clone(),
                    rel_path: String::new(),
                    folder_path: sanitized_folder,
                    disk_path: staged_path.clone(),
                    sha256: String::new(),
                    file_size_bytes: total_bytes,
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

                // 1. Write single durable job row in PostgreSQL
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

                // 2. If unbatched, push directly into Worker 0 (Assemble) channel
                if batch_id_opt.is_none() {
                    let _ = state.channels.assemble_tx.send(job).await;
                }

                results.push(UploadItemResult {
                    file_name,
                    status: "queued".to_string(),
                    id: Some(asset_id.to_string()),
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

/// POST /api/upload/chunk
pub async fn upload_chunk(
    State(state): State<AppState>,
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

    let temp_root = resolve_user_temp_dir(&state, auth_user.id, "chunks").await?;
    let session_dir = temp_root.join(&query.upload_id);
    fs::create_dir_all(&session_dir).await.map_err(|e| {
        AppError::Internal(format!("Failed to create chunk session dir: {e}"))
    })?;

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
    let temp_root = resolve_user_temp_dir(&state, auth_user.id, "chunks").await?;
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

    let asset_id = Uuid::new_v4();
    let final_folder = query.folder.unwrap_or_else(default_camera_folder);
    let sanitized_folder = StorageService::sanitize_folder_path(&final_folder);
    let batch_id_opt = query.batch_id.as_deref();
    let initial_status = if batch_id_opt.is_some() { "staged" } else { "pending" };

    let job = DbJob {
        id: Uuid::new_v4(),
        user_id: auth_user.id,
        asset_id,
        file_name: query.file_name.clone(),
        rel_path: String::new(),
        folder_path: sanitized_folder,
        disk_path: session_dir,
        sha256: String::new(),
        file_size_bytes: 0,
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

    // 1. Write single durable job entry in PostgreSQL
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
        id: Some(asset_id.to_string()),
        relative_path: None,
        message: Some("Upload verified; assembling in background".to_string()),
    }))
}