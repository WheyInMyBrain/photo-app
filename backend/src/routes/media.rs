use axum::{
    extract::{Path as AxumPath, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use tower_http::services::ServeFile;

use crate::db::AssetRepo;
use crate::domain::media::{DynamicFiltersResponse, MediaPageResponse, MediaQuery, SoftDeleteResponse, FavoriteToggleResponse};
use crate::error::AppError;
use crate::AppState;

/// GET /api/media
pub async fn list_media(
    State(state): State<AppState>,
    Query(params): Query<MediaQuery>,
) -> Result<Json<MediaPageResponse>, AppError> {
    if params.cursor_captured_at.is_some() ^ params.cursor_id.is_some() {
        return Err(AppError::BadRequest(
            "Both cursor_captured_at and cursor_id must be supplied together".into(),
        ));
    }

    let page = AssetRepo::query_media(&state.db, &params)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(page))
}

/// POST /api/assets/:id/favorite
pub async fn toggle_favorite(
    State(state): State<AppState>,
    AxumPath(asset_id): AxumPath<String>,
) -> Result<Json<FavoriteToggleResponse>, AppError> {
    let is_favorite = AssetRepo::toggle_favorite(&state.db, &asset_id)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound(format!("Asset {} not found", asset_id)),
            other => AppError::Internal(other.to_string()),
        })?;

    Ok(Json(FavoriteToggleResponse {
        asset_id,
        is_favorite,
    }))
}

/// GET /api/assets/:id/stream
pub async fn stream_asset(
    State(state): State<AppState>,
    AxumPath(asset_id): AxumPath<String>,
    req: Request,
) -> Result<impl IntoResponse, AppError> {
    let info = AssetRepo::get_storage_info(&state.db, &asset_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Asset {} not found", asset_id)))?;

    let file_to_serve = if info.is_video {
        let subfolder = if info.is_private { "originals/private" } else { "originals/public" };
        state.config.storage_root.join(subfolder).join(info.rel_path)
    } else {
        state.config.storage_root.join(info.preview_path)
    };

    if !file_to_serve.exists() {
        return Err(AppError::NotFound("File not found on disk".into()));
    }

    let res = ServeFile::new(file_to_serve)
        .try_call(req)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(res)
}

pub async fn get_available_filters(
    State(state): State<AppState>,
    Query(params): Query<MediaQuery>,
) -> Result<Json<DynamicFiltersResponse>, AppError> {
    let filters = AssetRepo::get_dynamic_filters(&state.db, &params)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(filters))
}

/// POST /api/assets/:id/delete
pub async fn toggle_soft_delete(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<SoftDeleteResponse>, AppError> {
    let deleted_at = AssetRepo::toggle_soft_delete(&state.db, &id)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound(format!("Asset {} not found", id)),
            other => AppError::Internal(other.to_string()),
        })?;

    Ok(Json(SoftDeleteResponse {
        id,
        is_deleted: deleted_at.is_some(),
        deleted_at,
    }))
}

/// POST /api/assets/:id/purge
pub async fn hard_delete_asset(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, AppError> {
    let found = AssetRepo::purge_asset(&state.db, &id, &state.config.storage_root)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !found {
        return Err(AppError::NotFound(format!("Asset {} not found", id)));
    }

    Ok(StatusCode::NO_CONTENT)
}