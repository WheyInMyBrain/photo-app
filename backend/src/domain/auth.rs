use serde::{Deserialize, Serialize};

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

#[derive(Deserialize)]
pub struct RegisterBiometricPayload {
    pub credential_id: String,
}