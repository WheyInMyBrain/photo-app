pub mod instagram;
pub mod models;
pub mod reddit;

use anyhow::{bail, Result};
pub use models::{DownloadedAsset, DownloadedBatch, ExtractedMediaMetadata, MediaItem, MediaType};

/// Downloads all high-resolution media from the URL directly into memory buffers
pub async fn download_batch(url: &str) -> Result<DownloadedBatch> {
    if url.contains("instagram.com") {
        instagram::download(url).await
    } else if url.contains("reddit.com") || url.contains("redd.it") {
        reddit::download(url).await
    } else {
        bail!("Unsupported platform for URL: {url}");
    }
}

/// Resolves media links and preview thumbnails without downloading full media files
pub async fn extract_links(url: &str) -> Result<ExtractedMediaMetadata> {
    if url.contains("instagram.com") {
        instagram::extract_links(url).await
    } else if url.contains("reddit.com") || url.contains("redd.it") {
        reddit::extract_links(url).await
    } else {
        bail!("Unsupported platform for URL: {url}");
    }
}