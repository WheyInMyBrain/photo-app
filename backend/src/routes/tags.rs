use axum::{
    extract::{Path as AxumPath, State},
    response::Json,
};

use crate::db::TagRepo;
use crate::domain::tag::AssetTagItem;
use crate::error::AppError;
use crate::AppState;

/// GET /api/assets/{id}/tags
pub async fn get_asset_tags(
    State(state): State<AppState>,
    AxumPath(asset_id): AxumPath<String>,
) -> Result<Json<Vec<AssetTagItem>>, AppError> {
    let tags = TagRepo::get_by_asset(&state.db, &asset_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(tags))
}