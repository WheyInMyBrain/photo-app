pub mod downloader;
pub mod instagram;
pub mod models;
pub mod reddit;

use anyhow::{bail, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bytes::Bytes;
use tokio::task::JoinSet;

// Re-export core models
pub use models::{ExtractedMediaMetadata, MediaItem, MediaType};

/// ----------------------------------------------------------------------------
/// 1. Unified Link Extractor (Instagram / Reddit)
/// ----------------------------------------------------------------------------
pub async fn extract_links(url: &str) -> Result<ExtractedMediaMetadata> {
    let mut meta = if url.contains("instagram.com") {
        instagram::extract_links(url).await?
    } else if url.contains("reddit.com") || url.contains("redd.it") {
        reddit::extract_links(url).await?
    } else {
        bail!("Unsupported platform for URL: {url}");
    };

    // Download each thumbnail via download_asset and convert to base64 concurrently using Tokio
    let mut set = JoinSet::new();

    for (index, item) in meta.items.into_iter().enumerate() {
        set.spawn(async move {
            let mut processed = item;
            if !processed.thumbnail_url.is_empty() {
                if let Ok(bytes) = download_asset(&processed.thumbnail_url, None, "image").await {
                    let encoded = BASE64.encode(&bytes);
                    processed.thumbnail_base64 = Some(encoded);
                }
            }
            (index, processed)
        });
    }

    let mut indexed_items = Vec::with_capacity(set.len());
    while let Some(res) = set.join_next().await {
        if let Ok(indexed_item) = res {
            indexed_items.push(indexed_item);
        }
    }

    // Preserve the exact original media sequence order
    indexed_items.sort_by_key(|(idx, _)| *idx);
    meta.items = indexed_items.into_iter().map(|(_, item)| item).collect();

    Ok(meta)
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