use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use serde::Deserialize;
use std::collections::BTreeSet;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;
use db::{AlbumRecord, AlbumRepo};

// ---------------------------------------------------------------------------
// 1. Payloads & Query Models
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SuggestionQuery {
    pub query: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateAlbumRequest {
    pub title: String,
    pub description: Option<String>,
    pub album_type: Option<String>, // "MANUAL" or "SMART"
    pub filter_criteria: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct UpdateAlbumRequest {
    pub title: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct AlbumAssetActionRequest {
    pub asset_ids: Vec<String>,
}

#[derive(Deserialize)]
pub struct ReorderAssetsRequest {
    pub asset_ids: Vec<String>,
}

#[derive(Deserialize)]
pub struct SetCoverRequest {
    pub asset_id: Option<String>,
}

#[derive(Deserialize)]
pub struct DeleteAlbumQuery {
    pub delete_media: Option<bool>,
}

// ---------------------------------------------------------------------------
// 2. Folder Suggestions
// ---------------------------------------------------------------------------

/// GET /api/albums/suggestions?query=
pub async fn get_folder_suggestions(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(params): Query<SuggestionQuery>,
) -> Result<Json<Vec<String>>, AppError> {
    let filter = params.query.unwrap_or_default().trim().to_lowercase();

    let raw_paths = AlbumRepo::get_all_folder_paths(&state.db, &auth_user.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let mut all_paths = BTreeSet::new();
    for raw in raw_paths {
        let cleaned = raw.trim().trim_matches('/');
        if cleaned.is_empty() {
            continue;
        }

        let segments: Vec<&str> = cleaned
            .split('/')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let mut prefix = String::new();
        for (i, seg) in segments.iter().enumerate() {
            if i > 0 {
                prefix.push('/');
            }
            prefix.push_str(seg);
            all_paths.insert(prefix.clone());
        }
    }

    let result: Vec<String> = all_paths
        .into_iter()
        .filter(|p| filter.is_empty() || p.to_lowercase().contains(&filter))
        .collect();

    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// 3. Custom Albums Endpoints
// ---------------------------------------------------------------------------

/// GET /api/albums
pub async fn list_albums(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<Vec<AlbumRecord>>, AppError> {
    let albums = AlbumRepo::list_custom_albums(&state.db, &auth_user.id)
        .await
        .map_err(|e| {
            tracing::error!("Error fetching custom albums: {e}");
            AppError::Internal(e.to_string())
        })?;

    Ok(Json(albums))
}

/// GET /api/albums/{id}
pub async fn get_album(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(album_id): Path<String>,
) -> Result<Json<AlbumRecord>, AppError> {
    let album = AlbumRepo::get_album_by_id(&state.db, &album_id, &auth_user.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Album not found".into()))?;

    Ok(Json(album))
}

/// POST /api/albums
pub async fn create_album(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<CreateAlbumRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let title = payload.title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("Album title cannot be empty".into()));
    }

    let album_type = payload.album_type.unwrap_or_else(|| "MANUAL".to_string());
    let filter_criteria_str = payload.filter_criteria.map(|v| v.to_string());

    let album_id = AlbumRepo::create_album(
        &state.db,
        &auth_user.id,
        title,
        payload.description.as_deref(),
        &album_type,
        filter_criteria_str.as_deref(),
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "album_id": album_id
    })))
}

/// PUT /api/albums/{id} (or POST /api/albums/{id}/update)
pub async fn update_album(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(album_id): Path<String>,
    Json(payload): Json<UpdateAlbumRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let title = payload.title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("Album title cannot be empty".into()));
    }

    let updated = AlbumRepo::update_album(
        &state.db,
        &auth_user.id,
        &album_id,
        title,
        payload.description.as_deref(),
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    if !updated {
        return Err(AppError::NotFound("Album not found".into()));
    }

    Ok(Json(serde_json::json!({ "status": "success" })))
}

/// POST /api/albums/{id}/assets
pub async fn add_assets_to_album(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(album_id): Path<String>,
    Json(payload): Json<AlbumAssetActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if payload.asset_ids.is_empty() {
        return Err(AppError::BadRequest("No assets specified".into()));
    }

    let added = AlbumRepo::add_assets(&state.db, &album_id, &auth_user.id, &payload.asset_ids)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "added": added
    })))
}

/// POST /api/albums/{id}/assets/remove
pub async fn remove_assets_from_album(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(album_id): Path<String>,
    Json(payload): Json<AlbumAssetActionRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if payload.asset_ids.is_empty() {
        return Err(AppError::BadRequest("No assets specified".into()));
    }

    let removed = AlbumRepo::remove_assets_from_album(
        &state.db,
        &auth_user.id,
        &album_id,
        &payload.asset_ids,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "removed": removed
    })))
}

/// PUT /api/albums/{id}/reorder (or POST /api/albums/{id}/reorder)
pub async fn reorder_album_assets(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(album_id): Path<String>,
    Json(payload): Json<ReorderAssetsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    AlbumRepo::reorder_album_assets(
        &state.db,
        &auth_user.id,
        &album_id,
        &payload.asset_ids,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(serde_json::json!({ "status": "success" })))
}

/// POST /api/albums/{id}/cover
pub async fn set_album_cover(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(album_id): Path<String>,
    Json(payload): Json<SetCoverRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let updated = match payload.asset_id.as_deref() {
        Some(aid) => {
            AlbumRepo::set_cover_asset(&state.db, &auth_user.id, &album_id, aid)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?
        }
        None => {
            AlbumRepo::set_cover(&state.db, &album_id, &auth_user.id, None)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?
        }
    };

    if !updated {
        return Err(AppError::NotFound("Album not found".into()));
    }

    Ok(Json(serde_json::json!({ "status": "success" })))
}

/// DELETE /api/albums/{id} or POST /api/albums/{id}/delete
pub async fn delete_album(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(album_id): Path<String>,
    Query(params): Query<DeleteAlbumQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let delete_media = params.delete_media.unwrap_or(false);

    let deleted = AlbumRepo::delete_album(&state.db, &auth_user.id, &album_id, delete_media)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if !deleted {
        return Err(AppError::NotFound("Album not found".into()));
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "deleted": true
    })))
}