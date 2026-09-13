pub mod downloader;
pub mod instagram;
pub mod models;
pub mod reddit;

use anyhow::{bail, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bytes::Bytes;
use tokio::task::JoinSet;
use std::path::Path;

// Re-export core models
pub use models::{ExtractedMediaMetadata, MediaItem, MediaType};

/// Detects if a URL points directly to an image or video file
fn parse_direct_media_link(url: &str) -> Option<ExtractedMediaMetadata> {
    // Strip query parameters (?token=... or ?raw_json=1) to inspect the path
    let clean_path = url.split('?').next().unwrap_or(url);
    
    let path = Path::new(clean_path);
    let ext = path.extension()?.to_str()?.to_lowercase();

    let media_type = match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "heic" | "avif" => MediaType::Image,
        "mp4" | "mov" | "webm" | "mkv" | "m4v" => MediaType::Video,
        _ => return None,
    };

    // Extract filename stem as a fallback title/caption
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("direct_asset")
        .to_string();

    let item = MediaItem::new(
        media_type,
        url.to_string(),
        None,
        url.to_string(), // For a direct image, thumbnail is the image itself
    );

    Some(ExtractedMediaMetadata {
        platform: "direct".to_string(),
        author: "web".to_string(),
        caption: stem,
        items: vec![item],
    })
}

/// ----------------------------------------------------------------------------
/// 1. Unified Link Extractor (Direct Files / Instagram / Reddit)
/// ----------------------------------------------------------------------------
pub async fn extract_links(url: &str) -> Result<ExtractedMediaMetadata> {
    // 1. Check for direct raw media links first (.jpg, .mp4, etc.)
    if let Some(direct_meta) = parse_direct_media_link(url) {
        return Ok(direct_meta);
    }

    // 2. Scraped social media platforms
    let mut meta = if url.contains("instagram.com") {
        instagram::extract_links(url).await?
    } else if url.contains("reddit.com") || url.contains("redd.it") {
        reddit::extract_links(url).await?
    } else {
        bail!("Unsupported platform for URL: {url}");
    };

    // 3. Multi-item post: convert thumbnails to Base64 concurrently for preview UI
    if meta.items.len() > 1 {
        let mut set = JoinSet::new();

        for (index, item) in meta.items.into_iter().enumerate() {
            set.spawn(async move {
                let mut processed = item;
                if !processed.thumbnail_url.is_empty() {
                    if let Ok(bytes) = download_asset(&processed.thumbnail_url, None, "image").await {
                        let encoded = BASE64.encode(&bytes);
                        processed.thumbnail_base64 = Some(format!("data:image/jpeg;base64,{encoded}"));
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

        indexed_items.sort_by_key(|(idx, _)| *idx);
        meta.items = indexed_items.into_iter().map(|(_, item)| item).collect();
    }

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