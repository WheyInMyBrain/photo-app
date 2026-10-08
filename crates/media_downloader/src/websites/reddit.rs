// src/websites/reddit.rs

use crate::config::DownloaderConfig;
use crate::models::{ExtractedMediaMetadata, MediaDimensions, MediaItem, MediaType, MediaVariant};
use crate::websites::hls;
use crate::websites::Extractor;
use anyhow::{bail, Context, Result};
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT,
};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tracing::{debug, info, warn};

pub struct RedditExtractor;

impl Extractor for RedditExtractor {
    fn supports(&self, url: &str) -> bool {
        let lower = url.to_lowercase();
        lower.contains("reddit.com") || lower.contains("redd.it")
    }

    fn extract<'a>(
        &'a self,
        url: &'a str,
        _config: Option<&'a DownloaderConfig>,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMediaMetadata>> + Send + 'a>> {
        Box::pin(async move { extract_reddit(url).await })
    }
}

pub async fn extract_reddit(input_url: &str) -> Result<ExtractedMediaMetadata> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
        ),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers.insert(
        "sec-fetch-site",
        HeaderValue::from_static("none"),
    );
    headers.insert(
        "sec-fetch-mode",
        HeaderValue::from_static("navigate"),
    );
    headers.insert(
        "sec-fetch-dest",
        HeaderValue::from_static("document"),
    );

    let client = Client::builder()
        .cookie_store(true)
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    // 1. Follow initial redirects to resolve canonical target (e.g. /s/... share links)
    let initial_resp = client
        .get(input_url)
        .send()
        .await
        .context("Failed following Reddit URL")?;

    let resolved_url = initial_resp.url().to_string();
    debug!(input = %input_url, resolved = %resolved_url, "Resolved final Reddit target URL");

    // 2. Check if the resolved URL is actually a user profile feed
    if let Some(username) = extract_reddit_username(&resolved_url) {
        if !resolved_url.contains("/comments/") && !resolved_url.contains("/s/") {
            info!(username = %username, "Detected Reddit user profile link; starting full crawl");
            return extract_reddit_user_profile(&client, &username).await;
        }
    }

    // 3. Extract as single submission using the canonical resolved URL
    extract_reddit_single_post(&client, &resolved_url).await
}

