use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, HeaderMap},
};
use sqlx::SqlitePool;

use db::AuthRepo;
use crate::error::AppError;

/// Local Axum extractor for authenticated user identity
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
}

fn extract_api_key(headers: &HeaderMap) -> Option<&str> {
    const API_KEY_HEADERS: [&str; 2] = ["x-api-key", "x-vault-api-key"];

    for name in API_KEY_HEADERS {
        if let Some(val) = headers.get(name) {
            if let Ok(str_val) = val.to_str() {
                return Some(str_val);
            }
        }
    }

    if let Some(auth_val) = headers.get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_val.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Some(token);
            }
        }
    }

    None
}

fn extract_cookie_value<'a>(headers: &'a HeaderMap, target_key: &str) -> Option<&'a str> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;

    for pair in cookie_header.split(';') {
        let mut kv = pair.trim().splitn(2, '=');
        if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
            if k == target_key {
                return Some(v);
            }
        }
    }

    None
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let pool = parts
            .extensions
            .get::<SqlitePool>()
            .ok_or_else(|| AppError::Internal("DB pool extension missing".into()))?;

        // 1. API Key check
        if let Some(key) = extract_api_key(&parts.headers) {
            let record = AuthRepo::find_by_api_key(pool, key)
                .await
                .map_err(|e| AppError::Internal(format!("Database lookup failure: {e}")))?;

            if let Some(u) = record {
                return Ok(AuthUser {
                    id: u.id,
                    username: u.username,
                });
            }
        }

        // 2. Cookie session check
        if let Some(session_id) = extract_cookie_value(&parts.headers, "app_session") {
            let record = AuthRepo::find_by_id(pool, session_id)
                .await
                .map_err(|e| AppError::Internal(format!("Database lookup failure: {e}")))?;

            if let Some(u) = record {
                return Ok(AuthUser {
                    id: u.id,
                    username: u.username,
                });
            }
        }

        Err(AppError::Unauthorized("Missing or invalid credentials".into()))
    }
}