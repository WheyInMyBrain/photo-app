// src/utils/patterns.rs

use std::sync::LazyLock;

pub static VIDEO_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)(?:https?:)?\\?/\\?/[^"'\s<>]+\.(?:m3u8|mp4|webm)(?:[?&#][^"'\s<>]*)?"#).unwrap()
});

pub static IMG_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)(?:https?:)?\\?/\\?/[^"'\s<>]+\.(?:jpg|jpeg|png|pnj|webp|gif|avif)(?:[?&#][^"'\s<>]*)?"#).unwrap()
});

pub static ATTR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)(?:src|data-src|data-original|data-highres|data-image)=["']([^"']+)["']"#).unwrap()
});

pub static SRCSET_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)(?:srcset|data-srcset)=["']([^"']+)["']"#).unwrap()
});

pub static TITLE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)<title[^>]*>(.*?)</title>").unwrap()
});

pub static REL_NEXT_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<link\s+[^>]*rel=["']next["'][^>]*href=["']([^"']+)["']"#).unwrap()
});

pub static REL_NEXT_REV_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<link\s+[^>]*href=["']([^"']+)["'][^>]*rel=["']next["']"#).unwrap()
});

pub static A_REL_NEXT_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<a\s+[^>]*(?:rel=["']next["']|aria-label=["']Next(?: Page)?["'])[^>]*href=["']([^"']+)["']"#).unwrap()
});

pub static A_CLASS_NEXT_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<a\s+[^>]*class=["'][^"']*\bnext\b[^"']*["'][^>]*href=["']([^"']+)["']"#).unwrap()
});

pub static ANCHOR_HREF_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<a\s+[^>]*href=["']([^"'#\s]+)["']"#).unwrap()
});

pub static POST_PATH_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)/(?:post|posts|p|entry|watch|video|thread|status|item|detail|article)/[a-zA-Z0-9_\-\.]+"#).unwrap()
});

pub static IFRAME_SRC_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<iframe\s+[^>]*(?:src|data-src)=["']([^"']+)["']"#).unwrap()
});

pub static TIME_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<time\s+[^>]*datetime=["']([^"']+)["']"#).unwrap()
});

pub static META_DESC_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<meta\s+[^>]*(?:name=["']description["']|property=["']og:description["'])[^>]*content=["']([^"']+)["']"#).unwrap()
});

pub static TAG_HREF_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<a\s+[^>]*href=["'][^"']*/(?:tag|tags)/([^"'/?#]+)["'][^>]*>"#).unwrap()
});

/// Direct <video poster="..."> or data-poster attribute
pub static VIDEO_POSTER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<video[^>]*(?:poster|data-poster)=["']([^"']+)["']"#).unwrap()
});

/// Standard og:image and twitter:image meta tags
pub static META_POSTER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)<meta\s+[^>]*(?:property=["'](?:og:image|og:image:url)["']|name=["'](?:twitter:image|thumbnail)["'])[^>]*content=["']([^"']+)["']"#).unwrap()
});

/// Inline JSON/player configuration thumbnail declarations (poster, image, thumbnail, thumbnailUrl)
pub static JSON_POSTER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?i)["'](?:poster|thumbnailUrl|thumbnail|preview_image)["']\s*:\s*["']([^"']+)["']"#).unwrap()
});