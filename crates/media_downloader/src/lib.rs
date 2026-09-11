pub mod downloader;
pub mod instagram;
pub mod models;
pub mod reddit;

use anyhow::{bail, Result};
use bytes::Bytes;

// Re-export core models
pub use models::{ExtractedMediaMetadata, MediaItem, MediaType};

/// ----------------------------------------------------------------------------
/// 1. Unified Link Extractor (Instagram / Reddit)
/// ----------------------------------------------------------------------------
pub async fn extract_links(url: &str) -> Result<ExtractedMediaMetadata> {
    if url.contains("instagram.com") {
        instagram::extract_links(url).await
    } else if url.contains("reddit.com") || url.contains("redd.it") {
        reddit::extract_links(url).await
    } else {
        bail!("Unsupported platform for URL: {url}");
    }
}

/// ----------------------------------------------------------------------------
/// 2. Global Asset Downloader (Direct CDN fetch + FFmpeg remuxing)
/// ----------------------------------------------------------------------------
pub async fn download_asset(
    high_res_url: &str,
    audio_url: Option<&str>,
    media_type: &str,
) -> Result<Bytes> {
    downloader::download_asset(high_res_url, audio_url, media_type).await
}