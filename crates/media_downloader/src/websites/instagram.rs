// crates/media_downloader/src/websites/instagram.rs

use crate::config::DownloaderConfig;
use crate::models::{
    ExtractedLocation, ExtractedMediaMetadata, MediaDimensions, MediaItem, MediaType, MediaVariant,
};
use crate::utils::page::extract_hashtags_from_text;
use crate::utils::rate_limiter::StealthRateLimiter;
use crate::websites::Extractor;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use wreq::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, COOKIE, ORIGIN, REFERER,
    USER_AGENT,
};
use wreq::Client;
use wreq_util::Emulation;

const SAFARI_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4.1 Safari/605.1.15";
const APP_ID: &str = "936619743392459";
const ASBD_ID: &str = "359341";

// Fallbacks if dynamic extraction fails
const FALLBACK_PROFILE_INITIAL_DOC_ID: &str = "28991540097136703";
const FALLBACK_PROFILE_PAGINATION_DOC_ID: &str = "29240983615539641";
const GQL_POST_DOC_ID: &str = "10015901848480474";

#[derive(Debug, Clone)]
struct DynamicSessionContext {
    fb_dtsg: String,
    jazoest: String,
    lsd: String,
    ds_user_id: String,
    initial_doc_id: String,
    pagination_doc_id: String,
    req_counter: Arc<AtomicU32>,
}

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
    info!(url = %input_url, "Starting Instagram extraction via wreq BoringSSL impersonation");

    // 1. Stories and Highlights
    if input_url.contains("/stories/") {
        let cfg = config.context("DownloaderConfig required for Instagram stories")?;
        let auth_client = build_safari_client(Some(cfg))?;
        if input_url.contains("/stories/highlights/") {
            return extract_highlight_links(input_url, &auth_client).await;
        } else {
            return extract_story_links(input_url, &auth_client).await;
        }
    }

    // 2. Posts and Reels
    if input_url.contains("/p/") || input_url.contains("/reel/") || input_url.contains("/reels/") {
        return extract_single_post(input_url, config).await;
    }

    // 3. Profile Grid Feed
    let cfg = config.context("DownloaderConfig with active credentials required to scrape profile feed")?;
    let auth_client = build_safari_client(Some(cfg))?;
    extract_user_profile_feed(input_url, cfg, &auth_client, None, None).await
}

