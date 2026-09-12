use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, COOKIE, USER_AGENT};
use reqwest::Client;
use serde_json::Value;

use crate::models::{ExtractedMediaMetadata, MediaItem, MediaType};

const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:155.0) Gecko/20100101 Firefox/155.0";
const APP_ID: &str = "936619743392459";

// -----------------------------------------------------------------------------
// Link Resolution Entrypoint
// -----------------------------------------------------------------------------
pub async fn extract_links(input_url: &str) -> Result<ExtractedMediaMetadata> {
    let cookie = std::env::var("IG_COOKIE").context("Missing IG_COOKIE in .env")?;
    let csrf = std::env::var("IG_CSRF_TOKEN").context("Missing IG_CSRF_TOKEN in .env")?;

    let client = build_client(&cookie, &csrf)?;

    if input_url.contains("/stories/") {
        extract_story_links(input_url, &client).await
    } else {
        extract_feed_links(input_url, &client).await
    }
}

// -----------------------------------------------------------------------------
// Stories Link Extraction
// -----------------------------------------------------------------------------
async fn extract_story_links(input_url: &str, client: &Client) -> Result<ExtractedMediaMetadata> {
    let clean = input_url.split('?').next().unwrap_or(input_url).trim_matches('/');
    let segments: Vec<&str> = clean.split('/').collect();

    let story_idx = segments
        .iter()
        .position(|&s| s == "stories")
        .context("Invalid story URL: missing '/stories/' path")?;

    let username = segments
        .get(story_idx + 1)
        .copied()
        .context("Missing username in story URL")?;

    if let Some(media_pk) = segments.get(story_idx + 2).copied() {
        return extract_direct_pk_links(media_pk, username, client).await;
    }

    let user_id = resolve_numeric_user_id(username, client).await?;
    let tray_url = format!("https://www.instagram.com/api/v1/feed/reels_media/?reel_ids={user_id}");

    let resp = client
        .get(&tray_url)
        .header("Referer", format!("https://www.instagram.com/stories/{username}/"))
        .send()
        .await?;

    if !resp.status().is_success() {
        bail!("Stories API returned HTTP {}", resp.status());
    }

    let payload: Value = resp.json().await?;
    let empty_vec = Vec::new();
    let raw_items = payload
        .get("reels")
        .and_then(|r| r.get(&user_id))
        .and_then(|t| t.get("items"))
        .and_then(|i| i.as_array())
        .unwrap_or(&empty_vec);

    if raw_items.is_empty() {
        bail!("@{username} has 0 active stories in tray.");
    }

    let mut items = Vec::new();
    for raw in raw_items {
        if let Some(item) = parse_media_item(raw) {
            items.push(item);
        }
    }

    let now = chrono::Local::now().format("%Y-%m-%d %H-%M-%S");

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.to_string(),
        caption: format!("Stories from @{username} at {now}"),
        items,
    })
}

// -----------------------------------------------------------------------------
// Feed & Reels Link Extraction
// -----------------------------------------------------------------------------
async fn extract_feed_links(input_url: &str, client: &Client) -> Result<ExtractedMediaMetadata> {
    let shortcode = extract_shortcode(input_url).context("Could not extract Instagram shortcode")?;
    let media_id = shortcode_to_id(shortcode)?;
    let api_url = format!("https://www.instagram.com/api/v1/media/{media_id}/info/");

    let resp = client.get(&api_url).send().await?;
    if !resp.status().is_success() {
        bail!("Feed API returned HTTP {}", resp.status());
    }

    let payload: Value = resp.json().await?;
    let item = payload
        .get("items")
        .and_then(|arr| arr.get(0))
        .context("No media item returned")?;

    let author = item
        .get("user")
        .and_then(|u| u.get("username"))
        .and_then(|u| u.as_str())
        .unwrap_or("unknown")
        .to_string();

    let caption = item
        .get("caption")
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let raw_nodes: Vec<&Value> = if let Some(arr) = item.get("carousel_media").and_then(|c| c.as_array()) {
        arr.iter().collect()
    } else {
        vec![item]
    };

    let mut items = Vec::new();
    for node in raw_nodes {
        if let Some(media_item) = parse_media_item(node) {
            items.push(media_item);
        }
    }

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author,
        caption,
        items,
    })
}

// -----------------------------------------------------------------------------
// Direct Single Story Item Lookup
// -----------------------------------------------------------------------------
async fn extract_direct_pk_links(media_pk: &str, username: &str, client: &Client) -> Result<ExtractedMediaMetadata> {
    let info_url = format!("https://www.instagram.com/api/v1/media/{media_pk}/info/");
    let resp = client.get(&info_url).send().await?;
    let payload: Value = resp.json().await?;

    let item = payload
        .get("items")
        .and_then(|arr| arr.get(0))
        .context("Media not found")?;

    let mut items = Vec::new();
    if let Some(media_item) = parse_media_item(item) {
        items.push(media_item);
    }

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.to_string(),
        caption: format!("Story item {media_pk} from @{username}"),
        items,
    })
}

