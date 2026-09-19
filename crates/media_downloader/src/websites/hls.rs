// src/websites/hls.rs

use crate::models::MediaDimensions;
use crate::utils::BROWSER_UA;
use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue, REFERER, USER_AGENT};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HlsVariant {
    pub resolution: String,
    pub dimensions: Option<MediaDimensions>,
    pub bandwidth: u64,
    pub video_url: String,
    pub audio_url: Option<String>,
}

pub async fn resolve_all_quality_streams(detected_url: &str, referer: &str) -> Result<Vec<HlsVariant>> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_UA));
    if let Ok(ref_val) = HeaderValue::try_from(referer) {
        headers.insert(REFERER, ref_val);
    }

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(8))
        .build()?;

    let mut candidate_master_urls = Vec::new();
    if let Some(base) = detected_url.rsplit_once('/') {
        candidate_master_urls.push(format!("{}/playlist.m3u8", base.0));
        candidate_master_urls.push(format!("{}/master.m3u8", base.0));
        candidate_master_urls.push(format!("{}.m3u8", base.0));
    }
    candidate_master_urls.push(detected_url.to_string());

    for master_url in candidate_master_urls {
        if let Ok(resp) = client.get(&master_url).send().await {
            if resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                if body.contains("#EXT-X-STREAM-INF") {
                    let variants = parse_m3u8_variants(&body, &master_url);
                    if !variants.is_empty() {
                        return Ok(variants);
                    }
                }
            }
        }
    }

    Ok(vec![HlsVariant {
        resolution: "Source".to_string(),
        dimensions: None,
        bandwidth: 0,
        video_url: detected_url.to_string(),
        audio_url: None,
    }])
}

fn parse_m3u8_variants(playlist_body: &str, master_url: &str) -> Vec<HlsVariant> {
    let mut variants = Vec::new();
    let mut audio_groups: HashMap<String, String> = HashMap::new();
    let lines: Vec<&str> = playlist_body.lines().collect();

    for line in &lines {
        if line.starts_with("#EXT-X-MEDIA:") && line.contains("TYPE=AUDIO") {
            let mut group_id = None;
            let mut uri = None;

            for part in line.trim_start_matches("#EXT-X-MEDIA:").split(',') {
                if let Some((k, v)) = part.split_once('=') {
                    match k.trim() {
                        "GROUP-ID" => group_id = Some(v.trim().trim_matches('"').to_string()),
                        "URI" => uri = Some(v.trim().trim_matches('"').to_string()),
                        _ => {}
                    }
                }
            }

            if let (Some(gid), Some(u)) = (group_id, uri) {
                audio_groups.insert(gid, resolve_url(master_url, &u));
            }
        }
    }

    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("#EXT-X-STREAM-INF:") {
            let mut bandwidth = 0u64;
            let mut resolution_str = "Unknown".to_string();
            let mut dimensions = None;
            let mut audio_group_id = None;

            for part in line.trim_start_matches("#EXT-X-STREAM-INF:").split(',') {
                if let Some((k, v)) = part.split_once('=') {
                    match k.trim() {
                        "BANDWIDTH" => bandwidth = v.trim().parse().unwrap_or(0),
                        "RESOLUTION" => {
                            resolution_str = v.trim().to_string();
                            if let Some((w, h)) = resolution_str.split_once('x') {
                                if let (Ok(width), Ok(height)) = (w.parse::<usize>(), h.parse::<usize>()) {
                                    dimensions = Some(MediaDimensions { width, height });
                                }
                            }
                        }
                        "AUDIO" => audio_group_id = Some(v.trim().trim_matches('"').to_string()),
                        _ => {}
                    }
                }
            }

            if let Some(next_line) = lines.get(i + 1) {
                let stream_path = next_line.trim();
                if !stream_path.is_empty() && !stream_path.starts_with('#') {
                    let full_video_url = resolve_url(master_url, stream_path);
                    let matched_audio_url =
                        audio_group_id.and_then(|gid| audio_groups.get(&gid).cloned());

                    variants.push(HlsVariant {
                        resolution: resolution_str,
                        dimensions,
                        bandwidth,
                        video_url: full_video_url,
                        audio_url: matched_audio_url,
                    });
                }
            }
        }
    }

    variants.sort_by_key(|v| v.bandwidth);
    variants
}

fn resolve_url(base_url: &str, relative_or_absolute: &str) -> String {
    if relative_or_absolute.starts_with("http://") || relative_or_absolute.starts_with("https://") {
        relative_or_absolute.to_string()
    } else if let Some(base) = base_url.rsplit_once('/') {
        format!("{}/{}", base.0, relative_or_absolute)
    } else {
        relative_or_absolute.to_string()
    }
}