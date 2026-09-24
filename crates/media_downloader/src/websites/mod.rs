pub mod generic_browser;
pub mod generic_request;
pub mod hls;
pub mod instagram;
pub mod reddit;

use crate::config::DownloaderConfig;
use crate::models::{ExtractedMediaMetadata, MediaItem, MediaType};
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

pub trait Extractor: Send + Sync {
    fn supports(&self, url: &str) -> bool;
    fn extract<'a>(
        &'a self,
        url: &'a str,
        config: Option<&'a DownloaderConfig>,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMediaMetadata>> + Send + 'a>>;
}

pub async fn route_and_extract(
    url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    let lower_url = url.to_lowercase();

    // 1. Route to site-specific extractors or cascade to generic scrapers
    let mut meta = if instagram::InstagramExtractor.supports(url) {
        instagram::InstagramExtractor.extract(url, config).await?
    } else if reddit::RedditExtractor.supports(url) {
        reddit::RedditExtractor.extract(url, config).await?
    } else {
        let is_video_watch_url = lower_url.contains("/watch")
            || lower_url.contains("/movie")
            || lower_url.contains("/video")
            || lower_url.contains("/film")
            || lower_url.contains("/series")
            || lower_url.contains("/stream")
            || lower_url.contains("/play")
            || lower_url.contains("/v/");

        match generic_request::scrape(url).await {
            Ok(res) => {
                let has_videos = res.items.iter().any(|i| i.media_type == MediaType::Video);

                // Escalate dynamic JS video players to headless browser if static request found no videos
                if is_video_watch_url && !has_videos {
                    match generic_browser::sniff(url, config).await {
                        Ok(mut browser_res) => {
                            if browser_res.post_text.is_none() {
                                browser_res.post_text = res.post_text;
                            }
                            if browser_res.published_at.is_none() {
                                browser_res.published_at = res.published_at;
                            }
                            if browser_res.tags.is_empty() {
                                browser_res.tags = res.tags;
                            }
                            if browser_res.discovered_post_urls.is_empty() {
                                browser_res.discovered_post_urls = res.discovered_post_urls;
                            }
                            browser_res
                        }
                        Err(_) => res,
                    }
                } else {
                    res
                }
            }
            Err(_) => generic_browser::sniff(url, config).await?,
        }
    };

    // 2. Auto-expand master .m3u8 playlists into their highest-resolution streams
    let mut expanded_items = Vec::new();
    for item in meta.items {
        if item.media_type == MediaType::Video && item.high_res_url.contains(".m3u8") {
            if let Ok(variants) = hls::resolve_all_quality_streams(&item.high_res_url, url).await {
                if let Some(best) = variants.last() {
                    expanded_items.push(MediaItem::new(
                        MediaType::Video,
                        item.mime_type,
                        best.dimensions.clone(),
                        item.file_size_bytes,
                        best.video_url.clone(),
                        item.thumbnail_url,
                        best.audio_url.clone(),
                        item.subtitles_url,
                        Some(url.to_string()),
                        item.raw_master_url,
                    ));
                    continue;
                }
            }
        }
        expanded_items.push(item);
    }

    // 3. Central quality normalization & thumbnail guarantee for ALL platforms
    meta.items = expanded_items
        .into_iter()
        .map(normalize_item_qualities)
        .collect();

    Ok(meta)
}

/// Ensures:
/// 1. If variants exist, highest resolution variant is set to high_res_url.
/// 2. If thumbnail_url is None, the lowest resolution variant is chosen.
/// 3. If no variants exist and thumbnail_url is None, defaults to high_res_url.
fn normalize_item_qualities(mut item: MediaItem) -> MediaItem {
    if item.variants.is_empty() {
        if item.thumbnail_url.is_none() {
            item.thumbnail_url = Some(item.high_res_url.clone());
        }
        return item;
    }

    // Sort ascending by area (w * h) or file size:
    // Index 0 = Smallest (ideal for thumbnail)
    // Index Last = Largest (ideal for master download)
    let mut sorted_variants = item.variants.clone();
    sorted_variants.sort_by(|a, b| {
        let area_a = a.dimensions.as_ref().map(|d| d.width * d.height).unwrap_or(0);
        let area_b = b.dimensions.as_ref().map(|d| d.width * d.height).unwrap_or(0);

        area_a
            .cmp(&area_b)
            .then_with(|| a.file_size_bytes.unwrap_or(0).cmp(&b.file_size_bytes.unwrap_or(0)))
    });

    // 1. Guarantee master high-resolution URL
    if let Some(best) = sorted_variants.last() {
        if best.dimensions.is_some() || item.high_res_url.is_empty() {
            item.high_res_url = best.url.clone();
            if item.dimensions.is_none() {
                item.dimensions = best.dimensions.clone();
            }
            if item.file_size_bytes.is_none() {
                item.file_size_bytes = best.file_size_bytes;
            }
        }
    }

    // 2. Guarantee lightweight thumbnail URL
    if item.thumbnail_url.is_none() {
        if let Some(smallest) = sorted_variants.first() {
            item.thumbnail_url = Some(smallest.url.clone());
        } else {
            item.thumbnail_url = Some(item.high_res_url.clone());
        }
    }

    item
}