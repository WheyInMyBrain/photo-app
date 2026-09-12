use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use reqwest::Client;
use serde_json::Value;

use crate::models::{ExtractedMediaMetadata, MediaItem, MediaType};

pub async fn extract_links(input_url: &str) -> Result<ExtractedMediaMetadata> {
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

    let resp = client.get(input_url).send().await.context("Failed to follow URL")?;
    let canonical_url = resp.url().to_string();
    let clean_url = canonical_url.split('?').next().unwrap_or(&canonical_url).trim_end_matches('/');
    let json_endpoint = format!("{clean_url}.json?raw_json=1");

    let json_resp = client.get(&json_endpoint).send().await?;
    if !json_resp.status().is_success() {
        bail!("Reddit API returned HTTP {}", json_resp.status());
    }

    let payload: Value = json_resp.json().await?;
    let post_data = payload
        .get(0)
        .and_then(|v| v.get("data"))
        .and_then(|v| v.get("children"))
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("data"))
        .context("Could not find post data")?;

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

    let mut items = Vec::new();

    // 1. Native Reddit Video (v.redd.it)
    let video_target = post_data
        .get("secure_media")
        .or_else(|| post_data.get("media"))
        .and_then(|m| m.get("reddit_video"));

    if let Some(vid) = video_target {
        if let Some(item) = parse_reddit_video(vid, post_data) {
            items.push(item);
            return Ok(ExtractedMediaMetadata {
                platform: "reddit".to_string(),
                author,
                caption,
                items,
            });
        }
    }

    // 2. Galleries (media_metadata)
    if let Some(gallery_items) = post_data
        .get("gallery_data")
        .and_then(|g| g.get("items"))
        .and_then(|i| i.as_array())
    {
        if let Some(metadata) = post_data.get("media_metadata") {
            for item in gallery_items {
                if let Some(media_id) = item.get("media_id").and_then(|id| id.as_str()) {
                    if let Some(media_obj) = metadata.get(media_id) {
                        if let Some(media_item) = parse_reddit_gallery_node(media_obj) {
                            items.push(media_item);
                        }
                    }
                }
            }
        }
    }

    // 3. Fallback direct image / video links
    if items.is_empty() {
        if let Some(url) = post_data.get("url_overridden_by_dest").and_then(|u| u.as_str()) {
            let is_video = url.ends_with(".mp4") || url.ends_with(".gif");
            let media_type = if is_video { MediaType::Video } else { MediaType::Image };

            let thumbnail_url = extract_preview_thumbnail(post_data).unwrap_or_else(|| url.to_string());

            items.push(MediaItem::new(
                media_type,
                clean_url_str(url),
                None,
                clean_url_str(&thumbnail_url),
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
        items,
    })
}

fn parse_reddit_video(vid: &Value, post_data: &Value) -> Option<MediaItem> {
    let has_audio = vid.get("has_audio").and_then(|b| b.as_bool()).unwrap_or(true);
    let fallback_url = vid.get("fallback_url").and_then(|u| u.as_str())?;

    let (url_path, query_params) = match fallback_url.split_once('?') {
        Some((path, query)) => (path, format!("?{query}")),
        None => (fallback_url, String::new()),
    };

    let audio_url = if has_audio {
        url_path.rsplit_once('/').map(|(base, filename)| {
            // Detect if video is CMAF or DASH
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

    let thumbnail_url = extract_preview_thumbnail(post_data).unwrap_or_else(|| fallback_url.to_string());

    Some(MediaItem::new(
        MediaType::Video,
        clean_url_str(fallback_url),
        audio_url,
        clean_url_str(&thumbnail_url),
    ))
}

fn parse_reddit_gallery_node(media_obj: &Value) -> Option<MediaItem> {
    let thumbnail_url = media_obj
        .get("p")
        .and_then(|arr| arr.as_array())
        .and_then(|arr| arr.first())
        .and_then(|p| p.get("u"))
        .and_then(|u| u.as_str());

    if let Some(mp4) = media_obj.get("s").and_then(|s| s.get("mp4")).and_then(|u| u.as_str()) {
        return Some(MediaItem::new(
            MediaType::Video,
            clean_url_str(mp4),
            None,
            clean_url_str(thumbnail_url.unwrap_or(mp4)),
        ));
    }

    if let Some(img) = media_obj.get("s").and_then(|s| s.get("u")).and_then(|u| u.as_str()) {
        return Some(MediaItem::new(
            MediaType::Image,
            clean_url_str(img),
            None,
            clean_url_str(thumbnail_url.unwrap_or(img)),
        ));
    }

    None
}

fn extract_preview_thumbnail(post_data: &Value) -> Option<String> {
    let images = post_data.get("preview").and_then(|p| p.get("images"))?.as_array()?;
    let first_img = images.first()?;

    if let Some(resolutions) = first_img.get("resolutions").and_then(|r| r.as_array()) {
        if let Some(smallest) = resolutions.first().and_then(|r| r.get("url")).and_then(|u| u.as_str()) {
            return Some(smallest.to_string());
        }
    }

    first_img
        .get("source")
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
}

fn clean_url_str(raw: &str) -> String {
    raw.replace("&amp;", "&").replace(r"\/", "/")
}