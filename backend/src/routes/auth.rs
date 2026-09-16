use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use axum::{
    extract::{FromRequestParts, State},
    http::{header, request::Parts, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use uuid::Uuid;

use crate::{
    error::AppError,
    middleware::auth::AuthUser,
    AppState,
};

use db::AuthRepo;
use db::domain::auth::{
    ApiKeyResponse, AuthStatusResponse, AuthSuccessResponse, LoginPayload,
    RegisterBiometricPayload, RegisterPayload,
};

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/status", get(get_status))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/api-key", get(get_api_key))
        .route("/api/auth/api-key/rotate", post(rotate_api_key))
        .route("/api/auth/register-biometric", post(register_biometric))
}

fn generate_api_key() -> String {
    format!("vlt_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn make_session_cookie(user_id: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "app_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=2592000",
        user_id
    ))
    .unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn make_clear_cookie() -> HeaderValue {
    HeaderValue::from_static("app_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

/// Fallback extractor that yields `Option<AuthUser>` without failing the request
pub struct MaybeAuthUser(pub Option<AuthUser>);

impl<S> FromRequestParts<S> for MaybeAuthUser
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(MaybeAuthUser(Some(user))),
            Err(_) => Ok(MaybeAuthUser(None)),
        }
    }
}

/// GET /api/auth/status
async fn get_status(
    State(state): State<AppState>,
    MaybeAuthUser(auth_user): MaybeAuthUser,
) -> Result<Json<AuthStatusResponse>, AppError> {
    let user = match auth_user {
        Some(u) => u,
        None => {
            return Ok(Json(AuthStatusResponse {
                is_authenticated: false,
                user_id: None,
                username: None,
                display_name: None,
                has_passkey: false,
            }))
        }
    };

    let (display_name, has_passkey) = AuthRepo::get_user_status(&state.db, &user.id)
        .await
        .unwrap_or_default();

    Ok(Json(AuthStatusResponse {
        is_authenticated: true,
        user_id: Some(user.id),
        username: Some(user.username),
        display_name,
        has_passkey,
    }))
}

/// POST /api/auth/register
async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterPayload>,
) -> Result<Response, AppError> {
    let username = payload.username.trim();
    if username.len() < 3 {
        return Err(AppError::BadRequest("Username must be at least 3 characters".into()));
    }
    if payload.password.trim().len() < 6 {
        return Err(AppError::BadRequest("Password must be at least 6 characters".into()));
    }

    let exists = AuthRepo::username_exists(&state.db, username)
        .await
        .unwrap_or(false);

    if exists {
        return Err(AppError::BadRequest("Username already exists".into()));
    }

    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(payload.password.as_bytes())
        .map_err(|e| AppError::Internal(e.to_string()))?
        .to_string();

    let user_id = Uuid::new_v4().to_string();
    let api_key = generate_api_key();
    let clean_display = payload.display_name.as_deref().map(str::trim).filter(|s| !s.is_empty());

    AuthRepo::create_user(
        &state.db,
        &user_id,
        username,
        &password_hash,
        clean_display,
        &api_key,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let response_body = AuthSuccessResponse {
        user_id: user_id.clone(),
        username: username.to_string(),
        display_name: clean_display.map(String::from),
        api_key,
    };

    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, make_session_cookie(&user_id));

    Ok((StatusCode::CREATED, headers, Json(response_body)).into_response())
}

/// POST /api/auth/login
async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginPayload>,
) -> Result<Response, AppError> {
    let user = AuthRepo::find_for_login(&state.db, payload.username.trim())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::BadRequest("Invalid username or password".into()))?;

    let parsed_hash = PasswordHash::new(&user.password_hash)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if Argon2::default()
        .verify_password(payload.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return Err(AppError::BadRequest("Invalid username or password".into()));
    }

    let active_key = match user.api_key {
        Some(k) if !k.is_empty() => k,
        _ => {
            let new_key = generate_api_key();
            let _ = AuthRepo::set_api_key(&state.db, &user.id, &new_key).await;
            new_key
        }
    };

    let response_body = AuthSuccessResponse {
        user_id: user.id.clone(),
        username: user.username,
        display_name: user.display_name,
        api_key: active_key,
    };

    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, make_session_cookie(&user.id));

    Ok((StatusCode::OK, headers, Json(response_body)).into_response())
}

/// POST /api/auth/logout
async fn logout() -> Result<Response, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, make_clear_cookie());
    Ok((StatusCode::OK, headers, Json(true)).into_response())
}

/// GET /api/auth/api-key
async fn get_api_key(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<ApiKeyResponse>, AppError> {
    let key = AuthRepo::get_api_key(&state.db, &auth_user.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let api_key = match key {
        Some(k) if !k.is_empty() => k,
        _ => {
            let new_key = generate_api_key();
            AuthRepo::set_api_key(&state.db, &auth_user.id, &new_key)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;
            new_key
        }
    };

    Ok(Json(ApiKeyResponse { api_key }))
}

/// POST /api/auth/api-key/rotate
async fn rotate_api_key(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<ApiKeyResponse>, AppError> {
    let new_key = generate_api_key();

    AuthRepo::set_api_key(&state.db, &auth_user.id, &new_key)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(ApiKeyResponse { api_key: new_key }))
}

/// POST /api/auth/register-biometric
async fn register_biometric(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<RegisterBiometricPayload>,
) -> Result<Json<bool>, AppError> {
    AuthRepo::upsert_passkey_credential(
        &state.db,
        &payload.credential_id,
        &auth_user.id,
        payload.name.as_deref(),
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(true))
}