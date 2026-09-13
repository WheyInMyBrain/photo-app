use axum::{
    extract::{Path as AxumPath, Query, Request, State},
    http::{header, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
};
use tower_http::services::ServeFile;

use crate::db::AssetRepo;
use crate::domain::media::{
    DynamicFiltersResponse, FavoriteToggleResponse, MediaPageResponse, MediaQuery,
    SoftDeleteResponse, BatchActionRequest, BatchActionResponse, SimilarMediaItem,
};
use crate::error::AppError;
use crate::AppState;

/// GET /api/media
pub async fn list_media(
    State(state): State<AppState>,
    Query(mut params): Query<MediaQuery>, // <-- Make params `mut`
) -> Result<Json<MediaPageResponse>, AppError> {
    if params.cursor_captured_at.is_some() ^ params.cursor_id.is_some() {
        return Err(AppError::BadRequest(
            "Both cursor_captured_at and cursor_id must be supplied together".into(),
        ));
    }

    // Resolve Hybrid Search (Person entity extraction + CLIP text encoding)
    resolve_hybrid_query(&state, &mut params).await;

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
/// Serves photos strictly from lightweight preview paths.
/// Serves videos from originals with full zero-copy byte-range (HTTP 206) scrubbing via ServeFile.
pub async fn stream_asset(
    State(state): State<AppState>,
    AxumPath(asset_id): AxumPath<String>,
    req: Request,
) -> Result<Response, AppError> {
    let info = AssetRepo::get_storage_info(&state.db, &asset_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Asset {} not found", asset_id)))?;

    let file_to_serve = if info.is_video {
        // Videos: stream the original via zero-copy range chunks
        let subfolder = if info.is_private { "originals/private" } else { "originals/public" };
        state.config.storage_root.join(subfolder).join(info.rel_path)
    } else {
        // Photos: strictly serve the compressed WebP preview, never the original
        let preview = state.config.storage_root.join(&info.preview_path);
        if preview.exists() {
            preview
        } else {
            // Fallback only if preview creation previously failed
            let subfolder = if info.is_private { "originals/private" } else { "originals/public" };
            state.config.storage_root.join(subfolder).join(info.rel_path)
        }
    };

    if !file_to_serve.exists() {
        return Err(AppError::NotFound("File not found on disk".into()));
    }

    // ServeFile handles:
    // - HTTP 206 Partial Content & Range header seeking
    // - HTTP 304 Not Modified & ETag matching
    // - Proper Content-Type & streaming without buffering into RAM
    let mut response = ServeFile::new(file_to_serve)
        .try_call(req)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .into_response();

    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=2592000, immutable"),
    );

    // Disable proxy buffering for instant byte-range scrubbing
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );

    Ok(response)
}

/// GET /api/media/filters
pub async fn get_available_filters(
    State(state): State<AppState>,
    Query(mut params): Query<MediaQuery>, // <-- Make params `mut`
) -> Result<Json<DynamicFiltersResponse>, AppError> {
    // Resolve Hybrid Search so sidebar facet counts match the vector search results
    resolve_hybrid_query(&state, &mut params).await;

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

/// POST /api/assets/batch/delete
pub async fn batch_toggle_soft_delete(
    State(state): State<AppState>,
    Json(payload): Json<BatchActionRequest>,
) -> Result<Json<BatchActionResponse>, AppError> {
    if payload.ids.is_empty() {
        return Ok(Json(BatchActionResponse { affected_count: 0 }));
    }

    let affected_count = AssetRepo::batch_toggle_soft_delete(&state.db, &payload.ids)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(BatchActionResponse { affected_count }))
}

/// POST /api/assets/batch/purge
pub async fn batch_purge_assets(
    State(state): State<AppState>,
    Json(payload): Json<BatchActionRequest>,
) -> Result<Json<BatchActionResponse>, AppError> {
    if payload.ids.is_empty() {
        return Ok(Json(BatchActionResponse { affected_count: 0 }));
    }

    let affected_count = AssetRepo::batch_purge(&state.db, &payload.ids, &state.config.storage_root)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(BatchActionResponse { affected_count }))
}

/// GET /api/assets/:id/similar
pub async fn get_similar_assets(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Vec<SimilarMediaItem>>, AppError> {
    let similar = state.clip_cache.find_similar(&id, 0.55, 12).await;
    Ok(Json(similar))
}

async fn resolve_hybrid_query(state: &AppState, q: &mut MediaQuery) {
    let raw_q = match q.q.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => s,
        _ => return,
    };

    let is_private = q.is_private.unwrap_or(false);
    let mut words: Vec<String> = raw_q.split_whitespace().map(String::from).collect();

    // 1. Check if any word matches an identified person's name in SQLite
    let named_persons: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, name FROM persons WHERE name IS NOT NULL"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    for (pid, name) in named_persons {
        let name_lower = name.to_lowercase();
        if let Some(pos) = words.iter().position(|w| w.to_lowercase() == name_lower) {
            // Append to person_id filter
            q.person_id = match q.person_id.take() {
                Some(existing) => Some(format!("{},{}", existing, pid)),
                None => Some(pid),
            };
            words.remove(pos); // Strip out name so it doesn't pollute CLIP text
            break;
        }
    }

    let visual_prompt = words.join(" ");

    // 2. If visual keywords remain, compute CLIP text embedding
    if !visual_prompt.is_empty() {
        if let Ok(text_vector) = state.clip_engine.extract_text_embedding(&visual_prompt) {
            // 0.24 threshold for text-to-image similarity
            let matches = state.clip_cache.search_by_vector(&text_vector, is_private, 0.24, 200).await;
            let matched_ids: Vec<String> = matches.into_iter().map(|(id, _)| id).collect();

            q.candidate_ids = Some(matched_ids);
            
            // Clear lexical fts query: visual concept search is handled by CLIP embeddings
            q.q = None;
        }
    } else {
        // Only person names were in the search query, clear q.q so it only uses person_id filter
        q.q = None;
    }
}