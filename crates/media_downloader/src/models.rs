use bytes::Bytes;

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