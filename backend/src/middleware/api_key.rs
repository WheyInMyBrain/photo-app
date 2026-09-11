use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub async fn require_api_key(
    req: Request<Body>,
    next: Next,
) -> Response {
    let expected_key = match std::env::var("VAULT_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "VAULT_API_KEY environment variable not configured" })),
            )
                .into_response();
        }
    };

    let provided_key = req
        .headers()
        .get("X-Vault-API-Key")
        .and_then(|h| h.to_str().ok());

    match provided_key {
        Some(k) if k == expected_key => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid or missing API key" })),
        )
            .into_response(),
    }
}