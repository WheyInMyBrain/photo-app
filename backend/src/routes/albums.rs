use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::db::AlbumRepo;
use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct FolderQuery {
    pub path: Option<String>,
    pub is_private: Option<bool>,
}

#[derive(Serialize)]
pub struct SubAlbum {
    pub name: String,
    pub full_path: String,
    pub media_count: i64,
    pub cover_thumb: Option<String>,
}

#[derive(Serialize)]
pub struct AlbumViewResponse {
    pub current_path: String,
    pub sub_albums: Vec<SubAlbum>,
}

/// GET /api/albums?path=vacation
pub async fn get_album_contents(
    State(state): State<AppState>,
    Query(params): Query<FolderQuery>,
) -> Result<Json<AlbumViewResponse>, AppError> {
    let current_path = params.path.unwrap_or_default().trim_matches('/').to_string();
    let privacy_level = if params.is_private.unwrap_or(false) { 1 } else { 0 };

    let sub_albums_raw = AlbumRepo::get_sub_albums(&state.db, &current_path, privacy_level)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let sub_albums = sub_albums_raw
        .into_iter()
        .map(|r| SubAlbum {
            name: r.name,
            full_path: r.full_path,
            media_count: r.media_count,
            cover_thumb: r.cover_thumb,
        })
        .collect();

    Ok(Json(AlbumViewResponse {
        current_path,
        sub_albums,
    }))
}

#[derive(Deserialize)]
pub struct SuggestionQuery {
    pub query: Option<String>,
    pub is_private: Option<bool>,
}

/// GET /api/albums/suggestions?query=
pub async fn get_folder_suggestions(
    State(state): State<AppState>,
    Query(params): Query<SuggestionQuery>,
) -> Result<Json<Vec<String>>, AppError> {
    let privacy_level = if params.is_private.unwrap_or(false) { 1 } else { 0 };
    let filter = params.query.unwrap_or_default().trim().to_lowercase();

    let raw_paths = AlbumRepo::get_all_folder_paths(&state.db, privacy_level)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let mut all_paths = BTreeSet::new();
    for raw in raw_paths {
        let cleaned = raw.trim().trim_matches('/');
        if cleaned.is_empty() {
            continue;
        }

        let segments: Vec<&str> = cleaned.split('/').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
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