use axum::{
    extract::{Path as AxumPath, State},
    response::Json,
};
use serde::Deserialize;

use crate::db::{AssetRepo, PersonRepo};
use crate::domain::person::{AssetFaceDetail, PersonCard};
use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;

/// GET /api/smart-albums/people
pub async fn get_people_overview(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<Vec<PersonCard>>, AppError> {
    let people = PersonRepo::get_overview(&state.db, &auth_user.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(people))
}

/// GET /api/assets/{id}/faces
pub async fn get_asset_faces(
    State(state): State<AppState>,
    auth_user: AuthUser,
    AxumPath(asset_id): AxumPath<String>,
) -> Result<Json<Vec<AssetFaceDetail>>, AppError> {
    let faces = PersonRepo::get_faces_by_asset(&state.db, &auth_user.id, &asset_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(faces))
}

#[derive(Deserialize)]
pub struct NamePersonPayload {
    pub name: String,
}

/// POST /api/persons/{id}/name
pub async fn name_person(
    State(state): State<AppState>,
    auth_user: AuthUser,
    AxumPath(person_id): AxumPath<String>,
    Json(payload): Json<NamePersonPayload>,
) -> Result<Json<bool>, AppError> {
    let trimmed = payload.name.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("Name cannot be empty".into()));
    }

    let affected_assets = PersonRepo::rename_person(&state.db, &auth_user.id, &person_id, trimmed)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    for aid in affected_assets {
        let _ = AssetRepo::sync_search_index(&state.db, &auth_user.id, &aid).await;
    }

    Ok(Json(true))
}

#[derive(Deserialize)]
pub struct ReassignFacePayload {
    pub target_person_id: String,
}

/// POST /api/faces/{face_id}/reassign
pub async fn reassign_face(
    State(state): State<AppState>,
    auth_user: AuthUser,
    AxumPath(face_id): AxumPath<String>,
    Json(payload): Json<ReassignFacePayload>,
) -> Result<Json<bool>, AppError> {
    let affected_asset_id = PersonRepo::reassign_face(
        &state.db,
        &auth_user.id,
        &face_id,
        &payload.target_person_id,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let _ = AssetRepo::sync_search_index(&state.db, &auth_user.id, &affected_asset_id).await;

    Ok(Json(true))
}

/// POST /api/faces/{face_id}/verify
pub async fn verify_face(
    State(state): State<AppState>,
    auth_user: AuthUser,
    AxumPath(face_id): AxumPath<String>,
) -> Result<Json<bool>, AppError> {
    PersonRepo::verify_face(&state.db, &auth_user.id, &face_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(true))
}

#[derive(Deserialize)]
pub struct MergeRequest {
    pub source_person_id: String,
    pub target_person_id: String,
}

/// POST /api/persons/merge
pub async fn merge_persons(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<MergeRequest>,
) -> Result<Json<bool>, AppError> {
    if payload.source_person_id == payload.target_person_id {
        return Err(AppError::BadRequest("Cannot merge a person into themselves".into()));
    }

    let affected_assets = PersonRepo::merge_persons(
        &state.db,
        &auth_user.id,
        &payload.source_person_id,
        &payload.target_person_id,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    for aid in affected_assets {
        let _ = AssetRepo::sync_search_index(&state.db, &auth_user.id, &aid).await;
    }

    Ok(Json(true))
}

/// GET /api/persons/names
pub async fn get_names_directory(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<Vec<PersonCard>>, AppError> {
    let names = PersonRepo::get_name_directory(&state.db, &auth_user.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(names))
}