// -----------------------------------------------------------------------------
// 1. User Profile Timeline Crawler
// -----------------------------------------------------------------------------
async fn extract_reddit_user_profile(
    client: &Client,
    username: &str,
) -> Result<ExtractedMediaMetadata> {
    let mut all_items: Vec<MediaItem> = Vec::new();
    let mut discovered_urls: Vec<String> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut aggregated_tags: HashSet<String> = HashSet::new();
    let mut latest_timestamp: Option<String> = None;

    let mut after: Option<String> = None;
    let mut page_count = 0;
    const MAX_PAGES: usize = 100;

    loop {
        page_count += 1;
        let mut api_url = format!(
            "https://www.reddit.com/user/{username}/submitted.json?raw_json=1&limit=100"
        );
        if let Some(ref cursor) = after {
            api_url.push_str(&format!("&after={cursor}"));
        }

        debug!(page = page_count, url = %api_url, "Fetching user submission page");
        let mut resp = client.get(&api_url).send().await?;

        // Fallback to old.reddit.com if blocked with 403
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            let alt_url = api_url.replace("www.reddit.com", "old.reddit.com");
            debug!(alt_url = %alt_url, "Retrying user profile via old.reddit.com fallback");
            resp = client.get(&alt_url).send().await?;
        }

        if !resp.status().is_success() {
            if all_items.is_empty() {
                bail!("Reddit user profile API returned HTTP {}", resp.status());
            } else {
                warn!(page = page_count, status = %resp.status(), "Pagination halted due to HTTP error");
                break;
            }
        }

        let body = resp.text().await?;
        let payload: Value = serde_json::from_str(&body)?;

        let data_node = match payload.get("data") {
            Some(d) => d,
            None => break,
        };

        let children = match data_node.get("children").and_then(|c| c.as_array()) {
            Some(arr) if !arr.is_empty() => arr,
            _ => break,
        };

        let mut new_on_page = 0;

        for child in children {
            let post = match child.get("data") {
                Some(p) => p,
                None => continue,
            };

            let post_id = post.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if !post_id.is_empty() && !seen_ids.insert(post_id.to_string()) {
                continue;
            }

            let permalink = post
                .get("permalink")
                .and_then(|p| p.as_str())
                .map(|p| format!("https://www.reddit.com{p}"))
                .unwrap_or_default();

            if !permalink.is_empty() {
                discovered_urls.push(permalink.clone());
            }

            let post_title = post.get("title").and_then(|t| t.as_str()).unwrap_or("");
            let post_time = post
                .get("created_utc")
                .and_then(|c| c.as_f64())
                .map(|ts| ts as i64)
                .and_then(format_epoch_timestamp);

            if latest_timestamp.is_none() && post_time.is_some() {
                latest_timestamp = post_time.clone();
            }

            if let Some(sub) = post.get("subreddit").and_then(|s| s.as_str()) {
                aggregated_tags.insert(format!("r/{sub}"));
            }

            let mut post_media_items = extract_post_media_items(post, &permalink).await;

            for item in &mut post_media_items {
                item.source_post_url = Some(permalink.clone());
                if item.caption.is_none() && !post_title.is_empty() {
                    item.caption = Some(post_title.to_string());
                }
                if item.published_at.is_none() {
                    item.published_at = post_time.clone();
                }
            }

            new_on_page += post_media_items.len();
            all_items.extend(post_media_items);
        }

        info!(
            username,
            page = page_count,
            items_found = new_on_page,
            total_items = all_items.len(),
            "Processed profile page"
        );

        let next_cursor = data_node
            .get("after")
            .and_then(|a| a.as_str())
            .map(|s| s.to_string());

        if next_cursor.is_none() || page_count >= MAX_PAGES {
            break;
        }

        after = next_cursor;
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    if all_items.is_empty() {
        bail!("No media items found in submissions for u/{username}");
    }

    Ok(ExtractedMediaMetadata {
        platform: "reddit".to_string(),
        author: username.to_string(),
        caption: format!("User profile feed for u/{username}"),
        post_text: None,
        published_at: latest_timestamp,
        tags: aggregated_tags.into_iter().collect(),
        items: all_items,
        location: None,
        next_page_url: None,
        discovered_post_urls: discovered_urls,
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// 2. Single Post Pipeline
// -----------------------------------------------------------------------------
async fn extract_reddit_single_post(
    client: &Client,
    canonical_url: &str,
) -> Result<ExtractedMediaMetadata> {
    let clean_url = canonical_url
        .split('?')
        .next()
        .unwrap_or(canonical_url)
        .trim_end_matches('/');

    let json_endpoint = format!("{clean_url}.json?raw_json=1");

    let mut json_resp = client.get(&json_endpoint).send().await?;

    // Fallback to old.reddit.com if blocked with 403
    if json_resp.status() == reqwest::StatusCode::FORBIDDEN {
        let alt_endpoint = json_endpoint.replace("www.reddit.com", "old.reddit.com");
        debug!(alt = %alt_endpoint, "403 encountered, trying old.reddit.com fallback");
        json_resp = client.get(&alt_endpoint).send().await?;
    }

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
        .map(|ts| ts as i64)
        .and_then(format_epoch_timestamp);

    let mut tags = Vec::new();
    if let Some(flair) = post_data.get("link_flair_text").and_then(|f| f.as_str()) {
        if !flair.trim().is_empty() {
            tags.push(flair.to_string());
        }
    }
    if let Some(sub) = post_data.get("subreddit").and_then(|s| s.as_str()) {
        tags.push(format!("r/{sub}"));
    }

    let mut items = extract_post_media_items(post_data, canonical_url).await;
    if items.is_empty() {
        bail!("No media items found for this Reddit post.");
    }

    for item in &mut items {
        item.source_post_url = Some(canonical_url.to_string());
        item.caption = Some(caption.clone());
        item.published_at = published_at.clone();
        item.tags = tags.clone();
    }

    Ok(ExtractedMediaMetadata {
        platform: "reddit".to_string(),
        author,
        caption,
        post_text,
        published_at,
        tags,
        items,
        location: None,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// Unified Node Inspector: Handles Video, Previews, Galleries, & Direct URLs
// -----------------------------------------------------------------------------
async fn extract_post_media_items(post_data: &Value, page_url: &str) -> Vec<MediaItem> {
    let mut items = Vec::new();

    // 1. Native Reddit Video (v.redd.it)
    let video_target = post_data
        .get("secure_media")
        .or_else(|| post_data.get("media"))
        .and_then(|m| m.get("reddit_video"));

    if let Some(vid) = video_target {
        if let Some(item) = parse_reddit_video(vid, post_data, page_url).await {
            items.push(item);
            return items;
        }
    }

    // 2. Animated GIF / Video Preview
    if let Some(gif_item) = parse_reddit_gif_or_preview(post_data, page_url) {
        items.push(gif_item);
        return items;
    }

    // 3. Multi-image Galleries
    if let Some(gallery_items) = post_data
        .get("gallery_data")
        .and_then(|g| g.get("items"))
        .and_then(|i| i.as_array())
    {
        if let Some(metadata) = post_data.get("media_metadata") {
            for item in gallery_items {
                if let Some(media_id) = item.get("media_id").and_then(|id| id.as_str()) {
                    if let Some(media_obj) = metadata.get(media_id) {
                        if let Some(media_item) = parse_reddit_gallery_node(media_obj, page_url) {
                            items.push(media_item);
                        }
                    }
                }
            }
        }
    }

    // 4. Single Image (with full variant chain)
    if items.is_empty() {
        if let Some(single_img) = parse_reddit_single_image(post_data, page_url) {
            items.push(single_img);
        }
    }

    // 5. Fallback direct link
    if items.is_empty() {
        if let Some(url) = post_data.get("url_overridden_by_dest").and_then(|u| u.as_str()) {
            let clean = clean_url_str(url);
            let is_video = clean.ends_with(".mp4") || clean.ends_with(".webm");
            let is_image = clean.ends_with(".jpg")
                || clean.ends_with(".jpeg")
                || clean.ends_with(".png")
                || clean.ends_with(".webp");

            if is_video || is_image {
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
    }

    items
}

// -----------------------------------------------------------------------------
// Parsers & Helpers
// -----------------------------------------------------------------------------

fn extract_reddit_username(url: &str) -> Option<String> {
    let clean = url.split('?').next().unwrap_or(url);
    let parts: Vec<&str> = clean.trim_matches('/').split('/').collect();

    // Matches: /user/<name>, /u/<name>
    for (i, part) in parts.iter().enumerate() {
        if (*part == "user" || *part == "u") && i + 1 < parts.len() {
            let name = parts[i + 1].trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }

    None
}

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

    let thumbnail_url = variants.first().map(|v| v.url.clone()).or_else(|| Some(high_res_url.clone()));

    let mut media = MediaItem::new(
        MediaType::Image,
        "image/jpeg",
        dims,
        None,
        high_res_url.clone(),
        thumbnail_url,
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

    if let Some(img) = media_obj.get("s").and_then(|s| s.get("u")).and_then(|u| u.as_str()) {
        let clean_img = clean_url_str(img);
        let w = media_obj.pointer("/s/x").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let h = media_obj.pointer("/s/y").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let dims = if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None };

        let thumbnail = variants.first().map(|v| v.url.clone()).or_else(|| Some(clean_img.clone()));

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
            thumbnail,
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

fn format_epoch_timestamp(epoch_secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(epoch_secs, 0).map(|dt| dt.to_rfc3339())
}