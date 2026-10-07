// photo-app/backend/src/routes/upload/utils.rs

use chrono::Local;
use std::path::{Path, PathBuf};
use tokio::fs::create_dir_all;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;
use db::domain::RawUploadQuery;
use media_downloader::MediaType;

pub fn default_camera_folder() -> String {
    format!("Camera Roll/{}", Local::now().format("%Y-%m"))
}

pub async fn resolve_user_temp_dir(
    state: &AppState,
    user_id: Uuid,
    sub: &str,
) -> Result<PathBuf, AppError> {
    let dir = state
        .config
        .storage_root
        .join("temp")
        .join(sub)
        .join(user_id.to_string());
    create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed creating temp dir {dir:?}: {e}")))?;
    Ok(dir)
}

pub fn detect_extension_from_magic_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 {
        return None;
    }

    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }

    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png");
    }

    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }

    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }

    if &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        return match brand {
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => Some("heic"),
            b"qt  " => Some("mov"),
            _ => Some("mp4"),
        };
    }

    None
}

pub fn resolve_incoming_filename(
    query: &RawUploadQuery,
    headers: &axum::http::HeaderMap,
    body: &[u8],
) -> String {
    let mut file_name = query
        .file_name
        .clone()
        .or_else(|| {
            headers
                .get("X-File-Name")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let has_extension = Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| !e.is_empty())
        .unwrap_or(false);

    if !has_extension {
        let ext = query
            .ext
            .clone()
            .or_else(|| {
                headers
                    .get("X-File-Ext")
                    .and_then(|h| h.to_str().ok())
                    .map(|s| s.trim_start_matches('.').to_string())
            })
            .or_else(|| detect_extension_from_magic_bytes(body).map(|s| s.to_string()))
            .unwrap_or_else(|| "jpg".to_string());

        file_name = format!("{}.{}", file_name, ext);
    }

    file_name
}

pub fn ext_from_mime(mime: &str, media_type: &MediaType) -> &'static str {
    let clean = mime.split(';').next().unwrap_or(mime).trim();
    match clean {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/heic" | "image/heif" => "heic",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/x-matroska" => "mkv",
        "application/x-mpegurl" | "application/vnd.apple.mpegurl" => "mp4",
        _ => match media_type {
            MediaType::Video => "mp4",
            MediaType::Image => "jpg",
        },
    }
}

pub fn build_suggested_filename(caption: &str, index: usize, total_items: usize, ext: &str) -> String {
    let prefix = caption
        .chars()
        .take(30)
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase();

    let safe_prefix = if prefix.is_empty() { "media" } else { &prefix };

    if total_items > 1 {
        format!("{}_{}.{}", safe_prefix, index + 1, ext)
    } else {
        format!("{}.{}", safe_prefix, ext)
    }
}