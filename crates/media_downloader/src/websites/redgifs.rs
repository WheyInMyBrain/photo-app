// crates/media_downloader/src/websites/redgifs.rs

use crate::config::DownloaderConfig;
use crate::models::{ExtractedMediaMetadata, MediaDimensions, MediaItem, MediaType, MediaVariant};
use crate::websites::Extractor;
use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tracing::{debug, info, warn};

pub struct RedgifsExtractor;

impl Extractor for RedgifsExtractor {
    fn supports(&self, url: &str) -> bool {
        let lower = url.to_lowercase();
        lower.contains("redgifs.com")
    }

    fn extract<'a>(
        &'a self,
        url: &'a str,
        config: Option<&'a DownloaderConfig>,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMediaMetadata>> + Send + 'a>> {
        Box::pin(async move { extract_redgifs(url, config).await })
    }
}

pub async fn extract_redgifs(
    input_url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    let client = create_redgifs_client()?;
    let token = fetch_guest_token(&client).await?;

    // 1. Check if it's a user profile link (e.g. /users/{username})
    if let Some(username) = extract_redgifs_username(input_url) {
        info!(username = %username, "Detected RedGIFs user profile link; starting crawl");
        return extract_redgifs_user_profile(&client, &token, &username, config).await;
    }

    // 2. Otherwise handle as single GIF/video
    let gif_id = extract_redgifs_id(input_url)
        .context("Could not extract valid RedGIFs ID from URL")?;

    info!(gif_id = %gif_id, "Extracting single RedGIFs video");
    let (item, tags, published_at, author) =
        resolve_redgifs_item_with_token(&client, &token, &gif_id).await?;

    Ok(ExtractedMediaMetadata {
        platform: "redgifs".to_string(),
        author: author.unwrap_or_else(|| "redgifs".to_string()),
        caption: item
            .caption
            .clone()
            .unwrap_or_else(|| format!("RedGIFs video {gif_id}")),
        post_text: None,
        published_at,
        tags,
        items: vec![item],
        location: None,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// Public Helper for Reddit & Other Extractors
// -----------------------------------------------------------------------------

/// Resolves an unwatermarked media item and metadata directly from a RedGIFs ID or URL.
/// Returns: (MediaItem, Tags, PublishedDate, Author)
pub async fn resolve_redgifs_item(
    client: &Client,
    url_or_id: &str,
) -> Result<(MediaItem, Vec<String>, Option<String>, Option<String>)> {
    let gif_id = extract_redgifs_id(url_or_id).unwrap_or_else(|| url_or_id.to_string());
    let token = fetch_guest_token(client).await?;
    resolve_redgifs_item_with_token(client, &token, &gif_id).await
}

// -----------------------------------------------------------------------------
// Internal Implementation Details
// -----------------------------------------------------------------------------

pub fn create_redgifs_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Safari/605.1.15",
        ),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    headers.insert("origin", HeaderValue::from_static("https://www.redgifs.com"));
    headers.insert(
        "referer",
        HeaderValue::from_static("https://www.redgifs.com/"),
    );
    headers.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
    headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));

    Client::builder()
        .cookie_store(true)
        .default_headers(headers)
        .build()
        .map_err(Into::into)
}

pub async fn fetch_guest_token(client: &Client) -> Result<String> {
    debug!("Requesting ephemeral RedGIFs guest token");
    let resp = client
        .get("https://api.redgifs.com/v2/auth/temporary")
        .send()
        .await
        .context("Failed calling RedGIFs temporary auth endpoint")?;

    if !resp.status().is_success() {
        bail!("RedGIFs auth request failed with HTTP {}", resp.status());
    }

    let payload: Value = resp.json().await?;
    let token = payload
        .get("token")
        .and_then(|t| t.as_str())
        .context("Missing 'token' field in RedGIFs auth payload")?;

    Ok(token.to_string())
}

