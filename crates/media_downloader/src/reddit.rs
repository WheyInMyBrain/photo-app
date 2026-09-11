use anyhow::{bail, Context, Result};
use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use reqwest::Client;
use serde_json::Value;
use std::process::Command;

use crate::models::{DownloadedAsset, DownloadedBatch, ExtractedMediaMetadata, MediaItem, MediaType};

const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:155.0) Gecko/20100101 Firefox/155.0";

// -----------------------------------------------------------------------------
// Public Downloader Entrypoint
// -----------------------------------------------------------------------------
pub async fn download(input_url: &str) -> Result<DownloadedBatch> {
    let meta = extract_links(input_url).await?;

    let download_client = Client::builder()
        .user_agent(BROWSER_UA)
        .build()?;

    let clean_caption = sanitize_caption_for_filename(&meta.caption);
    let target_folder = format!("reddit/{}", meta.author);
    let total_items = meta.items.len();

    let mut assets = Vec::new();

    for (idx, item) in meta.items.iter().enumerate() {
        let raw_bytes: Bytes = match item.media_type {
            MediaType::Video => {
                // Remux via FFmpeg if audio is separate or stream is HLS
                let video_buf = fetch_and_remux_video(&item.high_res_url, item.audio_url.as_deref(), &download_client).await?;
                Bytes::from(video_buf)
            }
            MediaType::Image => {
                let resp = download_client
                    .get(&item.high_res_url)
                    .send()
                    .await
                    .context(format!("Failed downloading image asset: {}", item.high_res_url))?;

                if !resp.status().is_success() {
                    bail!("CDN returned HTTP {} for image: {}", resp.status(), item.high_res_url);
                }

                resp.bytes().await?
            }
        };

        let ext = match item.media_type {
            MediaType::Video => "mp4",
            MediaType::Image => "jpg",
        };

        let file_name = if total_items > 1 {
            format!("{}_{}.{}", clean_caption, idx + 1, ext)
        } else {
            format!("{}.{}", clean_caption, ext)
        };

        assets.push(DownloadedAsset {
            file_name,
            bytes: raw_bytes,
        });
    }

    Ok(DownloadedBatch {
        platform: meta.platform,
        author: meta.author,
        caption: meta.caption,
        target_folder,
        assets,
    })
}

// -----------------------------------------------------------------------------
// Link Extraction
// -----------------------------------------------------------------------------
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
    if let Some(gallery_items) = post_data.get("gallery_data").and_then(|g| g.get("items")).and_then(|i| i.as_array()) {
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

            items.push(MediaItem {
                media_type,
                high_res_url: clean_url_str(url),
                audio_url: None,
                thumbnail_url: clean_url_str(&thumbnail_url),
            });
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

// -----------------------------------------------------------------------------
// Parsers
// -----------------------------------------------------------------------------
fn parse_reddit_video(vid: &Value, post_data: &Value) -> Option<MediaItem> {
    let has_audio = vid.get("has_audio").and_then(|b| b.as_bool()).unwrap_or(false);
    let fallback_url = vid.get("fallback_url").and_then(|u| u.as_str())?;
    let hls_url = vid.get("hls_url").and_then(|u| u.as_str());

    let clean_fallback = fallback_url.split('?').next().unwrap_or(fallback_url);

    // If HLS is absent but audio exists, target the DASH 128kbps audio channel
    let audio_url = if has_audio && hls_url.is_none() {
        clean_fallback.rsplit_once('/').map(|(base, _)| format!("{base}/DASH_AUDIO_128.mp4"))
    } else {
        None
    };

    // Prefer HLS master playlist for video+audio sync
    let high_res_url = hls_url.unwrap_or(fallback_url);
    let thumbnail_url = extract_preview_thumbnail(post_data).unwrap_or_else(|| clean_fallback.to_string());

    Some(MediaItem {
        media_type: MediaType::Video,
        high_res_url: clean_url_str(high_res_url),
        audio_url,
        thumbnail_url: clean_url_str(&thumbnail_url),
    })
}

fn parse_reddit_gallery_node(media_obj: &Value) -> Option<MediaItem> {
    let thumbnail_url = media_obj
        .get("p")
        .and_then(|arr| arr.as_array())
        .and_then(|arr| arr.first())
        .and_then(|p| p.get("u"))
        .and_then(|u| u.as_str());

    // 1. Video / GIF item in gallery
    if let Some(mp4) = media_obj.get("s").and_then(|s| s.get("mp4")).and_then(|u| u.as_str()) {
        return Some(MediaItem {
            media_type: MediaType::Video,
            high_res_url: clean_url_str(mp4),
            audio_url: None,
            thumbnail_url: clean_url_str(thumbnail_url.unwrap_or(mp4)),
        });
    }

    // 2. Photo item in gallery
    if let Some(img) = media_obj.get("s").and_then(|s| s.get("u")).and_then(|u| u.as_str()) {
        return Some(MediaItem {
            media_type: MediaType::Image,
            high_res_url: clean_url_str(img),
            audio_url: None,
            thumbnail_url: clean_url_str(thumbnail_url.unwrap_or(img)),
        });
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

// -----------------------------------------------------------------------------
// In-Memory Video Remuxing via FFmpeg
// -----------------------------------------------------------------------------
async fn fetch_and_remux_video(
    video_url: &str,
    audio_url: Option<&str>,
    client: &Client,
) -> Result<Vec<u8>> {
    // Case 1: Master HLS Stream (.m3u8)
    if video_url.contains(".m3u8") {
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-user_agent", BROWSER_UA,
                "-i", video_url,
                "-c", "copy",
                "-bsf:a", "aac_adtstoasc",
                "-movflags", "frag_keyframe+empty_moov",
                "-f", "mp4",
                "pipe:1",
            ])
            .output()
            .context("Failed executing ffmpeg for HLS stream")?;

        if !output.status.success() {
            bail!("ffmpeg HLS remux failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        return Ok(output.stdout);
    }

    // Case 2: Split DASH streams (separate video + audio inputs over network)
    if let Some(audio) = audio_url {
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-user_agent", BROWSER_UA,
                "-i", video_url,
                "-i", audio,
                "-c", "copy",
                "-movflags", "frag_keyframe+empty_moov",
                "-f", "mp4",
                "pipe:1",
            ])
            .output()
            .context("Failed executing ffmpeg for split DASH streams")?;

        if output.status.success() {
            return Ok(output.stdout);
        }
    }

    // Case 3: Silent or standalone direct video stream
    let resp = client.get(video_url).send().await?;
    if !resp.status().is_success() {
        bail!("Failed to download direct video stream with HTTP {}", resp.status());
    }

    Ok(resp.bytes().await?.to_vec())
}

// -----------------------------------------------------------------------------
// Utilities
// -----------------------------------------------------------------------------
fn sanitize_caption_for_filename(caption: &str) -> String {
    let clean: String = caption
        .chars()
        .take(30)
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();

    let trimmed = clean.trim_matches('_');
    if trimmed.is_empty() {
        "post".to_string()
    } else {
        trimmed.to_lowercase()
    }
}

fn clean_url_str(raw: &str) -> String {
    raw.replace("&amp;", "&").replace(r"\/", "/")
}