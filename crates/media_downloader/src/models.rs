use bytes::Bytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Video,
}

#[derive(Debug, Clone)]
pub struct MediaItem {
    pub media_type: MediaType,
    pub high_res_url: String,
    pub audio_url: Option<String>,
    pub thumbnail_url: String,
    pub thumbnail_base64: Option<String>,
}

impl MediaItem {
    /// Internal constructor for scrapers that do not know about base64
    pub fn new(
        media_type: MediaType,
        high_res_url: String,
        audio_url: Option<String>,
        thumbnail_url: String,
    ) -> Self {
        Self {
            media_type,
            high_res_url,
            audio_url,
            thumbnail_url,
            thumbnail_base64: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedMediaMetadata {
    pub platform: String,
    pub author: String,
    pub caption: String,
    pub items: Vec<MediaItem>,
}

#[derive(Debug, Clone)]
pub struct DownloadedAsset {
    pub file_name: String,
    pub bytes: Bytes,
}

#[derive(Debug, Clone)]
pub struct DownloadedBatch {
    pub platform: String,
    pub author: String,
    pub caption: String,
    pub target_folder: String,
    pub assets: Vec<DownloadedAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedCandidateItem {
    pub id: String,                         // e.g. "item_0" or DB item id
    pub media_type: String,                 // "image" | "video"
    pub thumbnail_url: String,
    pub thumbnail_base64: Option<String>,
    pub high_res_url: String,
    pub audio_url: Option<String>,
    pub suggested_filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedManifest {
    pub post_id: String,
    pub platform: String,
    pub author: String,
    pub caption: String,
    pub suggested_folder: String,
    pub items: Vec<StagedCandidateItem>,
}