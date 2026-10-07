// photo-app/backend/src/routes/upload/batch.rs

use axum::{extract::State, response::Json};
use media_processing::StorageService;
use uuid::Uuid;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;

use db::domain::{
    CheckUploadRequest, CheckUploadResponse, FinishBatchRequest,
    FinishBatchResponse, StartBatchResponse,
};
use db::{AlbumRepo, AssetRepo, JobRepo, UploadRepo};

/// POST /api/upload/batch/start
pub async fn start_batch(
    State(state): State<AppState>,
    _auth_user: AuthUser,
) -> Result<Json<StartBatchResponse>, AppError> {
    let batch_id = UploadRepo::new_batch_id();

    // Pause background workers to free full I/O and CPU for network uploads
    state.pause_processing();

    Ok(Json(StartBatchResponse { batch_id }))
}

/// POST /api/upload/batch/finish
pub async fn finish_batch(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<FinishBatchRequest>,
) -> Result<Json<FinishBatchResponse>, AppError> {
    // 1. Atomically promote all 'staged' jobs of this batch to 'pending' in PostgreSQL
    let released_job_ids = JobRepo::commit_staged_batch(&state.db, auth_user.id, &payload.batch_id)
        .await
        .map_err(|e| {
            state.resume_processing_if_idle();
            AppError::Internal(format!("Failed committing batch: {e}"))
        })?;

    let count = released_job_ids.len() as u64;

    // 2. Push each promoted job directly into the in-memory assemble channel
    for job_id in released_job_ids {
        if let Ok(Some(job)) = JobRepo::acquire_job_by_id(&state.db, job_id).await {
            let _ = state.channels.assemble_tx.send(job).await;
        }
    }

    // 3. Decrement active uploads; workers resume if no other batches are transferring
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

    // 1. O(1) Index lookup on UNIQUE(user_id, sha256)
    let check_res = AssetRepo::check_duplicate_by_sha256(&state.db, auth_user.id, &sha256)
        .await
        .map_err(|e| AppError::Internal(format!("Database lookup error: {e}")))?;

    // 2. If it exists and is active, link it into the destination folder/album if specified
    if check_res.exists && !check_res.is_deleted {
        if let Some(ref asset_id_str) = check_res.asset_id {
            if let Ok(asset_id) = Uuid::parse_str(asset_id_str) {
                let folder_candidate = payload.folder.unwrap_or_default();
                let sanitized_folder = StorageService::sanitize_folder_path(&folder_candidate);

                if !sanitized_folder.is_empty() && sanitized_folder != "root" {
                    if let Ok(mut tx) = state.db.begin().await {
                        if AlbumRepo::link_asset_to_folder_albums_tx(
                            &mut tx,
                            auth_user.id,
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
    }

    Ok(Json(check_res))
}