// -----------------------------------------------------------------------------
// Dynamic Session Context Scraper
// -----------------------------------------------------------------------------
async fn fetch_dynamic_profile_context(
    client: &Client,
    username: &str,
    cfg: &DownloaderConfig,
) -> Result<DynamicSessionContext> {
    let profile_url = format!("https://www.instagram.com/{username}/");
    debug!(username, "Bootstrapping profile HTML to extract tokens and doc IDs");

    let resp = client
        .get(&profile_url)
        .header(ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Sec-Fetch-Site", "none")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Dest", "document")
        .send()
        .await
        .context("Failed loading initial Instagram profile page")?;

    let html = resp.text().await.unwrap_or_default();

    // 1. Scrape fb_dtsg (falls back to config)
    let fb_dtsg = extract_regex_value(&html, r#""DTSGInitialData",\[\],\{"token":"([^"]+)""#)
        .or_else(|| extract_regex_value(&html, r#"name="fb_dtsg" value="([^"]+)""#))
        .or_else(|| cfg.ig_fb_dtsg.clone())
        .unwrap_or_default();

    let jazoest = compute_jazoest(&fb_dtsg);

    // 2. Scrape LSD token
    let lsd = extract_regex_value(&html, r#""LSD",\[\],\{"token":"([^"]+)""#)
        .or_else(|| extract_regex_value(&html, r#"name="lsd" value="([^"]+)""#))
        .or_else(|| cfg.ig_lsd.clone())
        .unwrap_or_else(|| cfg.ig_csrf_token.clone().unwrap_or_default());

    // 3. Scrape Document IDs
    let initial_doc_id = extract_regex_value(&html, r#""PolarisProfilePostsQuery"[^}]+doc_id":"(\d+)""#)
        .unwrap_or_else(|| FALLBACK_PROFILE_INITIAL_DOC_ID.to_string());

    let pagination_doc_id = extract_regex_value(
        &html,
        r#""PolarisProfilePostsTabContentQuery_connection"[^}]+doc_id":"(\d+)""#,
    )
    .unwrap_or_else(|| FALLBACK_PROFILE_PAGINATION_DOC_ID.to_string());

    // 4. Resolve user ID from cookies
    let cookie = cfg.ig_cookie.as_deref().unwrap_or("");
    let ds_user_id = extract_cookie_val(cookie, "ds_user_id")
        .or_else(|| {
            extract_cookie_val(cookie, "sessionid")
                .and_then(|s| s.split(':').next().map(|v| v.to_string()))
        })
        .unwrap_or_else(|| "0".to_string());

    info!(
        username,
        has_dtsg = !fb_dtsg.is_empty(),
        has_lsd = !lsd.is_empty(),
        initial_doc_id = %initial_doc_id,
        pagination_doc_id = %pagination_doc_id,
        "Synchronized dynamic page parameters"
    );

    Ok(DynamicSessionContext {
        fb_dtsg,
        jazoest,
        lsd,
        ds_user_id,
        initial_doc_id,
        pagination_doc_id,
        req_counter: Arc::new(AtomicU32::new(1)),
    })
}

// -----------------------------------------------------------------------------
// User Profile Feed Crawler
// -----------------------------------------------------------------------------
pub async fn extract_user_profile_feed(
    input_url: &str,
    cfg: &DownloaderConfig,
    client: &Client,
    initial_cursor: Option<String>,
    page_budget: Option<usize>,
) -> Result<ExtractedMediaMetadata> {
    let username = extract_username_from_url(input_url)
        .context("Could not extract username from Instagram profile URL")?;

    let max_pages = page_budget.unwrap_or(30);
    info!(username = %username, max_pages, "Starting profile timeline extraction");

    let rate_limiter = StealthRateLimiter::new();
    let mut backoff_attempt: u32 = 0;

    let session_ctx = fetch_dynamic_profile_context(client, &username, cfg).await?;

    let mut all_media_items = Vec::new();
    let mut discovered_post_urls = Vec::new();
    let mut seen_codes = HashSet::new();
    let mut aggregated_tags = HashSet::new();
    let mut latest_post_date = None;
    let mut primary_location = None;

    let mut cursor: Option<String> = initial_cursor;
    let mut page_count = 0;
    let mut hit_known_boundary = false;
    let mut next_cursor_to_return: Option<String> = None;

    loop {
        page_count += 1;
        let current_req = session_ctx.req_counter.fetch_add(1, Ordering::SeqCst);

        debug!(
            username = %username,
            page = page_count,
            __req = current_req,
            has_cursor = cursor.is_some(),
            "Requesting profile timeline GraphQL connection"
        );

        let (doc_id, friendly_name, variables) = if let Some(ref c) = cursor {
            (
                &session_ctx.pagination_doc_id,
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
                &session_ctx.initial_doc_id,
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
        let encoded_dtsg = urlencoding::encode(&session_ctx.fb_dtsg);

        let form_body = format!(
            "av={}&__d=www&__user=0&__a=1&__req={current_req}&dpr=2&__comet_req=7&fb_dtsg={}&jazoest={}&lsd={}&fb_api_caller_class=RelayModern&fb_api_req_friendly_name={friendly_name}&server_timestamps=true&doc_id={doc_id}&variables={encoded_vars}",
            session_ctx.ds_user_id, encoded_dtsg, session_ctx.jazoest, session_ctx.lsd
        );

        let resp = match client
            .post("https://www.instagram.com/graphql/query")
            .header(REFERER, format!("https://www.instagram.com/{username}/"))
            .header(ORIGIN, "https://www.instagram.com")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(ACCEPT, "*/*")
            .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header("X-FB-Friendly-Name", friendly_name)
            .header("X-IG-App-ID", APP_ID)
            .header("X-ASBD-ID", ASBD_ID)
            .header("X-FB-LSD", &session_ctx.lsd)
            .header("Sec-Fetch-Site", "same-origin")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Dest", "empty")
            .header("X-Root-Field-Name", "xdt_api__v1__feed__user_timeline_graphql_connection")
            .body(form_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Network error during profile query, entering backoff");
                backoff_attempt += 1;
                if backoff_attempt > 3 {
                    bail!("Persistent network error contacting Instagram: {e}");
                }
                rate_limiter.handle_backoff(backoff_attempt).await;
                continue;
            }
        };

        let status = resp.status();

        if status == wreq::StatusCode::TOO_MANY_REQUESTS || status == wreq::StatusCode::FORBIDDEN {
            warn!(status = %status, "Rate limit HTTP code encountered from Instagram. Stopping session.");
            bail!("Rate limited by Instagram (HTTP {status}). Stopping to preserve account session.");
        }

        if !status.is_success() {
            error!(username = %username, page = page_count, status = %status, "Timeline query returned failure status");
            if all_media_items.is_empty() && !hit_known_boundary {
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
                error!(username = %username, error = %e, preview = %&body[..body.len().min(300)], "Failed to parse timeline JSON");
                if all_media_items.is_empty() && !hit_known_boundary {
                    bail!("Failed to parse Instagram GraphQL JSON: {e}");
                } else {
                    break;
                }
            }
        };

        let message = payload.get("message").and_then(|m| m.as_str()).unwrap_or("");
        let status_field = payload.get("status").and_then(|s| s.as_str()).unwrap_or("");

        if status_field == "fail"
            || message.contains("feedback_required")
            || message.contains("checkpoint_required")
            || message.contains("login_required")
        {
            error!(
                status = status_field,
                message = message,
                "CRITICAL: Action challenge / checkpoint triggered. Halting crawl."
            );
            bail!("Scraper stopped: Instagram challenge triggered ('{message}').");
        }

        backoff_attempt = 0;

        let (edges, page_info) = if let Some(conn) = payload.pointer("/data/xdt_api__v1__feed__user_timeline_graphql_connection") {
            (conn.get("edges").and_then(|e| e.as_array()), conn.get("page_info"))
        } else if let Some(conn) = payload.pointer("/data/user/edge_owner_to_timeline_media") {
            (conn.get("edges").and_then(|e| e.as_array()), conn.get("page_info"))
        } else {
            warn!(username = %username, "Could not locate timeline connection in response");
            if all_media_items.is_empty() && !hit_known_boundary {
                bail!("Failed to locate timeline connection in GraphQL response.");
            } else {
                break;
            }
        };

        let raw_edges = match edges {
            Some(arr) if !arr.is_empty() => arr,
            _ => {
                debug!(username = %username, "No edges found on this page, ending pagination");
                break;
            }
        };

        let mut new_items_on_page = 0;

        for edge in raw_edges {
            let node = edge.get("node").unwrap_or(edge);
            let code = match node.get("code").or_else(|| node.get("shortcode")).and_then(|c| c.as_str()) {
                Some(c) => c,
                None => continue,
            };

            if cfg.is_known_post(code) {
                info!(shortcode = %code, username = %username, "Encountered previously downloaded post. Stopping early.");
                hit_known_boundary = true;
                break;
            }

            if !seen_codes.insert(code.to_string()) {
                continue;
            }

            let post_url = format!("https://www.instagram.com/p/{code}/");
            discovered_post_urls.push(post_url.clone());

            let caption = node
                .pointer("/caption/text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            let post_tags = extract_hashtags_from_text(&caption);
            for tag in &post_tags {
                aggregated_tags.insert(tag.clone());
            }

            let published_at = node
                .get("taken_at")
                .and_then(|t| t.as_i64())
                .and_then(format_epoch_timestamp);

            if latest_post_date.is_none() && published_at.is_some() {
                latest_post_date = published_at.clone();
            }

            let post_location = extract_and_resolve_location(node.get("location"), client, Some(&rate_limiter)).await;
            if primary_location.is_none() && post_location.is_some() {
                primary_location = post_location.clone();
            }

            let raw_media_nodes: Vec<&Value> = if let Some(carousel) = node.get("carousel_media").and_then(|c| c.as_array()) {
                carousel.iter().collect()
            } else {
                vec![node]
            };

            for media_node in raw_media_nodes {
                if let Some(mut media_item) = parse_media_item(media_node) {
                    let slide_location = if media_node.get("location").is_some() {
                        extract_and_resolve_location(media_node.get("location"), client, Some(&rate_limiter)).await
                    } else {
                        post_location.clone()
                    };

                    media_item.source_post_url = Some(post_url.clone());
                    media_item.caption = if !caption.is_empty() {
                        Some(caption.clone())
                    } else {
                        None
                    };
                    media_item.published_at = published_at.clone();
                    media_item.location = slide_location;
                    media_item.tags = post_tags.clone();

                    all_media_items.push(media_item);
                    new_items_on_page += 1;
                }
            }
        }

        info!(
            username = %username,
            page = page_count,
            items_parsed_this_page = new_items_on_page,
            total_items = all_media_items.len(),
            total_posts = discovered_post_urls.len(),
            "Timeline page parsed directly"
        );

        if hit_known_boundary {
            next_cursor_to_return = None;
            break;
        }

        let has_next_page = page_info
            .and_then(|p| p.get("has_next_page"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);

        let next_cursor = page_info
            .and_then(|p| p.get("end_cursor"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        if !has_next_page || next_cursor.is_none() {
            next_cursor_to_return = None;
            break;
        }

        if page_count >= max_pages {
            info!(username = %username, pages_completed = page_count, "Batch quota reached.");
            next_cursor_to_return = next_cursor;
            break;
        }

        cursor = next_cursor;
        rate_limiter.wait_for_next_page().await;
    }

    if all_media_items.is_empty() && !hit_known_boundary {
        bail!("No media items could be extracted for @{username}");
    }

    Ok(ExtractedMediaMetadata {
        platform: "instagram".to_string(),
        author: username.clone(),
        caption: format!("Profile feed for @{username}"),
        post_text: None,
        published_at: latest_post_date,
        tags: aggregated_tags.into_iter().collect(),
        items: all_media_items,
        location: primary_location,
        next_page_url: next_cursor_to_return,
        discovered_post_urls,
        embedded_player_urls: Vec::new(),
    })
}

// -----------------------------------------------------------------------------
// Single Post Resolution
// -----------------------------------------------------------------------------
pub async fn extract_single_post(
    input_url: &str,
    config: Option<&DownloaderConfig>,
) -> Result<ExtractedMediaMetadata> {
    let shortcode = extract_shortcode(input_url).context("Could not extract Instagram shortcode")?;
    let client = build_safari_client(config)?;
    fetch_graphql_post_info(shortcode, &client).await
}

async fn fetch_graphql_post_info(shortcode: &str, client: &Client) -> Result<ExtractedMediaMetadata> {
    let gql_url = format!(
        "https://www.instagram.com/graphql/query/?doc_id={GQL_POST_DOC_ID}&variables=%7B%22shortcode%22%3A%22{shortcode}%22%7D"
    );

    let resp = client
        .get(&gql_url)
        .header(REFERER, format!("https://www.instagram.com/p/{shortcode}/"))
        .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .header("Sec-Fetch-Site", "same-origin")
        .header("Sec-Fetch-Mode", "cors")
        .header("Sec-Fetch-Dest", "empty")
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

    if let Some(edges) = media.pointer("/edge_sidecar_to_children/edges").and_then(|e| e.as_array()) {
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

    let location = extract_and_resolve_location(media.get("location"), client, None).await;
    let published_at = media
        .get("taken_at_timestamp")
        .or_else(|| media.get("taken_at"))
        .and_then(|t| t.as_i64())
        .and_then(format_epoch_timestamp);

    for item in &mut items {
        item.source_post_url = Some(format!("https://www.instagram.com/p/{shortcode}/"));
        item.caption = if !caption.is_empty() { Some(caption.clone()) } else { None };
        item.published_at = published_at.clone();
        item.location = location.clone();
        item.tags = tags.clone();
    }

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

// -----------------------------------------------------------------------------
// Location & Story Resolvers
// -----------------------------------------------------------------------------
async fn extract_and_resolve_location(
    loc_val: Option<&Value>,
    client: &Client,
    limiter: Option<&StealthRateLimiter>,
) -> Option<ExtractedLocation> {
    let loc = loc_val?;
    let name = loc.get("name").and_then(|n| n.as_str())?.trim().to_string();
    if name.is_empty() { return None; }

    let mut latitude = loc.get("lat").and_then(|l| l.as_f64());
    let mut longitude = loc.get("lng").and_then(|l| l.as_f64());
    let location_id = loc.get("pk").or_else(|| loc.get("id")).map(|v| v.to_string().trim_matches('"').to_string());

    if (latitude.is_none() || longitude.is_none()) && location_id.is_some() {
        if let Some(ref lid) = location_id {
            if let Some(rl) = limiter { rl.wait_lightweight().await; }
            let loc_api = format!("https://www.instagram.com/api/v1/locations/{lid}/info/");
            if let Ok(resp) = client.get(&loc_api).header(REFERER, "https://www.instagram.com/").send().await {
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

    Some(ExtractedLocation { name, latitude, longitude, location_id })
}

async fn extract_highlight_links(_url: &str, _client: &Client) -> Result<ExtractedMediaMetadata> {
    bail!("Highlights extraction pending session refresh");
}

async fn extract_story_links(_url: &str, _client: &Client) -> Result<ExtractedMediaMetadata> {
    bail!("Stories extraction pending session refresh");
}

// -----------------------------------------------------------------------------
// Media Parsers
// -----------------------------------------------------------------------------
fn parse_graphql_node(node: &Value) -> Option<MediaItem> {
    let is_video = node.get("is_video").and_then(|v| v.as_bool()).unwrap_or(false);
    let display_url = node.get("display_url").and_then(|u| u.as_str()).map(clean_url)?;

    let width = node.pointer("/dimensions/width").and_then(|w| w.as_u64()).unwrap_or(0) as usize;
    let height = node.pointer("/dimensions/height").and_then(|h| h.as_u64()).unwrap_or(0) as usize;
    let dims = if width > 0 && height > 0 { Some(MediaDimensions { width, height }) } else { None };

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
        if let Some(resources) = node.get("display_resources").and_then(|r| r.as_array()) {
            for res in resources {
                if let Some(src) = res.get("src").and_then(|s| s.as_str()) {
                    let cleaned = clean_url(src);
                    let w = res.get("config_width").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let h = res.get("config_height").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    variants.push(MediaVariant {
                        url: cleaned,
                        dimensions: if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None },
                        file_size_bytes: None,
                        label: Some(format!("{w}w")),
                    });
                }
            }
        }

        let high_res_url = variants.last().map(|v| v.url.clone()).unwrap_or_else(|| display_url.clone());
        let thumb_url = variants.first().map(|v| v.url.clone()).or_else(|| Some(display_url.clone()));

        let mut media = MediaItem::new(
            MediaType::Image,
            "image/jpeg",
            dims,
            None,
            high_res_url.clone(),
            thumb_url,
            None,
            None,
            Some("https://www.instagram.com/".to_string()),
            high_res_url,
        );
        media.variants = variants;
        Some(media)
    }
}

fn parse_media_item(item: &Value) -> Option<MediaItem> {
    let media_type = item.get("media_type").and_then(|t| t.as_i64()).unwrap_or(1);
    let img_candidates = item.pointer("/image_versions2/candidates").and_then(|c| c.as_array())?;
    let default_thumb_url = img_candidates.last().and_then(|c| c.get("url")).and_then(|u| u.as_str()).map(clean_url);

    if media_type == 2 {
        let versions = item.get("video_versions")?.as_array()?;
        let first = versions.first()?;
        let video_url = first.get("url").and_then(|u| u.as_str()).map(clean_url)?;
        let w = first.get("width").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
        let h = first.get("height").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
        let dims = if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None };

        Some(MediaItem::new(
            MediaType::Video,
            "video/mp4",
            dims,
            None,
            video_url.clone(),
            default_thumb_url,
            None,
            None,
            Some("https://www.instagram.com/".to_string()),
            video_url,
        ))
    } else {
        let first = img_candidates.first()?;
        let image_url = first.get("url").and_then(|u| u.as_str()).map(clean_url)?;
        let w = first.get("width").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
        let h = first.get("height").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
        let dims = if w > 0 && h > 0 { Some(MediaDimensions { width: w, height: h }) } else { None };

        Some(MediaItem::new(
            MediaType::Image,
            "image/jpeg",
            dims,
            None,
            image_url.clone(),
            default_thumb_url,
            None,
            None,
            Some("https://www.instagram.com/".to_string()),
            image_url,
        ))
    }
}

// -----------------------------------------------------------------------------
// Client Constructor (wreq with Safari Emulation)
// -----------------------------------------------------------------------------
fn build_safari_client(cfg: Option<&DownloaderConfig>) -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(SAFARI_UA));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers.insert("X-IG-App-ID", HeaderValue::from_static(APP_ID));
    headers.insert("X-ASBD-ID", HeaderValue::from_static(ASBD_ID));
    headers.insert(ORIGIN, HeaderValue::from_static("https://www.instagram.com"));
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));

    if let Some(config) = cfg {
        if let Some(ref cookie) = config.ig_cookie {
            headers.insert(COOKIE, HeaderValue::from_str(cookie)?);
        }
        if let Some(ref csrf) = config.ig_csrf_token {
            headers.insert("X-CSRFToken", HeaderValue::from_str(csrf)?);
        }
        if let Some(ref lsd) = config.ig_lsd {
            headers.insert("X-FB-LSD", HeaderValue::from_str(lsd)?);
        }
    }

    // Emulate Apple Safari TLS handshake & HTTP/2 protocol frames
    let client = Client::builder()
        .emulation(Emulation::Safari26)
        .default_headers(headers)
        .build()?;

    Ok(client)
}

// -----------------------------------------------------------------------------
// Utilities & Helpers
// -----------------------------------------------------------------------------
fn extract_regex_value(haystack: &str, pattern: &str) -> Option<String> {
    if let Some(start_prefix) = pattern.split("([^\"#]+)").next() {
        if let Some(pos) = haystack.find(start_prefix) {
            let rest = &haystack[pos + start_prefix.len()..];
            if let Some(end_pos) = rest.find('"') {
                return Some(rest[..end_pos].to_string());
            }
        }
    }
    None
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

fn format_epoch_timestamp(epoch_secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(epoch_secs, 0).map(|dt| dt.to_rfc3339())
}

pub fn extract_username_from_url(input_url: &str) -> Option<String> {
    let without_query = input_url.split('?').next().unwrap_or(input_url);
    let without_fragment = without_query.split('#').next().unwrap_or(without_query);
    let path = without_fragment
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_start_matches("instagram.com/")
        .trim_start_matches("instagr.am/")
        .trim_matches('/');

    let username = path.split('/').next()?.trim();
    if username.is_empty()
        || username == "explore"
        || username == "direct"
        || username == "stories"
        || username == "p"
        || username == "reel"
        || username == "reels"
    {
        return None;
    }

    Some(username.to_string())
}