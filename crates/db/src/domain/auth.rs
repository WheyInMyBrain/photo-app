use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatusResponse {
    pub is_authenticated: bool,
    pub user_id: Option<Uuid>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub has_passkey: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSuccessResponse {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub api_key: String,
}

#[derive(Deserialize)]
pub struct RegisterPayload {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginPayload {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct ApiKeyResponse {
    pub api_key: String,
}

#[derive(Deserialize)]
pub struct RegisterBiometricPayload {
    pub credential_id: String,
    pub name: Option<String>,
}