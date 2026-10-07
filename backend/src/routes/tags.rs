use axum::{
    extract::{Path as AxumPath, State},
    response::Json,
};
use uuid::Uuid;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;
use db::domain::tag::AssetTagItem;
use db::TagRepo;

/// GET /api/assets/{id}/tags
pub async fn get_asset_tags(
    State(state): State<AppState>,
    _auth_user: AuthUser,
    AxumPath(asset_id): AxumPath<Uuid>,
) -> Result<Json<Vec<AssetTagItem>>, AppError> {
    let tags = TagRepo::get_by_asset(&state.db, asset_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(tags))
}