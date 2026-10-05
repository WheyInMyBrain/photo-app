// src/models/upload.rs (or src/models.rs)

use axum::body::Bytes;
use serde::{Deserialize, Serialize};

// ============================================================================
// Internal Upload Staging & Core Receipt Models
// ============================================================================

#[derive(Clone, Debug)]
pub struct StagedFile {
    pub file_name: String,
    pub bytes: Bytes,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UploadItemResult {
    pub file_name: String,
    pub status: String, // "queued" | "duplicate" | "error"
    pub id: Option<String>,
    pub relative_path: Option<String>,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BatchUploadReceipt {
    pub total_uploaded: usize,
    pub folder: String,
    pub items: Vec<UploadItemResult>,
}

// ============================================================================
// Link Scraper & Inspection Candidate Models
// ============================================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CandidateItem {
    pub id: String,                       // Stable unique ID
    pub media_type: String,               // "image" | "video" | "gif"
    pub mime_type: String,                // Probed MIME (e.g. "image/webp", "video/mp4")
    pub thumbnail_url: String,            // Lightweight preview URL
    pub thumbnail_base64: Option<String>, // Direct data URI for instant client preview
    pub high_res_url: String,             // Master payload source URL
    pub audio_url: Option<String>,        // Present for split DASH streams (Reddit/YouTube)
    pub suggested_filename: String,       // e.g. "caption_slug_1.webp"
    pub referer: Option<String>,
    pub caption: Option<String>,
    pub published_at: Option<String>,
    pub location_name: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub tags: Vec<String>,
    pub source_post_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CandidateManifest {
    pub platform: String,
    pub author: String,
    pub caption: String,
    pub suggested_folder: String,
    pub items: Vec<CandidateItem>,
}

#[derive(Debug, Deserialize)]
pub struct InspectLinkRequest {
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InspectLinkResponse {
    pub platform: String,
    pub author: String,
    pub caption: String,
    pub suggested_folder: String,
    pub total_items: usize,
    pub items: Vec<CandidateItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "status", content = "data")]
pub enum InspectResult {
    #[serde(rename = "preview")]
    Preview(InspectLinkResponse),
    #[serde(rename = "committed")]
    Committed(BatchUploadReceipt),
}

// ============================================================================
// Endpoint Payloads & Queries
// ============================================================================

/// Query parameters for POST /api/upload/ingest (raw binary mode)
#[derive(Debug, Deserialize, Clone)]
pub struct RawUploadQuery {
    pub folder: Option<String>,
    pub file_name: Option<String>,
    pub ext: Option<String>,
    pub batch_id: Option<String>,
}

/// JSON body for POST /api/upload/ingest (selective commit mode)
#[derive(Debug, Deserialize, Clone)]
pub struct CommitLinkRequest {
    pub platform: String,
    pub folder: Option<String>,
    pub selected_items: Vec<CandidateItem>,
}

/// Unified response from POST /api/upload/ingest
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum IngestResponse {
    #[serde(rename = "file_uploaded")]
    File(UploadItemResult),
    #[serde(rename = "batch_committed")]
    Batch(BatchUploadReceipt),
}

/// Query parameters for POST /api/upload/chunk
#[derive(Debug, Deserialize, Clone)]
pub struct ChunkUploadQuery {
    pub upload_id: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub chunk_size: u64,
}

/// Response returned by POST /api/upload/chunk
#[derive(Debug, Serialize, Clone)]
pub struct ChunkUploadResponse {
    pub upload_id: String,
    pub chunk_index: usize,
    pub received: bool,
}

/// Query parameters for POST /api/upload/chunk/finalize
#[derive(Debug, Deserialize, Clone)]
pub struct FinalizeChunkQuery {
    pub upload_id: String,
    pub file_name: String,
    pub folder: Option<String>,
    pub batch_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckUploadRequest {
    pub sha256: String,
    pub file_name: String,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckUploadResponse {
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    pub is_deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Row projection for O(1) duplicate lookup via UNIQUE(user_id, sha256)
#[derive(Debug, sqlx::FromRow)]
pub struct ExistingAssetRow {
    pub id: String,
    pub deleted_at: Option<String>,
}

#[derive(Serialize)]
pub struct StartBatchResponse {
    pub batch_id: String,
}

#[derive(Deserialize)]
pub struct FinishBatchRequest {
    pub batch_id: String,
}

#[derive(Serialize)]
pub struct FinishBatchResponse {
    pub released_jobs: u64,
    pub status: String,
}