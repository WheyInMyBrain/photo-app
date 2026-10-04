// src/websites/instagram.rs

use crate::config::DownloaderConfig;
use crate::models::{
    ExtractedLocation, ExtractedMediaMetadata, MediaDimensions, MediaItem, MediaType, MediaVariant,
};
use crate::utils::page::extract_hashtags_from_text;
use crate::websites::Extractor;
use anyhow::{bail, Context, Result};
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, USER_AGENT,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

const BROWSER_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Safari/605.1.15";
const APP_ID: &str = "936619743392459";
const ASBD_ID: &str = "359341";
const GQL_POST_DOC_ID: &str = "10015901848480474";

// Document IDs for Profile Queries
const PROFILE_INITIAL_DOC_ID: &str = "28991540097136703";
const PROFILE_PAGINATION_DOC_ID: &str = "29240983615539641";

pub struct InstagramExtractor;

impl Extractor for InstagramExtractor {
    fn supports(&self, url: &str) -> bool {
        let lower = url.to_lowercase();
        lower.contains("instagram.com") || lower.contains("instagr.am")
    }

    fn extract<'a>(
        &'a self,
        url: &'a str,
        config: Option<&'a DownloaderConfig>,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMediaMetadata>> + Send + 'a>> {
        Box::pin(async move { extract_instagram(url, config).await })
    }
}

pub async fn extract_instagram(
    input_url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    let has_auth = config.map_or(false, |c| c.has_instagram_auth());

    // 1. Stories and Highlights strictly require session cookies
    if input_url.contains("/stories/") {
        let cfg = config.context("DownloaderConfig required for Instagram stories")?;
        let auth_client = build_auth_client(cfg).context(
            "ig_cookie and ig_csrf_token must be set in DownloaderConfig to scrape stories and highlights.",
        )?;
        if input_url.contains("/stories/highlights/") {
            return extract_highlight_links(input_url, &auth_client).await;
        } else {
            return extract_story_links(input_url, &auth_client).await;
        }
    }

    // 2. Posts, Reels, and Carousels
    if input_url.contains("/p/") || input_url.contains("/reel/") || input_url.contains("/reels/") {
        let shortcode =
            extract_shortcode(input_url).context("Could not extract Instagram shortcode")?;

        if has_auth {
            if let Some(cfg) = config {
                if let Ok(auth_client) = build_auth_client(cfg) {
                    if let Ok(meta) = fetch_mobile_post_info(shortcode, &auth_client).await {
                        return Ok(meta);
                    }
                }
            }
        }

        let guest_client = build_guest_client()?;
        if let Ok(meta) = fetch_graphql_post_info(shortcode, &guest_client).await {
            return Ok(meta);
        }

        return fetch_mobile_post_info(shortcode, &guest_client).await;
    }

    // 3. Profile Grid Feed (Requires Authenticated GraphQL client)
    let cfg = config.context("DownloaderConfig with active credentials required to scrape profile feed")?;
    let auth_client = build_auth_client(cfg)?;
    extract_user_profile_feed(input_url, cfg, &auth_client).await
}

// -----------------------------------------------------------------------------
// Location Resolver
// -----------------------------------------------------------------------------
async fn extract_and_resolve_location(
    loc_val: Option<&Value>,
    client: &Client,
) -> Option<ExtractedLocation> {
    let loc = loc_val?;
    let name = loc.get("name").and_then(|n| n.as_str())?.trim().to_string();
    if name.is_empty() {
        return None;
    }

    let mut latitude = loc.get("lat").and_then(|l| l.as_f64());
    let mut longitude = loc.get("lng").and_then(|l| l.as_f64());

    let location_id = loc
        .get("pk")
        .or_else(|| loc.get("id"))
        .map(|v| v.to_string().trim_matches('"').to_string());

    if (latitude.is_none() || longitude.is_none()) && location_id.is_some() {
        if let Some(ref lid) = location_id {
            let loc_api = format!("https://www.instagram.com/api/v1/locations/{lid}/info/");
            if let Ok(resp) = client
                .get(&loc_api)
                .header(REFERER, "https://www.instagram.com/")
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(body) = resp.text().await {
                        if let Ok(json) = serde_json::from_str::<Value>(&body) {
                            if let Some(loc_node) = json.get("location") {
                                latitude = loc_node.get("lat").and_then(|l| l.as_f64());
                                longitude = loc_node.get("lng").and_then(|l| l.as_f64());
                            }
                        }
                    }
                }
            }
        }
    }

    Some(ExtractedLocation {
        name,
        latitude,
        longitude,
        location_id,
    })
}

