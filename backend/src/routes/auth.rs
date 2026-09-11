use axum::{
    extract::State,
    response::Json,
    routing::{get, post},
    Router,
};
use argon2::{
    password_hash::{
        phc::PasswordHash,
        PasswordHasher, PasswordVerifier,
    },
    Argon2,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::AppError, AppState};

#[derive(Serialize)]
pub struct VaultStatusResponse {
    pub is_initialized: bool,
    pub has_passkey: bool,
}

#[derive(Deserialize)]
pub struct PasswordPayload {
    pub password: String,
}

#[derive(Serialize)]
pub struct UnlockResponse {
    pub token: String,
}

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/api/vault/status", get(get_status))
        .route("/api/vault/setup", post(setup_vault))
        .route("/api/vault/unlock/password", post(unlock_password))
        .route("/api/vault/register-biometric", post(register_biometric))
}

async fn get_status(State(state): State<AppState>) -> Result<Json<VaultStatusResponse>, AppError> {
    let has_security: bool = sqlx::query_scalar("SELECT COUNT(*) > 0 FROM vault_security")
        .fetch_one(&state.db)
        .await
        .unwrap_or(false);

    let has_passkey: bool = sqlx::query_scalar("SELECT COUNT(*) > 0 FROM passkey_credentials")
        .fetch_one(&state.db)
        .await
        .unwrap_or(false);

    Ok(Json(VaultStatusResponse {
        is_initialized: has_security,
        has_passkey,
    }))
}

async fn setup_vault(
    State(state): State<AppState>,
    Json(payload): Json<PasswordPayload>,
) -> Result<Json<UnlockResponse>, AppError> {
    if payload.password.trim().len() < 6 {
        return Err(AppError::BadRequest("Password must be at least 6 characters".into()));
    }

    let argon2 = Argon2::default();
    // In password-hash 0.6+, hash_password takes only the password bytes and auto-generates salt internally
    let password_hash = argon2
        .hash_password(payload.password.as_bytes())
        .map_err(|e| AppError::Internal(e.to_string()))?
        .to_string();

    sqlx::query("INSERT OR REPLACE INTO vault_security (id, password_hash) VALUES (1, ?1)")
        .bind(password_hash)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let token = Uuid::new_v4().to_string();
    Ok(Json(UnlockResponse { token }))
}

async fn unlock_password(
    State(state): State<AppState>,
    Json(payload): Json<PasswordPayload>,
) -> Result<Json<UnlockResponse>, AppError> {
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM vault_security WHERE id = 1")
        .fetch_one(&state.db)
        .await
        .map_err(|_| AppError::BadRequest("Vault not initialized".into()))?;

    let parsed_hash = PasswordHash::new(&hash).map_err(|e| AppError::Internal(e.to_string()))?;

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return Err(AppError::BadRequest("Incorrect password".into()));
    }

    let token = Uuid::new_v4().to_string();
    Ok(Json(UnlockResponse { token }))
}

#[derive(Deserialize)]
pub struct RegisterBiometricPayload {
    pub credential_id: String,
}

async fn register_biometric(
    State(state): State<AppState>,
    Json(payload): Json<RegisterBiometricPayload>,
) -> Result<Json<bool>, AppError> {
    sqlx::query("INSERT OR REPLACE INTO passkey_credentials (id, public_key) VALUES (?1, X'00')")
        .bind(&payload.credential_id)
        .execute(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(true))
}