async fn resolve_redgifs_item_with_token(
    client: &Client,
    token: &str,
    gif_id: &str,
) -> Result<(MediaItem, Vec<String>, Option<String>, Option<String>)> {
    let api_url =
        format!("https://api.redgifs.com/v2/gifs/{gif_id}?views=yes&users=yes&niches=yes");
    debug!(gif_id, "Querying RedGIFs API v2");

    let resp = client
        .get(&api_url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .await
        .context("Failed querying RedGIFs gif metadata")?;

    if !resp.status().is_success() {
        bail!(
            "RedGIFs API returned HTTP {} for id '{gif_id}'",
            resp.status()
        );
    }

    let payload: Value = resp.json().await?;
    let gif_node = payload
        .get("gif")
        .context("Missing 'gif' object in RedGIFs response")?;

    parse_single_gif_node(gif_node)
}

fn parse_single_gif_node(
    gif: &Value,
) -> Result<(MediaItem, Vec<String>, Option<String>, Option<String>)> {
    let id = gif
        .get("id")
        .and_then(|i| i.as_str())
        .context("Missing 'id' in RedGIFs record")?;

    let urls = gif
        .get("urls")
        .context("Missing 'urls' object in GIF record")?;

    let hd_url = urls.get("hd").and_then(|u| u.as_str());
    let sd_url = urls.get("sd").and_then(|u| u.as_str());
    let poster_url = urls
        .get("poster")
        .or_else(|| urls.get("thumbnail"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());

    let primary_video_url = hd_url
        .or(sd_url)
        .context("No usable video URLs found in RedGIFs item")?
        .to_string();

    let width = gif.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as usize;
    let height = gif.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as usize;
    let dims = if width > 0 && height > 0 {
        Some(MediaDimensions { width, height })
    } else {
        None
    };

    let mut variants = Vec::new();
    if let Some(hd) = hd_url {
        variants.push(MediaVariant {
            url: hd.to_string(),
            dimensions: dims.clone(),
            file_size_bytes: None,
            label: Some("HD".to_string()),
        });
    }
    if let Some(sd) = sd_url {
        variants.push(MediaVariant {
            url: sd.to_string(),
            dimensions: None,
            file_size_bytes: None,
            label: Some("SD".to_string()),
        });
    }

    let mut tags = Vec::new();
    if let Some(tag_array) = gif.get("tags").and_then(|t| t.as_array()) {
        for t in tag_array {
            if let Some(s) = t.as_str() {
                tags.push(s.to_string());
            }
        }
    }
    if let Some(niche_array) = gif.get("niches").and_then(|n| n.as_array()) {
        for n in niche_array {
            if let Some(s) = n.as_str() {
                tags.push(s.to_string());
            }
        }
    }

    let author = gif
        .get("userName")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());

    let description = gif
        .get("description")
        .and_then(|d| d.as_str())
        .filter(|d| !d.trim().is_empty())
        .map(|d| d.to_string());

    let published_at = gif
        .get("createDate")
        .and_then(|d| d.as_i64())
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map(|dt| dt.to_rfc3339());

    let mut item = MediaItem::new(
        MediaType::Video,
        "video/mp4",
        dims,
        None,
        primary_video_url.clone(),
        poster_url,
        None,
        None,
        Some("https://www.redgifs.com/".to_string()),
        primary_video_url,
    );

    item.variants = variants;
    item.caption = description;
    item.published_at = published_at.clone();
    item.tags = tags.clone();
    item.source_post_url = Some(format!("https://www.redgifs.com/watch/{id}"));

    Ok((item, tags, published_at, author))
}

// -----------------------------------------------------------------------------
// User Profile Timeline Crawler
// -----------------------------------------------------------------------------

async fn extract_redgifs_user_profile(
    client: &Client,
    token: &str,
    username: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    let mut all_items: Vec<MediaItem> = Vec::new();
    let mut discovered_urls: Vec<String> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut aggregated_tags: HashSet<String> = HashSet::new();
    let mut latest_timestamp: Option<String> = None;
    let mut hit_known_boundary = false;

    let mut page = 1;
    const MAX_PAGES: usize = 100;

    loop {
        let api_url = format!(
            "https://api.redgifs.com/v2/users/{username}/search?order=new&count=40&type=g&page={page}"
        );
        debug!(username, page, "Fetching RedGIFs profile page");

        let resp = client
            .get(&api_url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await?;

        if !resp.status().is_success() {
            if all_items.is_empty() {
                bail!("RedGIFs user API returned HTTP {}", resp.status());
            } else {
                warn!(page, status = %resp.status(), "Pagination stopped due to HTTP status");
                break;
            }
        }

        let payload: Value = resp.json().await?;
        let gifs = match payload.get("gifs").and_then(|g| g.as_array()) {
            Some(arr) if !arr.is_empty() => arr,
            _ => break, // Reached end of user gallery
        };

        let mut new_on_page = 0;
        for gif in gifs {
            let id = gif.get("id").and_then(|i| i.as_str()).unwrap_or("");
            if id.is_empty() {
                continue;
            }

            // Early exit check if post was already downloaded
            if let Some(cfg) = config {
                if cfg.is_known_post(id) {
                    info!(
                        id,
                        username, "Encountered previously downloaded RedGIFs video. Stopping early."
                    );
                    hit_known_boundary = true;
                    break;
                }
            }

            if !seen_ids.insert(id.to_string()) {
                continue;
            }

            if let Ok((item, tags, pub_date, _)) = parse_single_gif_node(gif) {
                if latest_timestamp.is_none() && pub_date.is_some() {
                    latest_timestamp = pub_date;
                }
                for t in tags {
                    aggregated_tags.insert(t);
                }
                discovered_urls.push(format!("https://www.redgifs.com/watch/{id}"));
                all_items.push(item);
                new_on_page += 1;
            }
        }

        info!(
            username,
            page,
            items_found = new_on_page,
            total_items = all_items.len(),
            "Processed RedGIFs user page"
        );

        if hit_known_boundary || new_on_page == 0 || page >= MAX_PAGES {
            break;
        }

        page += 1;
        tokio::time::sleep(Duration::from_millis(600)).await;
    }

    if all_items.is_empty() && !hit_known_boundary {
        bail!("No videos found for RedGIFs user '{username}'");
    }

    Ok(ExtractedMediaMetadata {
        platform: "redgifs".to_string(),
        author: username.to_string(),
        caption: format!("User profile feed for {username}"),
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
// Parsers & Helpers
// -----------------------------------------------------------------------------

pub fn extract_redgifs_id(input: &str) -> Option<String> {
    let clean = input.split('?').next().unwrap_or(input).trim_end_matches('/');
    let parts: Vec<&str> = clean.split('/').collect();

    // Handles https://www.redgifs.com/watch/{id} or /ifr/{id}
    for (idx, part) in parts.iter().enumerate() {
        if (*part == "watch" || *part == "ifr") && idx + 1 < parts.len() {
            let id = parts[idx + 1].trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }

    // Fallback: If passed a clean ID string or trailing slug (and not a reserved word)
    if let Some(last) = parts.last() {
        let trimmed = last.trim();
        if !trimmed.is_empty()
            && trimmed != "watch"
            && trimmed != "ifr"
            && trimmed != "users"
            && trimmed != "search"
        {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub fn extract_redgifs_username(input: &str) -> Option<String> {
    let clean = input.split('?').next().unwrap_or(input).trim_end_matches('/');
    let parts: Vec<&str> = clean.split('/').collect();

    for (idx, part) in parts.iter().enumerate() {
        if (*part == "users" || *part == "user") && idx + 1 < parts.len() {
            let u = parts[idx + 1].trim();
            if !u.is_empty() && u != "search" {
                return Some(u.to_string());
            }
        }
    }
    None
}