// -----------------------------------------------------------------------------
// Mobile /info/ API
// -----------------------------------------------------------------------------
async fn fetch_mobile_post_info(shortcode: &str, client: &Client) -> Result<ExtractedMediaMetadata> {
    let media_id = shortcode_to_id(shortcode)?;
    let api_url = format!("https://www.instagram.com/api/v1/media/{media_id}/info/");

    let resp = client
        .get(&api_url)
        .header(REFERER, "https://www.instagram.com/")
        .send()
        .await?;

    let final_url = resp.url().as_str();
    if final_url.contains("/login/") || final_url.contains("/accounts/") {
        bail!("Instagram redirected to login wall. Session expired or missing sessionid.");
    }

    if !resp.status().is_success() {
        bail!("Mobile feed API HTTP {}", resp.status());
    }

    let body = resp.text().await?;
    let payload: Value = serde_json::from_str(&body)?;
    let item = payload
        .get("items")
        .and_then(|arr| arr.get(0))
        .context("No media item returned for this shortcode")?;

    let author = item
        .pointer("/user/username")
        .and_then(|u| u.as_str())
        .unwrap_or("unknown")
        .to_string();

    let caption = item
        .pointer("/caption/text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let tags = extract_hashtags_from_text(&caption);

    let raw_nodes: Vec<&Value> =
        if let Some(arr) = item.get("carousel_media").and_then(|c| c.as_array()) {
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

    if items.is_empty() {
        bail!("Failed to parse media items from mobile payload for shortcode: {shortcode}");
    }

    let location = extract_and_resolve_location(item.get("location"), client).await;

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author,
        caption: caption.clone(),
        post_text: Some(caption),
        published_at: item
            .get("taken_at")
            .and_then(|t| t.as_i64())
            .and_then(format_epoch_timestamp),
        tags,
        items,
        location,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// Public Web GraphQL Query (Zero-Cookie Fallback for single post)
// -----------------------------------------------------------------------------
async fn fetch_graphql_post_info(shortcode: &str, client: &Client) -> Result<ExtractedMediaMetadata> {
    let gql_url = format!(
        "https://www.instagram.com/graphql/query/?doc_id={GQL_POST_DOC_ID}&variables=%7B%22shortcode%22%3A%22{shortcode}%22%7D"
    );

    let resp = client
        .get(&gql_url)
        .header(REFERER, format!("https://www.instagram.com/p/{shortcode}/"))
        .send()
        .await?;

    if !resp.status().is_success() {
        bail!("GraphQL returned HTTP {}", resp.status());
    }

    let body = resp.text().await?;
    let payload: Value = serde_json::from_str(&body)?;
    let media = payload
        .pointer("/data/xdt_shortcode_media")
        .context("Missing xdt_shortcode_media in GraphQL response")?;

    let author = media
        .pointer("/owner/username")
        .and_then(|u| u.as_str())
        .unwrap_or("unknown")
        .to_string();

    let caption = media
        .pointer("/edge_media_to_caption/edges/0/node/text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    let tags = extract_hashtags_from_text(&caption);

    let mut items = Vec::new();
    if let Some(edges) = media
        .pointer("/edge_sidecar_to_children/edges")
        .and_then(|e| e.as_array())
    {
        for edge in edges {
            if let Some(node) = edge.get("node") {
                if let Some(item) = parse_graphql_node(node) {
                    items.push(item);
                }
            }
        }
    } else if let Some(item) = parse_graphql_node(media) {
        items.push(item);
    }

    if items.is_empty() {
        bail!("No items parsed from GraphQL for {shortcode}");
    }

    let location = extract_and_resolve_location(media.get("location"), client).await;

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author,
        caption: caption.clone(),
        post_text: Some(caption),
        published_at: media
            .get("taken_at_timestamp")
            .or_else(|| media.get("taken_at"))
            .and_then(|t| t.as_i64())
            .and_then(format_epoch_timestamp),
        tags,
        items,
        location,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

fn parse_graphql_node(node: &Value) -> Option<MediaItem> {
    let is_video = node.get("is_video").and_then(|v| v.as_bool()).unwrap_or(false);
    let display_url = node.get("display_url").and_then(|u| u.as_str()).map(clean_url)?;

    let width = node.pointer("/dimensions/width").and_then(|w| w.as_u64()).unwrap_or(0) as usize;
    let height = node.pointer("/dimensions/height").and_then(|h| h.as_u64()).unwrap_or(0) as usize;
    let dims = if width > 0 && height > 0 {
        Some(MediaDimensions { width, height })
    } else {
        None
    };

    if is_video {
        let video_url = node.get("video_url").and_then(|u| u.as_str()).map(clean_url)?;
        Some(MediaItem::new(
            MediaType::Video,
            "video/mp4",
            dims,
            None,
            video_url.clone(),
            Some(display_url),
            None,
            None,
            Some("https://www.instagram.com/".to_string()),
            video_url,
        ))
    } else {
        let mut variants = Vec::new();
        let mut seen = HashSet::new();

        if let Some(resources) = node.get("display_resources").and_then(|r| r.as_array()) {
            for res in resources {
                if let Some(src) = res.get("src").and_then(|s| s.as_str()) {
                    let cleaned = clean_url(src);
                    if seen.insert(cleaned.clone()) {
                        let w = res.get("config_width").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let h = res.get("config_height").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        let file_size_bytes = res.get("file_size_bytes").and_then(|v| v.as_u64());

                        variants.push(MediaVariant {
                            url: cleaned,
                            dimensions: if w > 0 && h > 0 {
                                Some(MediaDimensions { width: w, height: h })
                            } else {
                                None
                            },
                            file_size_bytes,
                            label: Some(format!("{w}w")),
                        });
                    }
                }
            }
        }

        variants.sort_by_key(|v| {
            v.dimensions
                .as_ref()
                .map(|d| d.width * d.height)
                .unwrap_or(0)
        });

        let high_res_url = variants
            .last()
            .map(|v| v.url.clone())
            .unwrap_or_else(|| display_url.clone());

        let best_dims = variants
            .last()
            .and_then(|v| v.dimensions.clone())
            .or(dims);

        let thumb_url = variants
            .first()
            .map(|v| v.url.clone())
            .or_else(|| Some(display_url.clone()));

        let mut media = MediaItem::new(
            MediaType::Image,
            "image/jpeg",
            best_dims,
            None,
            high_res_url.clone(),
            thumb_url,
            None,
            None,
            Some("https://www.instagram.com/".to_string()),
            high_res_url,
        );

        if !variants.is_empty() {
            media.variants = variants;
        }

        Some(media)
    }
}

// -----------------------------------------------------------------------------
// Highlights Extractor
// -----------------------------------------------------------------------------
async fn extract_highlight_links(input_url: &str, client: &Client) -> Result<ExtractedMediaMetadata> {
    let clean = input_url.split('?').next().unwrap_or(input_url).trim_matches('/');
    let highlight_id = clean
        .rsplit('/')
        .next()
        .context("Missing highlight ID in URL")?;

    let reel_id = format!("highlight:{highlight_id}");
    let api_url = format!("https://www.instagram.com/api/v1/feed/reels_media/?reel_ids={reel_id}");

    let resp = client
        .get(&api_url)
        .header(REFERER, "https://www.instagram.com/")
        .send()
        .await?;

    if !resp.status().is_success() {
        bail!("Highlight API returned HTTP {}", resp.status());
    }

    let body = resp.text().await?;
    let payload: Value = serde_json::from_str(&body)?;
    let empty_vec = Vec::new();
    let raw_items = payload
        .get("reels")
        .and_then(|r| r.get(&reel_id))
        .and_then(|t| t.get("items"))
        .and_then(|i| i.as_array())
        .unwrap_or(&empty_vec);

    if raw_items.is_empty() {
        bail!("Highlight {highlight_id} contains 0 media items.");
    }

    let user_name = payload
        .pointer(&format!("/reels/{reel_id}/user/username"))
        .and_then(|u| u.as_str())
        .unwrap_or("instagram_user")
        .to_string();

    let mut items = Vec::new();
    for raw in raw_items {
        if let Some(media_item) = parse_media_item(raw) {
            items.push(media_item);
        }
    }

    let published_at = raw_items
        .first()
        .and_then(|item| item.get("taken_at"))
        .and_then(|t| t.as_i64())
        .and_then(format_epoch_timestamp);

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: user_name,
        caption: format!("Highlight {highlight_id}"),
        post_text: None,
        published_at,
        tags: Vec::new(),
        items,
        location: None,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// User Profile Feed (Full GraphQL Pagination Loop)
// -----------------------------------------------------------------------------
async fn extract_user_profile_feed(
    input_url: &str,
    cfg: &DownloaderConfig,
    client: &Client,
) -> Result<ExtractedMediaMetadata> {
    let parsed_url = reqwest::Url::parse(input_url)?;
    let clean_path = parsed_url.path().trim_matches('/');
    let username = clean_path
        .split('/')
        .next()
        .filter(|s| !s.is_empty() && *s != "explore" && *s != "direct")
        .context("Could not extract username from Instagram profile URL")?;

    let cookie = cfg.ig_cookie.as_deref().unwrap_or("");
    let csrf_token = cfg
        .ig_csrf_token
        .clone()
        .or_else(|| extract_cookie_val(cookie, "csrftoken"))
        .unwrap_or_default();

    let lsd = cfg
        .ig_lsd
        .clone()
        .unwrap_or_else(|| csrf_token.clone());

    let dtsg = cfg
        .ig_fb_dtsg
        .clone()
        .unwrap_or_default();

    let jazoest = compute_jazoest(&dtsg);

    let ds_user_id = extract_cookie_val(cookie, "ds_user_id")
        .or_else(|| extract_cookie_val(cookie, "sessionid").and_then(|s| s.split(':').next().map(|v| v.to_string())))
        .unwrap_or_else(|| "0".to_string());

    let mut items = Vec::new();
    let mut discovered_post_urls = Vec::new();
    let mut cursor: Option<String> = None;
    let mut page_count = 0;
    const MAX_PAGES: usize = 100;

    loop {
        page_count += 1;

        let (doc_id, friendly_name, variables) = if let Some(ref c) = cursor {
            (
                PROFILE_PAGINATION_DOC_ID,
                "PolarisProfilePostsTabContentQuery_connection",
                json!({
                    "after": c,
                    "before": null,
                    "first": 12,
                    "last": null,
                    "username": username,
                    "data": {
                        "count": 12,
                        "include_reel_media_seen_timestamp": true,
                        "include_relationship_info": true,
                        "latest_besties_reel_media": true,
                        "latest_reel_media": true
                    },
                    "include_multi_captions": false,
                    "__relay_internal__pv__PolarisMultiCaptionCarouselEnabledrelayprovider": false,
                    "__relay_internal__pv__PolarisShortDramaEnabledrelayprovider": false,
                    "__relay_internal__pv__PolarisReelsRecoDebugOverlayEnabledrelayprovider": false
                }),
            )
        } else {
            (
                PROFILE_INITIAL_DOC_ID,
                "PolarisProfilePostsQuery",
                json!({
                    "username": username,
                    "data": {
                        "count": 12,
                        "include_reel_media_seen_timestamp": true,
                        "include_relationship_info": true,
                        "latest_besties_reel_media": true,
                        "latest_reel_media": true
                    },
                    "__relay_internal__pv__PolarisMultiCaptionCarouselEnabledrelayprovider": false,
                    "__relay_internal__pv__PolarisShortDramaEnabledrelayprovider": false,
                    "__relay_internal__pv__PolarisReelsRecoDebugOverlayEnabledrelayprovider": false
                }),
            )
        };

        let raw_vars = variables.to_string();
        let encoded_vars = urlencoding::encode(&raw_vars);
        let encoded_dtsg = urlencoding::encode(&dtsg);

        let form_body = format!(
            "av={ds_user_id}&__d=www&__user=0&__a=1&__req=6&dpr=2&__comet_req=7&fb_dtsg={encoded_dtsg}&jazoest={jazoest}&lsd={lsd}&fb_api_caller_class=RelayModern&fb_api_req_friendly_name={friendly_name}&server_timestamps=true&doc_id={doc_id}&variables={encoded_vars}"
        );

        let resp = client
            .post("https://www.instagram.com/graphql/query")
            .header(REFERER, format!("https://www.instagram.com/{username}/"))
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("X-FB-Friendly-Name", friendly_name)
            .header("X-Root-Field-Name", "xdt_api__v1__feed__user_timeline_graphql_connection")
            .body(form_body)
            .send()
            .await?;

        if !resp.status().is_success() {
            if items.is_empty() {
                bail!("Instagram GraphQL query failed with HTTP {}", resp.status());
            } else {
                break;
            }
        }

        let mut body: String = resp.text().await?;
        if let Some(stripped) = body.strip_prefix("for (;;);") {
            body = stripped.to_string();
        }

        let payload: Value = match serde_json::from_str(&body) {
            Ok(val) => val,
            Err(e) => {
                if items.is_empty() {
                    bail!("Failed to parse Instagram GraphQL JSON: {e}. Raw: {}", &body[..body.len().min(200)]);
                } else {
                    break;
                }
            }
        };

        if let Some(err_code) = payload.get("error") {
            let msg = payload.get("errorSummary").and_then(|s| s.as_str()).unwrap_or("unknown error");
            if items.is_empty() {
                bail!("Meta GraphQL error {err_code}: {msg}. Verify session cookies and credentials in .env.");
            } else {
                break;
            }
        }

        let (edges, page_info) = if let Some(conn) = payload.pointer("/data/xdt_api__v1__feed__user_timeline_graphql_connection") {
            (conn.get("edges").and_then(|e| e.as_array()), conn.get("page_info"))
        } else if let Some(conn) = payload.pointer("/data/user/edge_owner_to_timeline_media") {
            (conn.get("edges").and_then(|e| e.as_array()), conn.get("page_info"))
        } else {
            if items.is_empty() {
                let debug_preview = &body[..body.len().min(300)];
                bail!("Failed to locate timeline connection in GraphQL response. Preview: {debug_preview}");
            } else {
                break;
            }
        };

        let raw_edges = match edges {
            Some(arr) if !arr.is_empty() => arr,
            _ => break,
        };

        for edge in raw_edges {
            let node = edge.get("node").unwrap_or(edge);
            if let Some(code) = node.get("code").or_else(|| node.get("shortcode")).and_then(|c| c.as_str()) {
                discovered_post_urls.push(format!("https://www.instagram.com/p/{code}/"));
            }

            if let Some(media_item) = parse_media_item(node).or_else(|| parse_graphql_node(node)) {
                items.push(media_item);
            } else if let Some(carousels) = node.get("carousel_media").and_then(|c| c.as_array()) {
                for c in carousels {
                    if let Some(item) = parse_media_item(c) {
                        items.push(item);
                    }
                }
            } else if let Some(carousels) = node.pointer("/edge_sidecar_to_children/edges").and_then(|c| c.as_array()) {
                for c in carousels {
                    let child_node = c.get("node").unwrap_or(c);
                    if let Some(item) = parse_graphql_node(child_node) {
                        items.push(item);
                    }
                }
            }
        }

        let has_next_page = page_info
            .and_then(|p| p.get("has_next_page"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);

        let next_cursor = page_info
            .and_then(|p| p.get("end_cursor"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        if !has_next_page || next_cursor.is_none() || page_count >= MAX_PAGES {
            break;
        }

        cursor = next_cursor;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    if items.is_empty() {
        bail!("No items returned for @{username}");
    }

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.to_string(),
        caption: format!("Profile grid feed for @{username}"),
        post_text: None,
        published_at: None,
        tags: Vec::new(),
        items,
        location: None,
        next_page_url: None,
        discovered_post_urls,
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// Stories Extractor
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
        .header(REFERER, format!("https://www.instagram.com/stories/{username}/"))
        .send()
        .await?;

    if !resp.status().is_success() {
        bail!("Stories API returned HTTP {}", resp.status());
    }

    let body = resp.text().await?;
    let payload: Value = serde_json::from_str(&body)?;
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
    for node in raw_items {
        if let Some(media_item) = parse_media_item(node) {
            items.push(media_item);
        }
    }

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.to_string(),
        caption: format!("Active stories from @{username}"),
        post_text: None,
        published_at: None,
        tags: Vec::new(),
        items,
        location: None,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

async fn extract_direct_pk_links(
    media_pk: &str,
    username: &str,
    client: &Client,
) -> Result<ExtractedMediaMetadata> {
    let info_url = format!("https://www.instagram.com/api/v1/media/{media_pk}/info/");
    let resp = client
        .get(&info_url)
        .header(REFERER, "https://www.instagram.com/")
        .send()
        .await?;

    let body = resp.text().await?;
    let payload: Value = serde_json::from_str(&body)?;
    let item = payload
        .get("items")
        .and_then(|arr| arr.get(0))
        .context("Story item not found")?;

    let mut items = Vec::new();
    if let Some(media_item) = parse_media_item(item) {
        items.push(media_item);
    }

    let published_at = item
        .get("taken_at")
        .and_then(|t| t.as_i64())
        .and_then(format_epoch_timestamp);

    let location = extract_and_resolve_location(item.get("location"), client).await;

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.to_string(),
        caption: format!("Story item {media_pk} from @{username}"),
        post_text: None,
        published_at,
        tags: Vec::new(),
        items,
        location: location,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// Media Parser: Guarantees Deduplication & Highest Quality Sorting
// -----------------------------------------------------------------------------
fn parse_media_item(item: &Value) -> Option<MediaItem> {
    let media_type = item.get("media_type").and_then(|t| t.as_i64()).unwrap_or(1);
    let candidates = item.get("image_versions2")?.get("candidates")?.as_array()?;
    let thumbnail_url = candidates.last()?.get("url")?.as_str().map(clean_url);

    if media_type == 2 {
        let versions = item.get("video_versions")?.as_array()?;
        if versions.is_empty() {
            return None;
        }

        let mut parsed_variants: Vec<(usize, usize, String, Option<u64>)> = Vec::new();
        let mut seen_urls = HashSet::new();

        for v in versions {
            if let Some(v_url) = v.get("url").and_then(|u| u.as_str()) {
                let cleaned = clean_url(v_url);
                if seen_urls.insert(cleaned.clone()) {
                    let w = v.get("width").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
                    let h = v.get("height").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
                    let size = v
                        .get("file_size")
                        .or_else(|| v.get("file_size_bytes"))
                        .and_then(|s| s.as_u64());

                    parsed_variants.push((w, h, cleaned, size));
                }
            }
        }

        parsed_variants.sort_by_key(|(w, h, _, _)| std::cmp::Reverse(w * h));
        let (best_w, best_h, best_url, best_size) = parsed_variants.first()?.clone();

        let dims = if best_w > 0 && best_h > 0 {
            Some(MediaDimensions { width: best_w, height: best_h })
        } else {
            None
        };

        let mut media = MediaItem::new(
            MediaType::Video,
            "video/mp4",
            dims,
            best_size,
            best_url.clone(),
            thumbnail_url,
            None,
            None,
            Some("https://www.instagram.com/".to_string()),
            best_url,
        );

        media.variants = parsed_variants
            .into_iter()
            .map(|(w, h, url, size)| MediaVariant {
                url,
                dimensions: if w > 0 && h > 0 {
                    Some(MediaDimensions { width: w, height: h })
                } else {
                    None
                },
                file_size_bytes: size,
                label: Some(format!("{w}x{h}")),
            })
            .collect();

        Some(media)
    } else {
        let mut parsed_variants: Vec<(usize, usize, String, Option<u64>)> = Vec::new();
        let mut seen_urls = HashSet::new();

        for c in candidates {
            if let Some(c_url) = c.get("url").and_then(|u| u.as_str()) {
                let cleaned = clean_url(c_url);
                if seen_urls.insert(cleaned.clone()) {
                    let w = c.get("width").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
                    let h = c.get("height").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
                    let size = c
                        .get("file_size")
                        .or_else(|| c.get("file_size_bytes"))
                        .and_then(|s| s.as_u64());

                    parsed_variants.push((w, h, cleaned, size));
                }
            }
        }

        parsed_variants.sort_by_key(|(w, h, _, _)| std::cmp::Reverse(w * h));
        let (best_w, best_h, best_url, best_size) = parsed_variants.first()?.clone();

        let dims = if best_w > 0 && best_h > 0 {
            Some(MediaDimensions { width: best_w, height: best_h })
        } else {
            None
        };

        let thumb = parsed_variants
            .last()
            .map(|(_, _, u, _)| u.clone())
            .or(thumbnail_url);

        let mut media = MediaItem::new(
            MediaType::Image,
            "image/jpeg",
            dims,
            best_size,
            best_url.clone(),
            thumb,
            None,
            None,
            Some("https://www.instagram.com/".to_string()),
            best_url,
        );

        media.variants = parsed_variants
            .into_iter()
            .map(|(w, h, url, size)| MediaVariant {
                url,
                dimensions: if w > 0 && h > 0 {
                    Some(MediaDimensions { width: w, height: h })
                } else {
                    None
                },
                file_size_bytes: size,
                label: Some(format!("{w}w")),
            })
            .collect();

        Some(media)
    }
}

// -----------------------------------------------------------------------------
// Numeric ID Resolver
// -----------------------------------------------------------------------------
async fn resolve_numeric_user_id(username: &str, client: &Client) -> Result<String> {
    let search_url = format!("https://www.instagram.com/api/v1/web/search/topsearch/?query={username}");
    if let Ok(resp) = client
        .get(&search_url)
        .header(REFERER, "https://www.instagram.com/")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(body) = resp.text().await {
                if let Ok(val) = serde_json::from_str::<Value>(&body) {
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
    }

    bail!("Could not resolve numeric target ID for @{username}");
}

// -----------------------------------------------------------------------------
// Client Constructors
// -----------------------------------------------------------------------------
fn build_guest_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));
    headers.insert("X-IG-App-ID", HeaderValue::from_static(APP_ID));
    headers.insert("X-Requested-With", HeaderValue::from_static("XMLHttpRequest"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    );

    Ok(Client::builder().cookie_store(true).default_headers(headers).build()?)
}

fn build_auth_client(cfg: &DownloaderConfig) -> Result<Client> {
    let cookie = cfg
        .ig_cookie
        .as_deref()
        .context("Missing ig_cookie in DownloaderConfig")?;
    let csrf = cfg
        .ig_csrf_token
        .as_deref()
        .context("Missing ig_csrf_token in DownloaderConfig")?;

    let lsd = cfg
        .ig_lsd
        .as_deref()
        .unwrap_or(csrf);

    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));
    headers.insert("X-IG-App-ID", HeaderValue::from_static(APP_ID));
    headers.insert("X-ASBD-ID", HeaderValue::from_static(ASBD_ID));
    headers.insert("X-CSRFToken", HeaderValue::from_str(csrf)?);
    headers.insert(COOKIE, HeaderValue::from_str(cookie)?);
    headers.insert("X-Requested-With", HeaderValue::from_static("XMLHttpRequest"));
    headers.insert(ORIGIN, HeaderValue::from_static("https://www.instagram.com"));
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));
    headers.insert("X-FB-LSD", HeaderValue::from_str(lsd)?);

    Ok(Client::builder().cookie_store(true).default_headers(headers).build()?)
}

fn compute_jazoest(dtsg: &str) -> String {
    let mut total: u32 = 0;
    for c in dtsg.chars() {
        total = total.wrapping_add(c as u32);
    }
    format!("2{total}")
}

fn extract_cookie_val(cookie: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    for part in cookie.split(';') {
        let trimmed = part.trim();
        if let Some(val) = trimmed.strip_prefix(&prefix) {
            return Some(val.to_string());
        }
    }
    None
}

fn clean_url(raw: &str) -> String {
    raw.replace(r"\/", "/").replace(r"\u0026", "&")
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

fn shortcode_to_id(shortcode: &str) -> Result<u128> {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut id: u128 = 0;
    for c in shortcode.chars() {
        let val = ALPHABET.find(c).context("Invalid character in shortcode")? as u128;
        id = id.checked_mul(64).context("Overflow shortcode")? + val;
    }
    Ok(id)
}

fn format_epoch_timestamp(epoch_secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(epoch_secs, 0).map(|dt| dt.to_rfc3339())
}