// -----------------------------------------------------------------------------
// Parser
// -----------------------------------------------------------------------------
fn parse_media_item(item: &Value) -> Option<MediaItem> {
    let media_type = item.get("media_type").and_then(|t| t.as_i64()).unwrap_or(1);

    let candidates = item.get("image_versions2")?.get("candidates")?.as_array()?;
    let thumbnail_url = clean_url(candidates.last()?.get("url")?.as_str()?);

    if media_type == 2 {
        let versions = item.get("video_versions")?.as_array()?;
        if versions.is_empty() {
            return None;
        }

        let high_res = versions
            .iter()
            .find(|v| {
                v.get("url")
                    .and_then(|u| u.as_str())
                    .map(|u| u.contains("vs=") || u.contains("_nc_vs="))
                    .unwrap_or(false)
            })
            .or_else(|| versions.first())?
            .get("url")?
            .as_str()?;

        Some(MediaItem::new(
            MediaType::Video,
            clean_url(high_res),
            None, // Instagram embeds audio in the multiplexed video stream
            thumbnail_url,
        ))
    } else {
        let high_res = candidates.first()?.get("url")?.as_str()?;

        Some(MediaItem::new(
            MediaType::Image,
            clean_url(high_res),
            None,
            thumbnail_url,
        ))
    }
}

// -----------------------------------------------------------------------------
// Numeric ID Resolver
// -----------------------------------------------------------------------------
async fn resolve_numeric_user_id(username: &str, client: &Client) -> Result<String> {
    // Strategy 1: TopSearch API
    let search_url = format!("https://www.instagram.com/api/v1/web/search/topsearch/?query={username}");
    if let Ok(resp) = client
        .get(&search_url)
        .header("Referer", "https://www.instagram.com/")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(val) = resp.json::<Value>().await {
                if let Some(users) = val.get("users").and_then(|u| u.as_array()) {
                    for entry in users {
                        let matched_name = entry
                            .pointer("/user/username")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if matched_name.eq_ignore_ascii_case(username) {
                            if let Some(pk) = entry.pointer("/user/pk").and_then(|v| {
                                v.as_str()
                                    .map(|s| s.to_string())
                                    .or_else(|| v.as_i64().map(|n| n.to_string()))
                            }) {
                                return Ok(pk);
                            }
                        }
                    }
                }
            }
        }
    }

    // Strategy 2: Direct HTML Scrape
    let profile_url = format!("https://www.instagram.com/{username}/");
    if let Ok(resp) = client
        .get(&profile_url)
        .header("Referer", "https://www.instagram.com/")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(html) = resp.text().await {
                if let Some(pos) = html.find("\"props\":{\"id\":\"") {
                    let rem = &html[pos + 15..];
                    if let Some(end) = rem.find('\"') {
                        let id = &rem[..end];
                        if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() {
                            return Ok(id.to_string());
                        }
                    }
                }

                if let Some(pos) = html.find("\"profilePage_") {
                    let rem = &html[pos + 13..];
                    if let Some(end) = rem.find('\"') {
                        let id = &rem[..end];
                        if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() {
                            return Ok(id.to_string());
                        }
                    }
                }
            }
        }
    }

    bail!("Could not resolve numeric target ID for @{username}");
}

// -----------------------------------------------------------------------------
// Utilities
// -----------------------------------------------------------------------------
fn clean_url(raw: &str) -> String {
    raw.replace(r"\/", "/").replace(r"\u0026", "&")
}

fn build_client(cookie: &str, csrf: &str) -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));
    headers.insert("X-IG-App-ID", HeaderValue::from_static(APP_ID));
    headers.insert("X-CSRFToken", HeaderValue::from_str(csrf)?);
    headers.insert(COOKIE, HeaderValue::from_str(cookie)?);
    headers.insert("X-Requested-With", HeaderValue::from_static("XMLHttpRequest"));

    Ok(Client::builder().cookie_store(true).default_headers(headers).build()?)
}

fn extract_shortcode(url: &str) -> Option<&str> {
    let clean = url.split('?').next()?;
    let segments: Vec<&str> = clean.trim_matches('/').split('/').collect();
    for window in segments.windows(2) {
        if window[0] == "p" || window[0] == "reel" || window[0] == "reels" {
            return Some(window[1]);
        }
    }
    None
}

fn shortcode_to_id(shortcode: &str) -> Result<u64> {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut id: u64 = 0;
    for c in shortcode.chars() {
        let val = ALPHABET.find(c).context("Invalid character in shortcode")? as u64;
        id = id.checked_mul(64).context("Overflow shortcode")? + val;
    }
    Ok(id)
}