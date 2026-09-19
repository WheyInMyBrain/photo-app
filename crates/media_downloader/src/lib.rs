// src/lib.rs

pub mod config;
pub use config::DownloaderConfig;

pub mod downloader;
pub mod filter;
pub mod models;
pub mod utils;
pub mod websites;

use anyhow::Result;
use std::path::{Path, PathBuf};

pub use downloader::engine::download_to_disk;
pub use filter::{apply_custom_filters, apply_default_filters, DimensionFilterConfig};
pub use models::{ExtractedMediaMetadata, MediaDimensions, MediaItem, MediaType};

/// Primary public entrypoint: extracts raw links, then runs the filter pipeline
pub async fn extract_media(
    url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    // 1. Unfiltered raw extraction passing the unified config down to route_and_extract
    let mut meta = websites::route_and_extract(url, config).await?;

    // 2. Filter out tiny thumbnails and junk
    filter::apply_default_filters(&mut meta);

    Ok(meta)
}

/// Variant entrypoint: skips all filtering to inspect 100% raw assets
pub async fn extract_media_raw(
    url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    websites::route_and_extract(url, config).await
}

/// Decoupled download entrypoint: accepts raw URLs directly
pub async fn download_media<P: AsRef<Path>>(
    video_or_image_url: &str,
    audio_url: Option<&str>,
    destination: P,
    referer: Option<&str>,
) -> Result<PathBuf> {
    downloader::engine::download_to_disk(
        video_or_image_url,
        audio_url,
        destination,
        referer,
    )
    .await
}