use axum::{
    extract::{Path as AxumPath, Query, Request, State},
    http::{header, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
};
use tower_http::services::ServeFile;
use tracing::error;
use uuid::Uuid;

use db::AssetRepo;
use db::domain::{
    AssetPoseDetail, BatchActionRequest, BatchActionResponse, DynamicFiltersResponse,
    FavoriteToggleResponse, MapLocationPoint, MapLocationsQuery, MediaPageResponse,
    MediaQuery, SimilarMediaItem, SoftDeleteResponse,
};
use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;

/// GET /api/media
pub async fn list_media(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(mut params): Query<MediaQuery>,
) -> Result<Json<MediaPageResponse>, AppError> {
    let is_timeline = params.sort.as_deref() == Some("timeline");

    // In timeline mode, keyset pagination requires both cursors to preserve total order.
    if is_timeline && (params.cursor_captured_at.is_some() ^ params.cursor_id.is_some()) {
        return Err(AppError::BadRequest(
            "Both cursor_captured_at and cursor_id must be supplied together for timeline pagination".into(),
        ));
    }

    // Resolve Hybrid Search (person names + SIMD CLIP text search scoped to user)
    resolve_hybrid_query(&state, auth_user.id, &mut params).await;

    let page = AssetRepo::query_media(&state.db, auth_user.id, &params)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(page))
}

/// POST /api/assets/:id/favorite
pub async fn toggle_favorite(
    State(state): State<AppState>,
    auth_user: AuthUser,
    AxumPath(asset_id): AxumPath<Uuid>,
) -> Result<Json<FavoriteToggleResponse>, AppError> {
    let is_favorite = AssetRepo::toggle_favorite(&state.db, auth_user.id, asset_id)
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
    auth_user: AuthUser,
    AxumPath(asset_id): AxumPath<Uuid>,
    req: Request,
) -> Result<Response, AppError> {
    let info = AssetRepo::get_storage_info(&state.db, auth_user.id, asset_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Asset {} not found", asset_id)))?;

    let user_storage = state
        .config
        .storage_root
        .join("users")
        .join(auth_user.id.to_string());

    let file_to_serve = if info.is_video {
        // Full original video stream (or route to preview proxy if requested)
        user_storage.join("originals").join(&info.rel_path)
    } else {
        // Properly scoped: storage_root/users/<user_id>/thumbs/a1/b2/<asset_id>_preview.webp
        let preview = user_storage.join("thumbs").join(&info.preview_path);

        if preview.exists() {
            preview
        } else {
            // Fallback to original if preview hasn't been generated yet
            user_storage.join("originals").join(&info.rel_path)
        }
    };

    if !file_to_serve.exists() {
        return Err(AppError::NotFound("File not found on disk".into()));
    }

    let mut response = ServeFile::new(file_to_serve)
        .try_call(req)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .into_response();

    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=2592000, immutable"),
    );

    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );

    Ok(response)
}

/// GET /api/media/filters
pub async fn get_available_filters(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(mut params): Query<MediaQuery>,
) -> Result<Json<DynamicFiltersResponse>, AppError> {
    resolve_hybrid_query(&state, auth_user.id, &mut params).await;

    let filters = AssetRepo::get_dynamic_filters(&state.db, auth_user.id, &params)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(filters))
}

