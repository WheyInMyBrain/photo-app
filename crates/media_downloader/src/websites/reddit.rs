// src/websites/reddit.rs

use crate::models::{ExtractedMediaMetadata, MediaDimensions, MediaItem, MediaType, MediaVariant};
use crate::websites::hls;
use crate::websites::Extractor;
use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use reqwest::Client;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

pub struct RedditExtractor;

impl Extractor for RedditExtractor {
    fn supports(&self, url: &str) -> bool {
        let lower = url.to_lowercase();
        lower.contains("reddit.com") || lower.contains("redd.it")
    }

    fn extract<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMediaMetadata>> + Send + 'a>> {
        Box::pin(async move { extract_reddit(url).await })
    }
}

pub async fn extract_reddit(input_url: &str) -> Result<ExtractedMediaMetadata> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Reddit/2024.10.0 (iPhone; iOS 17.4.1; Scale/3.00)"),
    );
    headers.insert(ACCEPT, HeaderValue::from_static("application/json, text/html"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));

    let client = Client::builder()
        .cookie_store(true)
        .default_headers(headers)
        .build()?;

    let resp = client.get(input_url).send().await.context("Failed to follow Reddit URL")?;
    let canonical_url = resp.url().to_string();
    let clean_url = canonical_url.split('?').next().unwrap_or(&canonical_url).trim_end_matches('/');
    let json_endpoint = format!("{clean_url}.json?raw_json=1");

    let json_resp = client.get(&json_endpoint).send().await?;
    if !json_resp.status().is_success() {
        bail!("Reddit API returned HTTP {}", json_resp.status());
    }

    let body = json_resp.text().await?;
    let payload: Value = serde_json::from_str(&body)?;

    let post_data = payload
        .get(0)
        .and_then(|v| v.get("data"))
        .and_then(|v| v.get("children"))
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("data"))
        .context("Could not parse Reddit post data")?;

    let author = post_data
        .get("author")
        .and_then(|a| a.as_str())
        .unwrap_or("unknown")
        .to_string();

    let caption = post_data
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let post_text = post_data
        .get("selftext")
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.to_string());

    let published_at = post_data
        .get("created_utc")
        .and_then(|c| c.as_f64())
        .map(|ts| (ts as i64).to_string());

    let mut tags = Vec::new();
    if let Some(flair) = post_data.get("link_flair_text").and_then(|f| f.as_str()) {
        if !flair.trim().is_empty() {
            tags.push(flair.to_string());
        }
    }
    if let Some(sub) = post_data.get("subreddit").and_then(|s| s.as_str()) {
        tags.push(format!("r/{sub}"));
    }

    let mut items = Vec::new();

    // 1. Native Reddit Video (v.redd.it)
    let video_target = post_data
        .get("secure_media")
        .or_else(|| post_data.get("media"))
        .and_then(|m| m.get("reddit_video"));

    if let Some(vid) = video_target {
        if let Some(item) = parse_reddit_video(vid, post_data, input_url).await {
            items.push(item);
            return Ok(ExtractedMediaMetadata {
                platform: "reddit".to_string(),
                author,
                caption,
                post_text,
                published_at,
                tags,
                items,
                next_page_url: None,
                discovered_post_urls: Vec::new(),
                embedded_player_urls: Vec::new(),
            });
        }
    }

    // 2. Animated GIF / Video Preview (preview.reddit_video_preview or preview.images.variants)
    if let Some(gif_item) = parse_reddit_gif_or_preview(post_data, input_url) {
        items.push(gif_item);
        return Ok(ExtractedMediaMetadata {
            platform: "reddit".to_string(),
            author,
            caption,
            post_text,
            published_at,
            tags,
            items,
            next_page_url: None,
            discovered_post_urls: Vec::new(),
            embedded_player_urls: Vec::new(),
        });
    }

    // 3. Galleries (media_metadata)
    if let Some(gallery_items) = post_data
        .get("gallery_data")
        .and_then(|g| g.get("items"))
        .and_then(|i| i.as_array())
    {
        if let Some(metadata) = post_data.get("media_metadata") {
            for item in gallery_items {
                if let Some(media_id) = item.get("media_id").and_then(|id| id.as_str()) {
                    if let Some(media_obj) = metadata.get(media_id) {
                        if let Some(media_item) = parse_reddit_gallery_node(media_obj, input_url) {
                            items.push(media_item);
                        }
                    }
                }
            }
        }
    }

    // 4. Single Image (with full preview resolutions)
    if items.is_empty() {
        if let Some(single_img) = parse_reddit_single_image(post_data, input_url) {
            items.push(single_img);
        }
    }

    // 5. Fallback direct link (url_overridden_by_dest)
    if items.is_empty() {
        if let Some(url) = post_data.get("url_overridden_by_dest").and_then(|u| u.as_str()) {
            let clean = clean_url_str(url);
            let is_video = clean.ends_with(".mp4") || clean.ends_with(".webm");
            let media_type = if is_video { MediaType::Video } else { MediaType::Image };
            let mime = if is_video { "video/mp4" } else { "image/jpeg" };
            let thumbnail_url = extract_preview_thumbnail(post_data).or_else(|| Some(clean.clone()));

            items.push(MediaItem::new(
                media_type,
                mime,
                None,
                None,
                clean.clone(),
                thumbnail_url,
                None,
                None,
                Some("https://www.reddit.com/".to_string()),
                clean,
            ));
        }
    }

    if items.is_empty() {
        bail!("No media items found for this Reddit post.");
    }

    Ok(ExtractedMediaMetadata {
        platform: "reddit".to_string(),
        author,
        caption,
        post_text,
        published_at,
        tags,
        items,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// Parsers
// -----------------------------------------------------------------------------

async fn parse_reddit_video(vid: &Value, post_data: &Value, page_url: &str) -> Option<MediaItem> {
    let fallback_url = clean_url_str(vid.get("fallback_url")?.as_str()?);
    let hls_url = vid.get("hls_url").and_then(|u| u.as_str()).map(clean_url_str);
    let has_audio = vid.get("has_audio").and_then(|b| b.as_bool()).unwrap_or(true);
    let thumbnail_url = extract_preview_thumbnail(post_data).or_else(|| Some(fallback_url.clone()));

    let width = vid.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as usize;
    let height = vid.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as usize;
    let dims = if width > 0 && height > 0 {
        Some(MediaDimensions { width, height })
    } else {
        None
    };

    // Separate DASH/CMAF audio stream fallback
    let (url_path, query_params) = match fallback_url.split_once('?') {
        Some((path, query)) => (path, format!("?{query}")),
        None => (fallback_url.as_str(), String::new()),
    };

    let audio_url = if has_audio {
        url_path.rsplit_once('/').map(|(base, filename)| {
            let audio_file = if filename.starts_with("CMAF_") {
                "CMAF_AUDIO_128.mp4"
            } else {
                "DASH_AUDIO_128.mp4"
            };
            format!("{base}/{audio_file}{query_params}")
        })
    } else {
        None
    };

    // Auto-resolve HLS variants if Reddit returned HLS manifest
    let mut variants = Vec::new();
    let mut resolved_video_url = fallback_url.clone();
    let mut resolved_audio_url = audio_url.clone();

    if let Some(ref hls_link) = hls_url {
        if let Ok(hls_variants) = hls::resolve_all_quality_streams(hls_link, page_url).await {
            for v in &hls_variants {
                variants.push(MediaVariant {
                    url: v.video_url.clone(),
                    dimensions: v.dimensions.clone(),
                    file_size_bytes: None,
                    label: Some(v.resolution.clone()),
                });
            }
            if let Some(best) = hls_variants.last() {
                resolved_video_url = best.video_url.clone();
                if best.audio_url.is_some() {
                    resolved_audio_url = best.audio_url.clone();
                }
            }
        }
    }

    // Default fallback variant
    if variants.is_empty() {
        variants.push(MediaVariant {
            url: fallback_url.clone(),
            dimensions: dims.clone(),
            file_size_bytes: None,
            label: dims.as_ref().map(|d| format!("{}x{}", d.width, d.height)),
        });
    }

    let mut media = MediaItem::new(
        MediaType::Video,
        "video/mp4",
        dims,
        None,
        resolved_video_url.clone(),
        thumbnail_url,
        resolved_audio_url,
        None,
        Some("https://www.reddit.com/".to_string()),
        resolved_video_url,
    );

    media.variants = variants;
    Some(media)
}

fn parse_reddit_single_image(post_data: &Value, _page_url: &str) -> Option<MediaItem> {
    let images = post_data.get("preview")?.get("images")?.as_array()?;
    let first_img = images.first()?;
    let source = first_img.get("source")?;

    let high_res_url = clean_url_str(source.get("url")?.as_str()?);
    let src_w = source.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as usize;
    let src_h = source.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as usize;
    let dims = if src_w > 0 && src_h > 0 {
        Some(MediaDimensions { width: src_w, height: src_h })
    } else {
        None
    };

    let mut variants = Vec::new();

    // Collect all preview renditions
    if let Some(resolutions) = first_img.get("resolutions").and_then(|r| r.as_array()) {
        for res in resolutions {
            if let Some(u) = res.get("url").and_then(|u| u.as_str()) {
                let w = res.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as usize;
                let h = res.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as usize;
                variants.push(MediaVariant {
                    url: clean_url_str(u),
                    dimensions: if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None },
                    file_size_bytes: None,
                    label: Some(format!("{w}w")),
                });
            }
        }
    }

    variants.push(MediaVariant {
        url: high_res_url.clone(),
        dimensions: dims.clone(),
        file_size_bytes: None,
        label: dims.as_ref().map(|d| format!("{}w", d.width)),
    });

    let mut media = MediaItem::new(
        MediaType::Image,
        "image/jpeg",
        dims,
        None,
        high_res_url.clone(),
        None,
        None,
        None,
        Some("https://www.reddit.com/".to_string()),
        high_res_url,
    );

    media.variants = variants;
    Some(media)
}

fn parse_reddit_gallery_node(media_obj: &Value, _page_url: &str) -> Option<MediaItem> {
    let mut variants = Vec::new();

    if let Some(previews) = media_obj.get("p").and_then(|arr| arr.as_array()) {
        for p in previews {
            if let Some(u) = p.get("u").and_then(|u| u.as_str()) {
                let w = p.get("x").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                let h = p.get("y").and_then(|y| y.as_u64()).unwrap_or(0) as usize;
                variants.push(MediaVariant {
                    url: clean_url_str(u),
                    dimensions: if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None },
                    file_size_bytes: None,
                    label: Some(format!("{w}w")),
                });
            }
        }
    }

    // Video slide inside gallery
    if let Some(mp4) = media_obj.get("s").and_then(|s| s.get("mp4")).and_then(|u| u.as_str()) {
        let clean_mp4 = clean_url_str(mp4);
        let w = media_obj.pointer("/s/x").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let h = media_obj.pointer("/s/y").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let dims = if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None };
        let thumbnail = variants.first().map(|v| v.url.clone());

        variants.push(MediaVariant {
            url: clean_mp4.clone(),
            dimensions: dims.clone(),
            file_size_bytes: None,
            label: dims.as_ref().map(|d| format!("{}x{}", d.width, d.height)),
        });

        let mut media = MediaItem::new(
            MediaType::Video,
            "video/mp4",
            dims,
            None,
            clean_mp4.clone(),
            thumbnail,
            None,
            None,
            Some("https://www.reddit.com/".to_string()),
            clean_mp4,
        );
        media.variants = variants;
        return Some(media);
    }

    // Image slide inside gallery
    if let Some(img) = media_obj.get("s").and_then(|s| s.get("u")).and_then(|u| u.as_str()) {
        let clean_img = clean_url_str(img);
        let w = media_obj.pointer("/s/x").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let h = media_obj.pointer("/s/y").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let dims = if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None };

        variants.push(MediaVariant {
            url: clean_img.clone(),
            dimensions: dims.clone(),
            file_size_bytes: None,
            label: dims.as_ref().map(|d| format!("{}w", d.width)),
        });

        let mut media = MediaItem::new(
            MediaType::Image,
            "image/jpeg",
            dims,
            None,
            clean_img.clone(),
            None,
            None,
            None,
            Some("https://www.reddit.com/".to_string()),
            clean_img,
        );
        media.variants = variants;
        return Some(media);
    }

    None
}

