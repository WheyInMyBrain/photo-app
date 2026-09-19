// src/utils/page.rs

use super::patterns::{IFRAME_SRC_RE, META_DESC_RE, TAG_HREF_RE, TIME_RE};
use super::url::resolve_relative_url;
use std::collections::HashSet;

pub fn extract_published_time(html: &str) -> Option<String> {
    TIME_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

pub fn extract_post_text(html: &str) -> Option<String> {
    META_DESC_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

pub fn extract_tags(html: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = HashSet::new();

    for caps in TAG_HREF_RE.captures_iter(html) {
        if let Some(matched) = caps.get(1) {
            let tag = matched.as_str().trim().to_lowercase();
            if !tag.is_empty() && seen.insert(tag.clone()) {
                tags.push(tag);
            }
        }
    }

    tags
}

pub fn extract_hashtags_from_text(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for word in text.split_whitespace() {
        if let Some(tag) = word.strip_prefix('#') {
            // Trim trailing punctuation like commas, periods, quotes, or emojis
            let clean_tag = tag.trim_matches(|c: char| {
                c.is_ascii_punctuation() || c.is_whitespace()
            });

            if !clean_tag.is_empty() && seen.insert(clean_tag.to_lowercase()) {
                tags.push(clean_tag.to_string());
            }
        }
    }

    tags
}

pub fn extract_embedded_players(current_url: &str, html: &str) -> Vec<String> {
    let mut players = Vec::new();
    let mut seen = HashSet::new();

    for caps in IFRAME_SRC_RE.captures_iter(html) {
        if let Some(matched) = caps.get(1) {
            let full_url = resolve_relative_url(current_url, matched.as_str());
            let lower = full_url.to_lowercase();

            let is_known_player = lower.contains("youtube.com/embed")
                || lower.contains("youtu.be")
                || lower.contains("player.vimeo.com")
                || lower.contains("redgifs.com/ifr")
                || lower.contains("streamable.com/e/")
                || lower.contains("megacloud")
                || lower.contains("rabbitstream");

            if is_known_player && seen.insert(full_url.clone()) {
                players.push(full_url);
            }
        }
    }

    players
}