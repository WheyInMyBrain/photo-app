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
use tracing::{debug, error, info, warn};

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

// -----------------------------------------------------------------------------
// Main Instagram Router Entry Point
// -----------------------------------------------------------------------------
pub async fn extract_instagram(
    input_url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    info!(url = %input_url, "Starting Instagram extraction");

    // 1. Stories and Highlights
    if input_url.contains("/stories/") {
        info!("URL matched as Instagram Story or Highlight");
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

    // 2. Posts, Reels, and Carousels -> Handled strictly by extract_single_post
    if input_url.contains("/p/") || input_url.contains("/reel/") || input_url.contains("/reels/") {
        info!("URL matched as single post/reel");
        return extract_single_post(input_url, config).await;
    }

    // 3. Profile Grid Feed -> Discovers links and passes each through extract_single_post
    info!("URL matched as Instagram profile feed");
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
            debug!(location_name = %name, location_id = %lid, "Fetching missing coordinates from location API");
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

    debug!(name = %name, ?latitude, ?longitude, "Resolved location");
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

    debug!(shortcode, media_id = %media_id, "Calling mobile post info API");
    let resp = client
        .get(&api_url)
        .header(REFERER, "https://www.instagram.com/")
        .send()
        .await?;

    let final_url = resp.url().as_str();
    if final_url.contains("/login/") || final_url.contains("/accounts/") {
        error!(shortcode, "Mobile endpoint redirected to login wall. Session expired or missing.");
        bail!("Instagram redirected to login wall. Session expired or missing sessionid.");
    }

    if !resp.status().is_success() {
        warn!(shortcode, status = %resp.status(), "Mobile feed API returned non-success HTTP status");
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
        error!(shortcode, "Failed to parse any media item from mobile API response");
        bail!("Failed to parse media items from mobile payload for shortcode: {shortcode}");
    }

    let location = extract_and_resolve_location(item.get("location"), client).await;

    debug!(shortcode, author = %author, items_count = items.len(), "Successfully fetched post via mobile API");
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
// Public Web GraphQL Query (Extracts Full Resolution Variants, Timestamps & Loc)
// -----------------------------------------------------------------------------
async fn fetch_graphql_post_info(shortcode: &str, client: &Client) -> Result<ExtractedMediaMetadata> {
    let gql_url = format!(
        "https://www.instagram.com/graphql/query/?doc_id={GQL_POST_DOC_ID}&variables=%7B%22shortcode%22%3A%22{shortcode}%22%7D"
    );

    debug!(shortcode, "Executing GraphQL post query");
    let resp = client
        .get(&gql_url)
        .header(REFERER, format!("https://www.instagram.com/p/{shortcode}/"))
        .send()
        .await?;

    if !resp.status().is_success() {
        warn!(shortcode, status = %resp.status(), "GraphQL single-post query returned non-success HTTP status");
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
        error!(shortcode, "Zero media items parsed from GraphQL response");
        bail!("No items parsed from GraphQL for {shortcode}");
    }

    let location = extract_and_resolve_location(media.get("location"), client).await;

    let published_at = media
        .get("taken_at_timestamp")
        .or_else(|| media.get("taken_at"))
        .and_then(|t| t.as_i64())
        .and_then(format_epoch_timestamp);

    let post_url = format!("https://www.instagram.com/p/{shortcode}/");

    // Stamp this post's exact metadata onto every MediaItem parsed
    for item in &mut items {
        item.source_post_url = Some(post_url.clone());
        item.caption = if !caption.is_empty() {
            Some(caption.clone())
        } else {
            None
        };
        item.published_at = published_at.clone();
        item.location = location.clone();
        item.tags = tags.clone();
    }

    debug!(shortcode, author = %author, items_count = items.len(), "Successfully fetched post via GraphQL");
    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author,
        caption: caption.clone(),
        post_text: Some(caption),
        published_at,
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
// Single Post Resolution (The Single Function That Handles All Posts)
// -----------------------------------------------------------------------------
pub async fn extract_single_post(
    input_url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    let shortcode =
        extract_shortcode(input_url).context("Could not extract Instagram shortcode from URL")?;
    let has_auth = config.map_or(false, |c| c.has_instagram_auth());

    // 1. Authenticated Mobile API (Best for raw master files & stories)
    if has_auth {
        if let Some(cfg) = config {
            if let Ok(auth_client) = build_auth_client(cfg) {
                debug!(shortcode, "Attempting authenticated mobile post fetch");
                match fetch_mobile_post_info(shortcode, &auth_client).await {
                    Ok(meta) => return Ok(meta),
                    Err(e) => warn!(
                        error = %e,
                        shortcode,
                        "Authenticated mobile fetch failed, falling back to GraphQL"
                    ),
                }
            }
        }
    }

    // 2. Public Web GraphQL Query (Best for exact ISO timestamp, tags, locations & sidecars)
    let guest_client = build_guest_client()?;
    debug!(shortcode, "Attempting GraphQL post fetch");
    match fetch_graphql_post_info(shortcode, &guest_client).await {
        Ok(meta) => return Ok(meta),
        Err(e) => warn!(
            error = %e,
            shortcode,
            "GraphQL fetch failed, falling back to guest mobile API"
        ),
    }

    // 3. Guest Mobile API Fallback
    fetch_mobile_post_info(shortcode, &guest_client).await
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

    info!(highlight_id, "Extracting highlight");
    let reel_id = format!("highlight:{highlight_id}");
    let api_url = format!("https://www.instagram.com/api/v1/feed/reels_media/?reel_ids={reel_id}");

    let resp = client
        .get(&api_url)
        .header(REFERER, "https://www.instagram.com/")
        .send()
        .await?;

    if !resp.status().is_success() {
        error!(highlight_id, status = %resp.status(), "Highlight API request failed");
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
        warn!(highlight_id, "Highlight returned 0 items");
        bail!("Highlight {highlight_id} contains 0 media items.");
    }

    let user_name = payload
        .pointer(&format!("/reels/{reel_id}/user/username"))
        .and_then(|u| u.as_str())
        .unwrap_or("instagram_user")
        .to_string();

    let mut items = Vec::new();
    for raw in raw_items {
        if let Some(mut media_item) = parse_media_item(raw) {
            // Per-slide timestamp
            let slide_date = raw
                .get("taken_at")
                .and_then(|t| t.as_i64())
                .and_then(format_epoch_timestamp);

            // Per-slide caption
            let slide_caption = raw
                .pointer("/caption/text")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());

            let slide_tags = slide_caption
                .as_deref()
                .map(extract_hashtags_from_text)
                .unwrap_or_default();

            // Per-slide location
            let slide_location = extract_and_resolve_location(raw.get("location"), client).await;

            // Direct PK link for source URL
            let pk = raw.get("pk").or_else(|| raw.get("id")).and_then(|v| {
                v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string()))
            });
            let source_post_url = pk.map(|id| format!("https://www.instagram.com/stories/{user_name}/{id}/"));

            media_item.published_at = slide_date;
            media_item.caption = slide_caption.or_else(|| Some(format!("Highlight {highlight_id}")));
            media_item.tags = slide_tags;
            media_item.location = slide_location;
            media_item.source_post_url = source_post_url;

            items.push(media_item);
        }
    }

    let fallback_date = items.first().and_then(|i| i.published_at.clone());

    info!(highlight_id, author = %user_name, count = items.len(), "Successfully extracted highlight items");
    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: user_name,
        caption: format!("Highlight {highlight_id}"),
        post_text: None,
        published_at: fallback_date,
        tags: Vec::new(),
        items,
        location: None,
        next_page_url: None,
        discovered_post_urls: Vec::new(),
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// User Profile Feed: Crawls timeline & resolves each post via post extractor
// -----------------------------------------------------------------------------
async fn extract_user_profile_feed(
    input_url: &str,
    cfg: &DownloaderConfig,
    client: &Client,
) -> Result<ExtractedMediaMetadata> {
    let clean_path = reqwest::Url::parse(input_url)?
        .path()
        .trim_matches('/')
        .to_string();
    let username = clean_path
        .split('/')
        .next()
        .filter(|s| !s.is_empty() && *s != "explore" && *s != "direct")
        .context("Could not extract username from Instagram profile URL")?
        .to_string();

    // Step 1: Discover all profile post URLs
    let post_urls = discover_profile_post_urls(input_url, cfg, client).await?;
    let total_count = post_urls.len();
    info!(
        username = %username,
        total_posts = total_count,
        "Executing post extraction for each discovered link"
    );

    // Step 2: Loop every link through the single post extractor
    let mut all_media_items = Vec::new();
    let mut aggregated_tags = HashSet::new();
    let mut latest_post_date = None;
    let mut primary_location = None;

    for (idx, post_url) in post_urls.iter().enumerate() {
        debug!(progress = format!("{}/{}", idx + 1, total_count), url = %post_url, "Extracting post metadata & media");

        match extract_single_post(post_url, Some(cfg)).await {
            Ok(post_meta) => {
                let count = post_meta.items.len();

                // Capture profile-level fallbacks from the newest post if needed
                if latest_post_date.is_none() && post_meta.published_at.is_some() {
                    latest_post_date = post_meta.published_at.clone();
                }
                if primary_location.is_none() && post_meta.location.is_some() {
                    primary_location = post_meta.location.clone();
                }

                // Explicitly bind ONLY this specific post's metadata to its items
                for mut item in post_meta.items {
                    item.source_post_url = Some(post_url.clone());
                    item.caption = if !post_meta.caption.is_empty() {
                        Some(post_meta.caption.clone())
                    } else {
                        None
                    };
                    item.published_at = post_meta.published_at.clone();
                    item.location = post_meta.location.clone();
                    item.tags = post_meta.tags.clone();

                    for tag in &post_meta.tags {
                        aggregated_tags.insert(tag.clone());
                    }

                    all_media_items.push(item);
                }

                info!(
                    progress = format!("{}/{}", idx + 1, total_count),
                    url = %post_url,
                    media_count = count,
                    date = ?post_meta.published_at,
                    has_location = post_meta.location.is_some(),
                    "Post resolved successfully with individual metadata"
                );
            }
            Err(e) => {
                warn!(
                    progress = format!("{}/{}", idx + 1, total_count),
                    url = %post_url,
                    error = %e,
                    "Failed to extract post, skipping"
                );
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    if all_media_items.is_empty() {
        bail!("Failed to extract media items from discovered posts for @{username}");
    }

    info!(
        username = %username,
        total_media_items = all_media_items.len(),
        total_posts = post_urls.len(),
        "Profile extraction complete"
    );

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.clone(),
        caption: format!("Profile feed for @{username}"),
        post_text: None,
        published_at: latest_post_date,
        tags: aggregated_tags.into_iter().collect(),
        items: all_media_items,
        location: primary_location,
        next_page_url: None,
        discovered_post_urls: post_urls,
        embedded_player_urls: Vec::new(),
    })
}

/// Discovers all post URLs for a given Instagram profile URL.
pub async fn discover_profile_post_urls(
    input_url: &str,
    cfg: &DownloaderConfig,
    client: &Client,
) -> Result<Vec<String>> {
    let parsed_url = reqwest::Url::parse(input_url)?;
    let clean_path = parsed_url.path().trim_matches('/');
    let username = clean_path
        .split('/')
        .next()
        .filter(|s| !s.is_empty() && *s != "explore" && *s != "direct")
        .context("Could not extract username from Instagram profile URL")?;

    info!(username, "Starting profile timeline post discovery");

    let cookie = cfg.ig_cookie.as_deref().unwrap_or("");
    let csrf_token = cfg
        .ig_csrf_token
        .clone()
        .or_else(|| extract_cookie_val(cookie, "csrftoken"))
        .unwrap_or_default();

    let lsd = cfg.ig_lsd.clone().unwrap_or_else(|| csrf_token.clone());
    let dtsg = cfg.ig_fb_dtsg.clone().unwrap_or_default();
    let jazoest = compute_jazoest(&dtsg);

    let ds_user_id = extract_cookie_val(cookie, "ds_user_id")
        .or_else(|| {
            extract_cookie_val(cookie, "sessionid")
                .and_then(|s| s.split(':').next().map(|v| v.to_string()))
        })
        .unwrap_or_else(|| "0".to_string());

    debug!(username, ds_user_id = %ds_user_id, "Built profile request credentials");

    let mut discovered_post_urls = Vec::new();
    let mut seen_codes = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut page_count = 0;
    const MAX_PAGES: usize = 100;

    loop {
        page_count += 1;
        debug!(username, page = page_count, has_cursor = cursor.is_some(), "Requesting profile timeline page");

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

        let status = resp.status();
        if !status.is_success() {
            error!(username, page = page_count, status = %status, "Instagram GraphQL query failed");
            if discovered_post_urls.is_empty() {
                bail!("Instagram GraphQL query failed with HTTP {status}");
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
                error!(username, error = %e, preview = %&body[..body.len().min(300)], "Failed to parse timeline JSON");
                if discovered_post_urls.is_empty() {
                    bail!("Failed to parse Instagram GraphQL JSON: {e}");
                } else {
                    break;
                }
            }
        };

        if let Some(err_code) = payload.get("error") {
            let msg = payload.get("errorSummary").and_then(|s| s.as_str()).unwrap_or("unknown error");
            error!(username, error_code = %err_code, summary = %msg, "Meta returned an API-level error");
            if discovered_post_urls.is_empty() {
                bail!("Meta GraphQL error {err_code}: {msg}. Verify session cookies in .env.");
            } else {
                break;
            }
        }

        let (edges, page_info) = if let Some(conn) = payload.pointer("/data/xdt_api__v1__feed__user_timeline_graphql_connection") {
            (conn.get("edges").and_then(|e| e.as_array()), conn.get("page_info"))
        } else if let Some(conn) = payload.pointer("/data/user/edge_owner_to_timeline_media") {
            (conn.get("edges").and_then(|e| e.as_array()), conn.get("page_info"))
        } else {
            warn!(username, preview = %&body[..body.len().min(300)], "Could not locate timeline connection in payload");
            if discovered_post_urls.is_empty() {
                bail!("Failed to locate timeline connection in GraphQL response.");
            } else {
                break;
            }
        };

        let raw_edges = match edges {
            Some(arr) if !arr.is_empty() => arr,
            _ => {
                debug!(username, "No edges found on this page, ending pagination");
                break;
            }
        };

        let mut new_on_page = 0;
        for edge in raw_edges {
            let node = edge.get("node").unwrap_or(edge);
            if let Some(code) = node.get("code").or_else(|| node.get("shortcode")).and_then(|c| c.as_str()) {
                if seen_codes.insert(code.to_string()) {
                    discovered_post_urls.push(format!("https://www.instagram.com/p/{code}/"));
                    new_on_page += 1;
                }
            }
        }

        info!(username, page = page_count, new_posts = new_on_page, total_discovered = discovered_post_urls.len(), "Timeline page processed");

        let has_next_page = page_info
            .and_then(|p| p.get("has_next_page"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);

        let next_cursor = page_info
            .and_then(|p| p.get("end_cursor"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        if !has_next_page || next_cursor.is_none() || page_count >= MAX_PAGES {
            debug!(username, has_next_page, max_pages_reached = page_count >= MAX_PAGES, "Finished timeline pagination loop");
            break;
        }

        cursor = next_cursor;
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    if discovered_post_urls.is_empty() {
        error!(username, "0 posts discovered for profile");
        bail!("No posts found for @{username}");
    }

    info!(username, total_posts = discovered_post_urls.len(), "Discovered all profile post links");
    Ok(discovered_post_urls)
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

    info!(username, "Extracting active stories");

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
        error!(username, status = %resp.status(), "Stories tray API request failed");
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
        warn!(username, "0 active stories found in tray");
        bail!("@{username} has 0 active stories in tray.");
    }

    let mut items = Vec::new();
    for node in raw_items {
        if let Some(mut media_item) = parse_media_item(node) {
            // Per-story slide timestamp
            let slide_date = node
                .get("taken_at")
                .and_then(|t| t.as_i64())
                .and_then(format_epoch_timestamp);

            // Per-story slide caption
            let slide_caption = node
                .pointer("/caption/text")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());

            let slide_tags = slide_caption
                .as_deref()
                .map(extract_hashtags_from_text)
                .unwrap_or_default();

            // Per-story slide location
            let slide_location = extract_and_resolve_location(node.get("location"), client).await;

            // Direct PK link
            let pk = node.get("pk").or_else(|| node.get("id")).and_then(|v| {
                v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string()))
            });
            let source_post_url = pk.map(|id| format!("https://www.instagram.com/stories/{username}/{id}/"));

            media_item.published_at = slide_date;
            media_item.caption = slide_caption.or_else(|| Some(format!("Story from @{username}")));
            media_item.tags = slide_tags;
            media_item.location = slide_location;
            media_item.source_post_url = source_post_url;

            items.push(media_item);
        }
    }

    let fallback_date = items.first().and_then(|i| i.published_at.clone());

    info!(username, count = items.len(), "Successfully extracted stories");
    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.to_string(),
        caption: format!("Active stories from @{username}"),
        post_text: None,
        published_at: fallback_date,
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
    info!(media_pk, username, "Extracting single story PK item");
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

    let published_at = item
        .get("taken_at")
        .and_then(|t| t.as_i64())
        .and_then(format_epoch_timestamp);

    let location = extract_and_resolve_location(item.get("location"), client).await;

    let caption = item
        .pointer("/caption/text")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Story item {media_pk} from @{username}"));

    let tags = extract_hashtags_from_text(&caption);
    let post_url = format!("https://www.instagram.com/stories/{username}/{media_pk}/");

    let mut items = Vec::new();
    if let Some(mut media_item) = parse_media_item(item) {
        media_item.published_at = published_at.clone();
        media_item.caption = Some(caption.clone());
        media_item.location = location.clone();
        media_item.tags = tags.clone();
        media_item.source_post_url = Some(post_url.clone());
        items.push(media_item);
    }

    info!(media_pk, username, count = items.len(), "Story PK item extracted");
    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.to_string(),
        caption: caption.clone(),
        post_text: Some(caption),
        published_at,
        tags,
        items,
        location,
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
    debug!(username, "Resolving numeric user ID via topsearch API");
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
                                    debug!(username, user_id = %pk, "Successfully resolved user ID");
                                    return Ok(pk);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    error!(username, "Could not resolve numeric target ID");
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