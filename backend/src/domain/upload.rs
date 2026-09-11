use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct UploadItemResult {
    pub file_name: String,
    pub status: String, // "queued", "duplicate", or "error"
    pub id: Option<String>,
    pub relative_path: Option<String>,
    pub message: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct BatchUploadReceipt {
    pub total_uploaded: usize,
    pub folder: String,
    pub is_private: bool,
    pub items: Vec<UploadItemResult>,
}