use axum::{
    extract::FromRequestParts,
    http::request::Parts,
};
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
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

        // 1. Check API Key Header (Shortcuts / CLI)
        let maybe_api_key = parts
            .headers
            .get("X-API-Key")
            .or_else(|| parts.headers.get("X-Vault-API-Key"))
            .and_then(|h| h.to_str().ok())
            .or_else(|| {
                parts
                    .headers
                    .get("Authorization")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer "))
            });

        if let Some(key) = maybe_api_key {
            let user = sqlx::query_as::<_, AuthUser>(
                "SELECT id, username FROM users WHERE api_key = ? LIMIT 1",
            )
            .bind(key)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Internal(format!("Database lookup failure: {e}")))?;

            if let Some(u) = user {
                return Ok(u);
            }
        }

        // 2. Check Session Cookie (Web Browser)
        if let Some(cookie_hdr) = parts.headers.get("Cookie").and_then(|h| h.to_str().ok()) {
            for cookie in cookie_hdr.split(';') {
                let mut parts_kv = cookie.trim().splitn(2, '=');
                if let (Some(k), Some(v)) = (parts_kv.next(), parts_kv.next()) {
                    if k == "app_session" {
                        let user = sqlx::query_as::<_, AuthUser>(
                            "SELECT id, username FROM users WHERE id = ? LIMIT 1",
                        )
                        .bind(v)
                        .fetch_optional(pool)
                        .await
                        .map_err(|e| AppError::Internal(format!("Database lookup failure: {e}")))?;

                        if let Some(u) = user {
                            return Ok(u);
                        }
                    }
                }
            }
        }

        Err(AppError::Unauthorized("Missing or invalid credentials".into()))
    }
}