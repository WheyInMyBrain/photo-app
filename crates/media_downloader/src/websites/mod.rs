// src/websites/mod.rs

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

    if instagram::InstagramExtractor.supports(url) {
        return instagram::InstagramExtractor.extract(url, config).await;
    }

    if reddit::RedditExtractor.supports(url) {
        return reddit::RedditExtractor.extract(url, config).await;
    }

    // 1. Future website-specific plugins:
    // if lower_url.contains("youtube.com") { return youtube::YoutubeExtractor.extract(url, config).await; }
    // if lower_url.contains("tiktok.com")  { return tiktok::TiktokExtractor.extract(url, config).await; }

    let is_video_watch_url = lower_url.contains("/watch")
        || lower_url.contains("/movie")
        || lower_url.contains("/video")
        || lower_url.contains("/film")
        || lower_url.contains("/series")
        || lower_url.contains("/stream")
        || lower_url.contains("/play")
        || lower_url.contains("/v/");

    // 2. Cascade fallback with smart video page escalation
    let mut meta = match generic_request::scrape(url).await {
        Ok(res) => {
            let has_videos = res.items.iter().any(|i| i.media_type == MediaType::Video);

            // If it's explicitly a video/watch page and HTTP scraping caught 0 video streams,
            // static HTML is insufficient (player is dynamic/JS). Escalate to browser!
            if is_video_watch_url && !has_videos {
                match generic_browser::sniff(url, config).await {
                    Ok(mut browser_res) => {
                        // Merge page metadata if browser lacked it
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
                    Err(_) => res, // Fall back to HTTP result if browser fails
                }
            } else {
                res
            }
        }
        Err(_) => generic_browser::sniff(url, config).await?,
    };

    // 3. Auto-expand any master .m3u8 playlists into their highest-resolution streams
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

    meta.items = expanded_items;
    Ok(meta)
}