// src/filter/grouping.rs

use crate::models::{MediaItem, MediaType, MediaVariant};
use std::collections::HashMap;

pub fn deduplicate_media(items: Vec<MediaItem>) -> Vec<MediaItem> {
    let mut image_groups: HashMap<String, Vec<MediaItem>> = HashMap::new();
    let mut video_groups: HashMap<String, Vec<MediaItem>> = HashMap::new();
    let mut passthrough: Vec<MediaItem> = Vec::new();

    for item in items {
        match item.media_type {
            MediaType::Image => {
                let key = compute_image_fingerprint(&item.high_res_url);
                image_groups.entry(key).or_default().push(item);
            }
            MediaType::Video => {
                if item.high_res_url.contains(".m3u8") {
                    let key = compute_hls_stream_group_key(&item.high_res_url);
                    video_groups.entry(key).or_default().push(item);
                } else {
                    passthrough.push(item);
                }
            }
        }
    }

    let mut final_items = Vec::new();

    // 1. Merge Video Groups into single items containing all quality variants
    for (_group_key, variants) in video_groups {
        if let Some(merged) = merge_video_group(variants) {
            final_items.push(merged);
        }
    }

    // 2. Merge Image Groups into single items containing all dimension variants
    for (_group_key, variants) in image_groups {
        if let Some(merged) = merge_image_group(variants) {
            final_items.push(merged);
        }
    }

    final_items.extend(passthrough);
    final_items
}

fn compute_image_fingerprint(url: &str) -> String {
    let base = url.split('?').next().unwrap_or(url);
    if let Some((path, ext)) = base.rsplit_once('.') {
        let clean_path = strip_dimension_suffix(path);
        format!("{clean_path}.{ext}")
    } else {
        base.to_string()
    }
}

fn strip_dimension_suffix(path: &str) -> &str {
    if let Some((prefix, suffix)) = path.rsplit_once('_') {
        if suffix.chars().all(|c| c.is_ascii_digit()) || suffix.ends_with('w') || suffix.ends_with('p') {
            return prefix;
        }
    }
    if let Some((prefix, suffix)) = path.rsplit_once('-') {
        if suffix.chars().all(|c| c.is_ascii_digit()) || suffix.starts_with("scaled") {
            return prefix;
        }
    }
    path
}

fn compute_hls_stream_group_key(url: &str) -> String {
    if let Some((dir, _filename)) = url.rsplit_once('/') {
        dir.to_string()
    } else {
        url.to_string()
    }
}

/// Merges multiple image resolutions into one parent item while preserving all variants
fn merge_image_group(mut items: Vec<MediaItem>) -> Option<MediaItem> {
    if items.is_empty() {
        return None;
    }

    // Sort ascending by pixel area and size (highest quality ends up at the end)
    items.sort_by_key(|item| {
        let area = item
            .dimensions
            .as_ref()
            .map(|d| d.width * d.height)
            .unwrap_or(0);
        let size = item.file_size_bytes.unwrap_or(0);
        (area, size)
    });

    let mut collected_variants = Vec::new();
    for it in &items {
        for v in &it.variants {
            // Avoid duplicate variant entries
            if !collected_variants.iter().any(|existing: &MediaVariant| existing.url == v.url) {
                collected_variants.push(v.clone());
            }
        }
    }

    // Best candidate is the last item
    let mut best_item = items.pop()?;
    best_item.variants = collected_variants;
    Some(best_item)
}

/// Merges multiple video resolutions into one parent item, linking audio and preserving all renditions
fn merge_video_group(mut items: Vec<MediaItem>) -> Option<MediaItem> {
    if items.is_empty() {
        return None;
    }

    // Identify and capture external audio tracks
    let audio_track = items.iter().find_map(|v| {
        let lower = v.high_res_url.to_lowercase();
        if lower.contains("audio") || v.audio_url.is_some() {
            v.audio_url.clone().or_else(|| Some(v.high_res_url.clone()))
        } else {
            None
        }
    });

    // Collect all unique variants (including audio tracks for inspection)
    let mut collected_variants = Vec::new();
    for it in &items {
        for v in &it.variants {
            if !collected_variants.iter().any(|existing: &MediaVariant| existing.url == v.url) {
                let mut variant = v.clone();
                if variant.label.is_none() {
                    variant.label = infer_video_quality_label(&variant.url);
                }
                collected_variants.push(variant);
            }
        }
    }

    // Strip audio-only URLs from competing as the primary video stream
    items.retain(|v| !v.high_res_url.to_lowercase().contains("audio_"));

    items.sort_by_key(|item| {
        let area = item
            .dimensions
            .as_ref()
            .map(|d| d.width * d.height)
            .unwrap_or(0);

        let rank_heuristic = if item.high_res_url.contains("4k") || item.high_res_url.contains("2160") {
            4000
        } else if item.high_res_url.contains("1080") {
            2000
        } else if item.high_res_url.contains("720") {
            1000
        } else if item.high_res_url.contains("480") {
            500
        } else {
            0
        };

        (area, rank_heuristic)
    });

    let mut best_item = items.pop()?;
    if best_item.audio_url.is_none() {
        best_item.audio_url = audio_track;
    }
    best_item.variants = collected_variants;
    Some(best_item)
}

fn infer_video_quality_label(url: &str) -> Option<String> {
    let lower = url.to_lowercase();
    if lower.contains("4k") || lower.contains("2160") {
        Some("4K".to_string())
    } else if lower.contains("1080") {
        Some("1080p".to_string())
    } else if lower.contains("720") {
        Some("720p".to_string())
    } else if lower.contains("480") {
        Some("480p".to_string())
    } else if lower.contains("audio") {
        Some("Audio Track".to_string())
    } else {
        None
    }
}