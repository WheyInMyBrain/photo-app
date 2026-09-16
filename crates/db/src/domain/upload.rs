use axum::body::Bytes;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct StagedFile {
    pub file_name: String,
    pub bytes: Bytes,
}

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
    pub items: Vec<UploadItemResult>,
}

#[derive(Deserialize)]
pub struct RawUploadQuery {
    pub folder: Option<String>,
    pub file_name: Option<String>,
    pub ext: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct InspectLinkRequest {
    pub url: String,
}

#[derive(Serialize, Debug)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InspectResult {
    Committed(BatchUploadReceipt),
    Preview(InspectLinkResponse),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CandidateItem {
    pub id: String,                    // Stable identifier (e.g. "item_0", "item_1")
    pub media_type: String,            // "image" | "video"
    pub thumbnail_url: String,         // Lightweight image for frontend UI
    pub thumbnail_base64: Option<String>, // Base64 data URI for instant client preview
    pub high_res_url: String,          // CDN master payload
    pub audio_url: Option<String>,     // Present only for split DASH Reddit videos
    pub suggested_filename: String,    // e.g. "caption_slug_1.jpg" or ".mp4"
}

#[derive(Serialize, Debug)]
pub struct InspectLinkResponse {
    pub platform: String,              // "instagram" | "reddit"
    pub author: String,
    pub caption: String,
    pub suggested_folder: String,      // e.g. "instagram/riya.arora_official"
    pub total_items: usize,
    pub items: Vec<CandidateItem>,
}

#[derive(Deserialize, Debug)]
pub struct CommitLinkRequest {
    pub platform: String,
    pub folder: Option<String>,        // If omitted, defaults to "{platform}/{author}"
    pub selected_items: Vec<CandidateItem>,
}

#[derive(Deserialize)]
pub struct ChunkUploadQuery {
    pub upload_id: String,
    pub chunk_index: u32,
    pub chunk_size: u64,
    pub total_chunks: u32,
}

#[derive(Serialize)]
pub struct ChunkUploadResponse {
    pub upload_id: String,
    pub chunk_index: u32,
    pub received: bool,
}

#[derive(Deserialize)]
pub struct FinalizeChunkQuery {
    pub upload_id: String,
    pub file_name: String,
    pub folder: Option<String>,
}