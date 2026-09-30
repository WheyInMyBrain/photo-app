// src/downloader/metadata.rs

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone, Default)]
pub struct MediaMetadataPayload<'a> {
    pub author: Option<&'a str>,
    pub caption: Option<&'a str>,
    pub source_url: Option<&'a str>,
    pub tags: &'a [String],
    pub published_at: Option<&'a str>,
    pub location_name: Option<&'a str>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

pub async fn inject_metadata<P: AsRef<Path>>(
    file_path: P,
    meta: &MediaMetadataPayload<'_>,
) -> Result<()> {
    let path = file_path.as_ref();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" => {
            inject_image_exiftool(path, meta).await?;
        }
        "mp4" | "mov" | "m4v" => {
            inject_video_ffmpeg(path, meta).await?;
        }
        _ => {}
    }

    Ok(())
}

async fn inject_image_exiftool(path: &Path, meta: &MediaMetadataPayload<'_>) -> Result<()> {
    let mut cmd = Command::new("exiftool");
    cmd.arg("-overwrite_original");

    if let Some(author) = meta.author {
        cmd.arg(format!("-Artist={author}"));
        cmd.arg(format!("-By-line={author}"));
    }

    if let Some(caption) = meta.caption {
        cmd.arg(format!("-ImageDescription={caption}"));
        cmd.arg(format!("-Description={caption}"));
    }

    if let Some(url) = meta.source_url {
        cmd.arg(format!("-Source={url}"));
    }

    if !meta.tags.is_empty() {
        for tag in meta.tags {
            cmd.arg(format!("-Keywords={tag}"));
        }
    }

    if let Some(published) = meta.published_at {
        // EXIF date standard: "YYYY:MM:DD HH:MM:SS"
        let clean_date = published.replace('-', ":").replace('T', " ").replace('Z', "");
        let exif_date = clean_date.split('.').next().unwrap_or(&clean_date);
        cmd.arg(format!("-DateTimeOriginal={exif_date}"));
        cmd.arg(format!("-CreateDate={exif_date}"));
    }

    if let Some(loc_name) = meta.location_name {
        cmd.arg(format!("-City={loc_name}"));
        cmd.arg(format!("-Location={loc_name}"));
    }

    if let (Some(lat), Some(lng)) = (meta.latitude, meta.longitude) {
        cmd.arg(format!("-GPSLatitude={lat}"));
        cmd.arg(format!("-GPSLatitudeRef={}", if lat >= 0.0 { "N" } else { "S" }));
        cmd.arg(format!("-GPSLongitude={lng}"));
        cmd.arg(format!("-GPSLongitudeRef={}", if lng >= 0.0 { "E" } else { "W" }));
    }

    cmd.arg(path);

    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().await.context("Failed executing exiftool")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("exiftool metadata injection failed: {err}");
    }

    Ok(())
}

async fn inject_video_ffmpeg(path: &Path, meta: &MediaMetadataPayload<'_>) -> Result<()> {
    let temp_out = path.with_extension("meta_tmp.mp4");

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-loglevel").arg("error")
        .arg("-i").arg(path);

    if let Some(author) = meta.author {
        cmd.arg("-metadata").arg(format!("artist={author}"));
        cmd.arg("-metadata").arg(format!("author={author}"));
    }

    if let Some(caption) = meta.caption {
        cmd.arg("-metadata").arg(format!("title={caption}"));
        cmd.arg("-metadata").arg(format!("description={caption}"));
        cmd.arg("-metadata").arg(format!("comment={caption}"));
    }

    if let Some(url) = meta.source_url {
        cmd.arg("-metadata").arg(format!("synopsis={url}"));
    }

    if !meta.tags.is_empty() {
        let tag_list = meta.tags.join(", ");
        cmd.arg("-metadata").arg(format!("keywords={tag_list}"));
        cmd.arg("-metadata").arg(format!("genre={tag_list}"));
    }

    if let Some(published) = meta.published_at {
        cmd.arg("-metadata").arg(format!("creation_time={published}"));
    }

    if let Some(loc_name) = meta.location_name {
        cmd.arg("-metadata").arg(format!("location_name={loc_name}"));
    }

    if let (Some(lat), Some(lng)) = (meta.latitude, meta.longitude) {
        let iso_location = format!("{:+08.4}{:+09.4}/", lat, lng);
        cmd.arg("-metadata").arg(format!("location={iso_location}"));
        cmd.arg("-metadata").arg(format!("location-eng={iso_location}"));
    }

    cmd.args(["-map", "0", "-map_metadata", "0", "-c", "copy", "-movflags", "+faststart"])
        .arg(&temp_out);

    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().await.context("Failed launching ffmpeg for video tagging")?;

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&temp_out).await;
        let err_msg = String::from_utf8_lossy(&output.stderr);
        bail!("ffmpeg video tagging failed: {err_msg}");
    }

    tokio::fs::rename(&temp_out, path).await?;
    Ok(())
}