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
}