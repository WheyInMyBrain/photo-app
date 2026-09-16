pub mod downloader;
pub mod instagram;
pub mod models;
pub mod reddit;

use anyhow::{bail, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bytes::Bytes;
use std::path::Path;
use tokio::task::JoinSet;
use uuid::Uuid;

pub use models::{ExtractedMediaMetadata, MediaItem, MediaType, StagedCandidateItem, StagedManifest};

fn parse_direct_media_link(url: &str) -> Option<ExtractedMediaMetadata> {
    let clean_path = url.split('?').next().unwrap_or(url);
    let path = Path::new(clean_path);
    let ext = path.extension()?.to_str()?.to_lowercase();

    let media_type = match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "heic" | "avif" => MediaType::Image,
        "mp4" | "mov" | "webm" | "mkv" | "m4v" => MediaType::Video,
        _ => return None,
    };

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("direct_asset")
        .to_string();

    Some(ExtractedMediaMetadata {
        platform: "direct".to_string(),
        author: "web".to_string(),
        caption: stem,
        items: vec![MediaItem::new(media_type, url.to_string(), None, url.to_string())],
    })
}

/// Pure upstream extractor: extracts URLs, normalizes filenames, and encodes carousel thumbnails
pub async fn extract_manifest(url: &str) -> Result<StagedManifest> {
    let raw_meta = if let Some(direct_meta) = parse_direct_media_link(url) {
        direct_meta
    } else if url.contains("instagram.com") {
        instagram::extract_links(url).await?
    } else if url.contains("reddit.com") || url.contains("redd.it") {
        reddit::extract_links(url).await?
    } else {
        bail!("Unsupported platform for URL: {url}");
    };

    let clean_caption: String = raw_meta
        .caption
        .chars()
        .take(30)
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase();

    let prefix = if clean_caption.is_empty() { "post" } else { &clean_caption };
    let total = raw_meta.items.len();
    let post_uuid = Uuid::new_v4().to_string();

    let mut candidate_items: Vec<StagedCandidateItem> = raw_meta
        .items
        .into_iter()
        .enumerate()
        .map(|(idx, item)| {
            let media_type_str = match item.media_type {
                MediaType::Video => "video",
                MediaType::Image => "image",
            };

            let lower = item.high_res_url.to_lowercase();
            let ext = if media_type_str == "video" {
                "mp4"
            } else if lower.contains(".gif") {
                "gif"
            } else if lower.contains(".webp") {
                "webp"
            } else if lower.contains(".png") {
                "png"
            } else {
                "jpg"
            };

            let suggested_filename = if total > 1 {
                format!("{prefix}_{}.{ext}", idx + 1)
            } else {
                format!("{prefix}.{ext}")
            };

            StagedCandidateItem {
                id: format!("item_{idx}"),
                media_type: media_type_str.to_string(),
                thumbnail_url: item.thumbnail_url,
                thumbnail_base64: None,
                high_res_url: item.high_res_url,
                audio_url: item.audio_url,
                suggested_filename,
            }
        })
        .collect();

    // Concurrently fetch base64 thumbnails for carousels
    if candidate_items.len() > 1 {
        let mut set = JoinSet::new();
        for (idx, item) in candidate_items.clone().into_iter().enumerate() {
            set.spawn(async move {
                let mut it = item;
                if !it.thumbnail_url.is_empty() {
                    if let Ok(bytes) = download_asset(&it.thumbnail_url, None, "image").await {
                        it.thumbnail_base64 = Some(format!("data:image/jpeg;base64,{}", BASE64.encode(&bytes)));
                    }
                }
                (idx, it)
            });
        }
        let mut ordered = Vec::with_capacity(set.len());
        while let Some(Ok(res)) = set.join_next().await {
            ordered.push(res);
        }
        ordered.sort_by_key(|(idx, _)| *idx);
        candidate_items = ordered.into_iter().map(|(_, it)| it).collect();
    }

    Ok(StagedManifest {
        post_id: post_uuid,
        platform: raw_meta.platform.clone(),
        author: raw_meta.author.clone(),
        caption: raw_meta.caption,
        suggested_folder: format!("{}/{}", raw_meta.platform, raw_meta.author),
        items: candidate_items,
    })
}

pub async fn download_asset(
    high_res_url: &str,
    audio_url: Option<&str>,
    media_type: &str,
) -> Result<Bytes> {
    downloader::download_asset(high_res_url, audio_url, media_type).await
}