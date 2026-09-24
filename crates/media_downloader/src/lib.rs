pub mod config;
pub use config::DownloaderConfig;

pub mod downloader;
pub mod filter;
pub mod models;
pub mod utils;
pub mod websites;

use anyhow::Result;
use bytes::Bytes;
use std::path::{Path, PathBuf};

pub use downloader::engine::{download_to_disk, stream_thumbnail_base64, stream_to_memory};
pub use filter::{apply_custom_filters, apply_default_filters, DimensionFilterConfig};
pub use models::{ExtractedMediaMetadata, MediaDimensions, MediaItem, MediaType};
pub use downloader::metadata::{inject_metadata, MediaMetadataPayload};

/// Primary public entrypoint: extracts raw links, then runs the filter pipeline
pub async fn extract_media(
    url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    let mut meta = websites::route_and_extract(url, config).await?;
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

/// Decoupled download entrypoint: accepts raw URLs directly to disk
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

/// In-memory streaming entrypoint: fetches raw media bytes directly without writing to disk
pub async fn stream_media(
    url: &str,
    referer: Option<&str>,
) -> Result<(Bytes, String)> {
    downloader::engine::stream_to_memory(url, referer).await
}