fn parse_reddit_gif_or_preview(post_data: &Value, _page_url: &str) -> Option<MediaItem> {
    let thumbnail_url = extract_preview_thumbnail(post_data);

    // Preview video conversion
    if let Some(rvp) = post_data.get("preview").and_then(|p| p.get("reddit_video_preview")) {
        if let Some(fallback_url) = rvp.get("fallback_url").and_then(|u| u.as_str()) {
            let clean_vid = clean_url_str(fallback_url);
            let w = rvp.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as usize;
            let h = rvp.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as usize;
            let dims = if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None };

            return Some(MediaItem::new(
                MediaType::Video,
                "video/mp4",
                dims,
                None,
                clean_vid.clone(),
                thumbnail_url,
                None,
                None,
                Some("https://www.reddit.com/".to_string()),
                clean_vid,
            ));
        }
    }

    // GIF variants inside image previews
    if let Some(images) = post_data.get("preview").and_then(|p| p.get("images")).and_then(|i| i.as_array()) {
        if let Some(first_img) = images.first() {
            if let Some(variants) = first_img.get("variants") {
                if let Some(mp4_url) = variants.pointer("/mp4/source/url").and_then(|u| u.as_str()) {
                    let clean_mp4 = clean_url_str(mp4_url);
                    return Some(MediaItem::new(
                        MediaType::Video,
                        "video/mp4",
                        None,
                        None,
                        clean_mp4.clone(),
                        thumbnail_url,
                        None,
                        None,
                        Some("https://www.reddit.com/".to_string()),
                        clean_mp4,
                    ));
                }
                if let Some(gif_url) = variants.pointer("/gif/source/url").and_then(|u| u.as_str()) {
                    let clean_gif = clean_url_str(gif_url);
                    return Some(MediaItem::new(
                        MediaType::Image,
                        "image/gif",
                        None,
                        None,
                        clean_gif.clone(),
                        thumbnail_url,
                        None,
                        None,
                        Some("https://www.reddit.com/".to_string()),
                        clean_gif,
                    ));
                }
            }
        }
    }

    None
}

fn extract_preview_thumbnail(post_data: &Value) -> Option<String> {
    let images = post_data.get("preview").and_then(|p| p.get("images"))?.as_array()?;
    let first_img = images.first()?;

    if let Some(resolutions) = first_img.get("resolutions").and_then(|r| r.as_array()) {
        if let Some(smallest) = resolutions.first().and_then(|r| r.get("url")).and_then(|u| u.as_str()) {
            return Some(clean_url_str(smallest));
        }
    }

    first_img
        .get("source")
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .map(clean_url_str)
}

fn clean_url_str(raw: &str) -> String {
    raw.replace("&amp;", "&").replace(r"\/", "/")
}