/// POST /api/assets/:id/delete
pub async fn toggle_soft_delete(
    State(state): State<AppState>,
    auth_user: AuthUser,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<SoftDeleteResponse>, AppError> {
    let deleted_at = AssetRepo::toggle_soft_delete(&state.db, auth_user.id, id)
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
    auth_user: AuthUser,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<StatusCode, AppError> {
    let found = AssetRepo::purge_asset(&state.db, auth_user.id, id, &state.config.storage_root)
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
    auth_user: AuthUser,
    Json(payload): Json<BatchActionRequest>,
) -> Result<Json<BatchActionResponse>, AppError> {
    if payload.ids.is_empty() {
        return Ok(Json(BatchActionResponse { affected_count: 0 }));
    }

    let affected_count = AssetRepo::batch_toggle_soft_delete(&state.db, auth_user.id, &payload.ids)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(BatchActionResponse { affected_count }))
}

/// POST /api/assets/batch/purge
pub async fn batch_purge_assets(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<BatchActionRequest>,
) -> Result<Json<BatchActionResponse>, AppError> {
    if payload.ids.is_empty() {
        return Ok(Json(BatchActionResponse { affected_count: 0 }));
    }

    let affected_count = AssetRepo::batch_purge(
        &state.db,
        auth_user.id,
        &payload.ids,
        &state.config.storage_root,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(BatchActionResponse { affected_count }))
}

/// GET /api/assets/:id/similar
pub async fn get_similar_assets(
    State(state): State<AppState>,
    auth_user: AuthUser,
    AxumPath(id): AxumPath<Uuid>,
) -> Result<Json<Vec<SimilarMediaItem>>, AppError> {
    let rows = db::CacheRepo::search_similar_by_asset_id(
        &state.db,
        auth_user.id,
        id,
        0.55,
        12,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let response: Vec<SimilarMediaItem> = rows
        .into_iter()
        .map(|item| SimilarMediaItem {
            id: item.id,
            thumb_path: item.thumb_path,
            mime_type: item.mime_type,
            similarity: item.similarity,
        })
        .collect();

    Ok(Json(response))
}

async fn resolve_hybrid_query(state: &AppState, user_id: Uuid, q: &mut MediaQuery) {
    let raw_q = match q.q.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => s,
        _ => return,
    };

    let mut words: Vec<String> = raw_q.split_whitespace().map(String::from).collect();

    // 1. Resolve matching person identities for this user in PostgreSQL ($1)
    let named_persons: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM persons WHERE user_id = $1 AND name IS NOT NULL"
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    for (pid, name) in named_persons {
        let name_lower = name.to_lowercase();
        if let Some(pos) = words.iter().position(|w| w.to_lowercase() == name_lower) {
            let pid_str = pid.to_string();
            q.person_id = match q.person_id.take() {
                Some(existing) => Some(format!("{},{}", existing, pid_str)),
                None => Some(pid_str),
            };
            words.remove(pos);
            break;
        }
    }

    let visual_prompt = words.join(" ");

    // 2. Text embedding generation via ONNX + pgvector HNSW query in PostgreSQL
    if !visual_prompt.is_empty() {
        let clip_engine = match state.coordinator.ensure_search_engine().await {
            Ok(engine) => engine,
            Err(e) => {
                error!("Failed to acquire CLIP search engine: {}", e);
                return;
            }
        };

        if let Ok(text_vector) = clip_engine.extract_text_embedding(&visual_prompt) {
            match db::CacheRepo::search_by_vector(&state.db, user_id, &text_vector, 0.24, 200).await {
                Ok(matched_ids) => {
                    q.candidate_ids = Some(matched_ids);
                    q.q = None;
                }
                Err(e) => {
                    error!("pgvector search error: {}", e);
                }
            }
        }
    } else {
        q.q = None;
    }
}

/// GET /api/media/locations
pub async fn get_media_locations(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<MapLocationsQuery>,
) -> Result<Json<Vec<MapLocationPoint>>, (StatusCode, String)> {
    match AssetRepo::query_locations(&state.db, user.id, &query).await {
        Ok(points) => Ok(Json(points)),
        Err(e) => {
            error!(
                user_id = %user.id,
                error = %e,
                "Failed to query media map locations"
            );
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error while querying locations".to_string(),
            ))
        }
    }
}

/// GET /api/assets/:id/poses
pub async fn get_asset_poses(
    State(state): State<AppState>,
    auth_user: AuthUser,
    AxumPath(asset_id): AxumPath<Uuid>,
) -> Result<Json<Vec<AssetPoseDetail>>, AppError> {
    let poses = AssetRepo::get_asset_poses(&state.db, auth_user.id, asset_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(poses))
}