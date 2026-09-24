// src/utils/media.rs

use super::patterns::{JSON_POSTER_RE, META_POSTER_RE, VIDEO_POSTER_RE};
use super::url::{normalize_scraped_url, resolve_relative_url};
use super::BROWSER_UA;
use crate::models::MediaDimensions;
use reqwest::header::{HeaderValue, ACCEPT, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE, REFERER, USER_AGENT};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Clone)]
pub struct ProbedImageMeta {
    pub dimensions: Option<MediaDimensions>,
    pub content_type: Option<String>,
    pub file_size_bytes: Option<u64>,
}

pub async fn probe_image_metadata(
    image_url: &str,
    referer: Option<&str>,
) -> ProbedImageMeta {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(7))
        .build()
    {
        Ok(c) => c,
        Err(_) => return ProbedImageMeta { dimensions: None, content_type: None, file_size_bytes: None },
    };

    let mut req = client
        .get(image_url)
        .header(RANGE, "bytes=0-8191")
        .header(USER_AGENT, BROWSER_UA)
        .header(ACCEPT, "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8");

    if let Some(r) = referer {
        if let Ok(val) = HeaderValue::try_from(r) {
            req = req.header(REFERER, val);
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(_) => return ProbedImageMeta { dimensions: None, content_type: None, file_size_bytes: None },
    };

    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());

    let file_size_bytes = resp
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.rsplit_once('/'))
        .and_then(|(_, total)| total.trim().parse::<u64>().ok())
        .or_else(|| {
            resp.headers()
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok())
        });

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return ProbedImageMeta { dimensions: None, content_type, file_size_bytes },
    };

    let dimensions = match imagesize::blob_size(&bytes) {
        Ok(dim) => Some(MediaDimensions {
            width: dim.width,
            height: dim.height,
        }),
        Err(_) => None,
    };

    ProbedImageMeta {
        dimensions,
        content_type,
        file_size_bytes,
    }
}

pub fn infer_image_mime(url: &str) -> &'static str {
    let lower = url.to_lowercase();
    if lower.contains(".png") || lower.contains(".pnj") {
        "image/png"
    } else if lower.contains(".webp") {
        "image/webp"
    } else if lower.contains(".gif") {
        "image/gif"
    } else if lower.contains(".avif") {
        "image/avif"
    } else {
        "image/jpeg"
    }
}

pub fn infer_video_mime(url: &str) -> &'static str {
    let lower = url.to_lowercase();
    if lower.contains(".m3u8") {
        "application/x-mpegURL"
    } else if lower.contains(".webm") && !lower.contains(".webmanifest") {
        "video/webm"
    } else {
        "video/mp4"
    }
}

pub fn is_video_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains(".m3u8") || lower.contains(".mp4") || lower.contains(".webm")
}

pub fn is_image_candidate(url: &str) -> bool {
    let lower = url.to_lowercase();
    if lower.starts_with("data:") {
        return false;
    }
    let clean = lower.split('?').next().unwrap_or(&lower);
    clean.ends_with(".jpg")
        || clean.ends_with(".jpeg")
        || clean.ends_with(".png")
        || clean.ends_with(".pnj")
        || clean.ends_with(".webp")
        || clean.ends_with(".gif")
        || clean.ends_with(".avif")
        || lower.contains("tumblr.com/")
        || lower.contains("/thumbnail/")
}

/// Scans HTML for video poster attributes, og:image, and player JSON thumbnail declarations
pub fn extract_video_posters(base_url: &str, html: &str) -> Vec<String> {
    let mut posters = Vec::new();
    let mut seen = HashSet::new();

    // 1. <video poster="...">
    for caps in VIDEO_POSTER_RE.captures_iter(html) {
        if let Some(matched) = caps.get(1) {
            let full_url = resolve_relative_url(base_url, matched.as_str());
            if seen.insert(full_url.clone()) {
                posters.push(full_url);
            }
        }
    }

    // 2. <meta property="og:image" ...> or twitter:image
    for caps in META_POSTER_RE.captures_iter(html) {
        if let Some(matched) = caps.get(1) {
            let full_url = resolve_relative_url(base_url, matched.as_str());
            if seen.insert(full_url.clone()) {
                posters.push(full_url);
            }
        }
    }

    // 3. JSON/Player config keys ("poster": "...", "thumbnailUrl": "...")
    for caps in JSON_POSTER_RE.captures_iter(html) {
        if let Some(matched) = caps.get(1) {
            let clean = normalize_scraped_url(matched.as_str());
            if clean.starts_with("http") || clean.starts_with('/') {
                let full_url = resolve_relative_url(base_url, &clean);
                if seen.insert(full_url.clone()) {
                    posters.push(full_url);
                }
            }
        }
    }

    posters
}

/// Correlates a video URL with discovered thumbnail images by matching filename/token stems
pub fn correlate_video_thumbnail<'a>(video_url: &str, candidate_images: &'a [String]) -> Option<&'a String> {
    let video_token = extract_url_stem(video_url)?;
    if video_token.len() < 4 {
        return None;
    }

    candidate_images
        .iter()
        .find(|img| img.contains(video_token))
}

fn extract_url_stem(url: &str) -> Option<&str> {
    let path = url.split('?').next().unwrap_or(url);
    let filename = path.rsplit('/').next().unwrap_or(path);
    let stem = filename.split('.').next().unwrap_or(filename);
    
    // Drop resolution suffixes like _1080p, _720p to isolate the ID hash
    let clean = stem
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == 'p' || c == '_');

    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}