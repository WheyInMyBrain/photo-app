// src/websites/generic_request.rs

use crate::models::{ExtractedMediaMetadata, MediaItem, MediaType};
use crate::utils::{
    correlate_video_thumbnail, discover_next_page, discover_post_links, extract_embedded_players,
    extract_post_text, extract_published_time, extract_tags, extract_video_posters, infer_image_mime,
    infer_video_mime, is_video_url, normalize_scraped_url, probe_image_metadata,
    resolve_relative_url, ATTR_RE, BROWSER_UA, IMG_RE, SRCSET_RE, TITLE_RE, VIDEO_RE,
};
use anyhow::{bail, Result};
use futures::future::join_all;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use std::collections::HashSet;
use std::time::Duration;

pub async fn scrape(url: &str) -> Result<ExtractedMediaMetadata> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(8))
        .build()?;

    let resp = client.get(url).send().await?;
    let status = resp.status();

    if status.as_u16() == 403 || status.as_u16() == 503 {
        bail!("Anti-bot challenge or Cloudflare block detected (Status: {status})");
    }

    if !status.is_success() {
        bail!("HTTP request failed with status: {status}");
    }

    let html = resp.text().await?;

    let page_title = TITLE_RE
        .captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| "web_media".to_string());

    let mut video_candidates = Vec::new();
    let mut image_candidates = Vec::new();
    let mut seen = HashSet::new();

    // 1. Raw video regex match
    for caps in VIDEO_RE.captures_iter(&html) {
        if let Some(matched) = caps.get(0) {
            let clean_url = normalize_scraped_url(matched.as_str());
            if is_video_url(&clean_url) && seen.insert(clean_url.clone()) {
                video_candidates.push(clean_url);
            }
        }
    }

    // 2. Direct media URLs
    for caps in IMG_RE.captures_iter(&html) {
        if let Some(matched) = caps.get(0) {
            let clean_url = normalize_scraped_url(matched.as_str());
            if seen.insert(clean_url.clone()) {
                image_candidates.push(clean_url);
            }
        }
    }

    // 3. Image tag attributes
    for caps in ATTR_RE.captures_iter(&html) {
        if let Some(matched) = caps.get(1) {
            let raw_val = matched.as_str().trim();
            if !raw_val.is_empty() && raw_val != "/" && !raw_val.starts_with("data:") {
                let full_url = resolve_relative_url(url, raw_val);
                if seen.insert(full_url.clone()) {
                    image_candidates.push(full_url);
                }
            }
        }
    }

    // 4. Responsive srcset sets
    for caps in SRCSET_RE.captures_iter(&html) {
        if let Some(matched) = caps.get(1) {
            for entry in matched.as_str().split(',') {
                let candidate = entry.trim().split_whitespace().next().unwrap_or("");
                if !candidate.is_empty() {
                    let full_url = resolve_relative_url(url, candidate);
                    if seen.insert(full_url.clone()) {
                        image_candidates.push(full_url);
                    }
                }
            }
        }
    }

    // Discovered video poster tags (<video poster="...">, og:image, etc.)
    let posters = extract_video_posters(url, &html);

    let mut items = Vec::new();

    for v_url in video_candidates {
        let mime = infer_video_mime(&v_url);
        let thumbnail = correlate_video_thumbnail(&v_url, &posters)
            .cloned()
            .or_else(|| posters.first().cloned());

        items.push(MediaItem::new(
            MediaType::Video,
            mime,
            None,
            None,
            v_url.clone(),
            thumbnail,
            None,
            None,
            Some(url.to_string()),
            v_url,
        ));
    }

    let page_ref = url.to_string();
    let img_probe_tasks = image_candidates.into_iter().map(|img_url| {
        let referer = page_ref.clone();
        async move {
            let probed = probe_image_metadata(&img_url, Some(&referer)).await;
            (img_url, probed)
        }
    });

    let probed_results = join_all(img_probe_tasks).await;

    for (img_url, probed) in probed_results {
        let mime = probed
            .content_type
            .unwrap_or_else(|| infer_image_mime(&img_url).to_string());

        items.push(MediaItem::new(
            MediaType::Image,
            mime,
            probed.dimensions,
            probed.file_size_bytes,
            img_url.clone(),
            None,
            None,
            None,
            Some(url.to_string()),
            img_url,
        ));
    }

    if items.is_empty() {
        bail!("No media items found on page");
    }

    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "web".to_string());

    Ok(ExtractedMediaMetadata {
        platform: extract_site_name(&host),
        author: host,
        caption: page_title,
        post_text: extract_post_text(&html),
        published_at: extract_published_time(&html),
        tags: extract_tags(&html),
        items,
        next_page_url: discover_next_page(url, &html),
        discovered_post_urls: discover_post_links(url, &html),
        embedded_player_urls: extract_embedded_players(url, &html),
    })
}

fn extract_site_name(host: &str) -> String {
    let host_without_www = host.strip_prefix("www.").unwrap_or(host);
    let parts: Vec<&str> = host_without_www.split('.').collect();
    if parts.len() >= 2 {
        if parts.len() > 2 {
            parts[parts.len() - 2].to_string()
        } else {
            parts[0].to_string()
        }
    } else {
        host_without_www.to_string()
    }
}