use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use crate::AppState;

pub async fn require_api_key(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let expected_key = &state.config.vault_api_key;

    if expected_key.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "VAULT_API_KEY is not configured on the server" })),
        )
            .into_response();
    }

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