// src/filter/rules.rs

use crate::models::{MediaItem, MediaType};

/// Checks if an item has an invalid protocol, non-media MIME type, tracking pattern, or chunked stream segment
pub fn is_junk_item(item: &MediaItem) -> bool {
    // 1. Protocol validation: must be standard http or https web assets
    if !is_valid_protocol(&item.high_res_url) {
        return true;
    }

    // 2. MIME type verification: discard scripts, web pages, and raw text
    if !is_valid_media_mime(&item.mime_type, item.media_type.clone()) {
        return true;
    }

    // 3. Reject chunked transport segments (.ts, .m4s), ad networks, and vector UI assets
    if is_stream_segment_or_ad(&item.high_res_url) {
        return true;
    }

    // 4. Junk URL path indicators: strip non-media extensions, favicons, tracking pixels
    is_junk_path(&item.high_res_url)
}

/// Allows only fetchable web protocols (rejects concat:, javascript:, mailto:, data:)
fn is_valid_protocol(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Validates that the detected or probed MIME type corresponds to actual media
fn is_valid_media_mime(mime: &str, media_type: MediaType) -> bool {
    let lower = mime.to_lowercase();

    // Explicitly reject web code, documents, and analytics pings
    if lower.starts_with("text/")
        || lower.contains("javascript")
        || lower.contains("json")
        || lower.contains("xml")
        || lower.contains("html")
        || (lower == "application/octet-stream" && media_type == MediaType::Image)
    {
        return false;
    }

    // Allow standard media content types
    lower.starts_with("image/")
        || lower.starts_with("video/")
        || lower.starts_with("audio/")
        || lower == "application/x-mpegurl"
        || lower == "application/vnd.apple.mpegurl"
        || lower == "application/dash+xml"
}

/// Rejects transport chunks, ad networks, and vector UI assets
pub fn is_stream_segment_or_ad(url: &str) -> bool {
    let lower = url.to_lowercase();
    let clean = lower.split('?').next().unwrap_or(&lower);

    // 1. Chunked stream segments (keep playlists like .m3u8, but drop individual chunk requests)
    if clean.ends_with(".ts")
        || clean.ends_with(".m4s")
        || clean.ends_with(".aac")
        || clean.contains("/segment_")
        || clean.contains("/segment-")
        || clean.contains("/frag_")
        || clean.contains("/fragment-")
        || clean.contains("-seg-")
        || clean.contains("/range/")
    {
        return true;
    }

    // 2. Vector UI icons and graphics
    if clean.ends_with(".svg") || lower.contains("image/svg+xml") {
        return true;
    }

    // 3. Known ad delivery exchanges
    let is_ad = lower.contains("doubleclick.net")
        || lower.contains("googlesyndication.com")
        || lower.contains("adnxs.com")
        || lower.contains("/pagead/")
        || lower.contains("trafficjunky")
        || lower.contains("exoclick")
        || lower.contains("scorecardresearch.com");

    is_ad
}

/// Discards common non-media extensions and tracking markers
fn is_junk_path(url: &str) -> bool {
    let lower = url.to_lowercase();

    // Script, stylesheet, and manifest extensions
    if lower.contains(".js?")
        || lower.ends_with(".js")
        || lower.contains(".css?")
        || lower.ends_with(".css")
        || lower.contains(".webmanifest")
        || lower.contains(".json")
    {
        return true;
    }

    // Common tracking, avatar, and micro-icon markers
    lower.contains("/avatar_")
        || lower.contains("/favicon")
        || lower.contains("thumb_")
        || lower.contains("-thumb.")
        || lower.contains("_thumb.")
        || lower.contains("thumbnail_")
        || lower.contains("badge_")
        || lower.contains("emoji/")
        || lower.contains("pixel.gif")
        || lower.contains("1x1.")
        || lower.contains("/gsi/client")
        || lower.contains("accounts.google.com")
}