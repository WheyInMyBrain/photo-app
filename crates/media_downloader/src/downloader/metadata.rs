// src/downloader/metadata.rs

use anyhow::{bail, Context, Result};
use little_exif::exif_tag::ExifTag;
use little_exif::metadata::Metadata;
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
}

/// Dispatches metadata injection based on file extension / media type
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
            // little_exif is synchronous, execute inside spawn_blocking
            let owned_path = path.to_path_buf();
            let author = meta.author.map(|s| s.to_string());
            let caption = meta.caption.map(|s| s.to_string());
            let published_at = meta.published_at.map(|s| s.to_string());
            let tags = meta.tags.to_vec();

            tokio::task::spawn_blocking(move || {
                inject_image_exif(&owned_path, author.as_deref(), caption.as_deref(), published_at.as_deref(), &tags)
            })
            .await??;
        }
        "mp4" | "mov" | "m4v" => {
            inject_video_ffmpeg(path, meta).await?;
        }
        _ => {
            // Unsupported formats (e.g. raw gif, webm) safely pass through
        }
    }

    Ok(())
}

fn inject_image_exif(
    path: &Path,
    author: Option<&str>,
    caption: Option<&str>,
    published_at: Option<&str>,
    tags: &[String],
) -> Result<()> {
    let mut exif = Metadata::new_from_path(path).unwrap_or_else(|_| Metadata::new());

    if let Some(author_val) = author {
        exif.set_tag(ExifTag::Artist(author_val.to_string()));
    }

    if let Some(cap) = caption {
        exif.set_tag(ExifTag::ImageDescription(cap.to_string()));
    }

    if !tags.is_empty() {
        let tag_line = tags.join(", ");
        let mut comment_bytes = b"ASCII\0\0\0".to_vec();
        comment_bytes.extend_from_slice(tag_line.as_bytes());
        exif.set_tag(ExifTag::UserComment(comment_bytes));
    }

    if let Some(pub_date) = published_at {
        // EXIF standard format: "YYYY:MM:DD HH:MM:SS"
        let clean_date = pub_date.replace('-', ":").replace('T', " ").replace('Z', "");
        let exif_date = clean_date.split('.').next().unwrap_or(&clean_date);
        exif.set_tag(ExifTag::DateTimeOriginal(exif_date.to_string()));
    }

    // Write directly into the existing file header
    let _ = exif.write_to_file(path);
    Ok(())
}

async fn inject_video_ffmpeg(path: &Path, meta: &MediaMetadataPayload<'_>) -> Result<()> {
    let temp_out = path.with_extension("meta_tmp.mp4");

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").arg("-i").arg(path);

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

    cmd.args(["-c", "copy", "-movflags", "+faststart"])
        .arg(&temp_out);

    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().await.context("Failed executing ffmpeg metadata tagging")?;

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&temp_out).await;
        let err_msg = String::from_utf8_lossy(&output.stderr);
        bail!("ffmpeg metadata injection failed: {err_msg}");
    }

    // Replace downloaded file with tagged file
    tokio::fs::rename(&temp_out, path).await?;
    Ok(())
}