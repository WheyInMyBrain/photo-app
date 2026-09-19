// src/models.rs

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaType {
    Video,
    Image,
}

#[derive(Debug, Clone)]
pub struct MediaDimensions {
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone)]
pub struct MediaVariant {
    pub url: String,
    pub dimensions: Option<MediaDimensions>,
    pub file_size_bytes: Option<u64>,
    pub label: Option<String>, // e.g. "4k", "1080p", "640w"
}

#[derive(Debug, Clone)]
pub struct MediaItem {
    pub media_type: MediaType,
    pub mime_type: String,
    pub dimensions: Option<MediaDimensions>,
    pub file_size_bytes: Option<u64>,
    pub high_res_url: String,
    pub thumbnail_url: Option<String>,
    pub audio_url: Option<String>,
    pub subtitles_url: Option<String>,
    pub referer_required: Option<String>,
    pub raw_master_url: String,
    pub variants: Vec<MediaVariant>, 
}

impl MediaItem {
    pub fn new(
        media_type: MediaType,
        mime_type: impl Into<String>,
        dimensions: Option<MediaDimensions>,
        file_size_bytes: Option<u64>,
        high_res_url: impl Into<String>,
        thumbnail_url: Option<String>,
        audio_url: Option<String>,
        subtitles_url: Option<String>,
        referer_required: Option<String>,
        raw_master_url: impl Into<String>,
    ) -> Self {
        let url_str = high_res_url.into();
        let initial_variant = MediaVariant {
            url: url_str.clone(),
            dimensions: dimensions.clone(),
            file_size_bytes,
            label: None,
        };

        Self {
            media_type,
            mime_type: mime_type.into(),
            dimensions,
            file_size_bytes,
            high_res_url: url_str,
            thumbnail_url,
            audio_url,
            subtitles_url,
            referer_required,
            raw_master_url: raw_master_url.into(),
            variants: vec![initial_variant],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedMediaMetadata {
    pub platform: String,
    pub author: String,
    pub caption: String,
    pub post_text: Option<String>,
    pub published_at: Option<String>,
    pub tags: Vec<String>,
    pub items: Vec<MediaItem>,
    pub next_page_url: Option<String>,
    pub discovered_post_urls: Vec<String>,
    pub embedded_player_urls: Vec<String>,
}