use image::{imageops::FilterType, ImageFormat};
use serde::Deserialize;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

#[derive(Deserialize, Debug, Default)]
pub struct VideoStreamInfo {
    pub width: Option<i64>,
    pub height: Option<i64>,
}

#[derive(Deserialize, Debug, Default)]
pub struct VideoProbeFormat {
    pub duration: Option<String>,
    pub tags: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug, Default)]
pub struct ProbeOutput {
    pub streams: Option<Vec<VideoStreamInfo>>,
    pub format: Option<VideoProbeFormat>,
}

#[derive(Debug, Default)]
pub struct VideoMetadata {
    pub width: i64,
    pub height: i64,
    pub duration_seconds: f64,
    pub captured_at: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
}

pub struct VideoProcessor;

impl VideoProcessor {
    pub fn is_video(ext: &str) -> bool {
        matches!(
            ext.to_lowercase().as_str(),
            "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi"
        )
    }

    /// Runs ffprobe to extract dimensions, duration, and capture time
    pub fn extract_metadata(path: &Path) -> Result<VideoMetadata, Box<dyn std::error::Error + Send + Sync>> {
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-print_format", "json",
                "-show_format",
                "-show_streams",
                path.to_str().unwrap(),
            ])
            .output()?;

        if !output.status.success() {
            return Err("ffprobe failed to read video metadata".into());
        }

        let parsed: ProbeOutput = serde_json::from_slice(&output.stdout)?;

        let mut meta = VideoMetadata::default();

        // 1. Extract resolution from first video stream
        if let Some(streams) = parsed.streams {
            if let Some(video_stream) = streams.iter().find(|s| s.width.is_some() && s.height.is_some()) {
                meta.width = video_stream.width.unwrap_or(0);
                meta.height = video_stream.height.unwrap_or(0);
            }
        }

        // 2. Extract duration and QuickTime creation tags
        if let Some(format) = parsed.format {
            if let Some(dur_str) = format.duration {
                meta.duration_seconds = dur_str.parse::<f64>().unwrap_or(0.0);
            }

            if let Some(tags) = format.tags {
                if let Some(creation) = tags.get("creation_time").and_then(|v| v.as_str()) {
                    meta.captured_at = Some(creation.to_string());
                }
                if let Some(make) = tags.get("com.apple.quicktime.make").and_then(|v| v.as_str()) {
                    meta.camera_make = Some(make.to_string());
                }
                if let Some(model) = tags.get("com.apple.quicktime.model").and_then(|v| v.as_str()) {
                    meta.camera_model = Some(model.to_string());
                }
            }
        }

        Ok(meta)
    }

    /// Extracts a JPEG frame via FFmpeg directly to stdout, then uses Rust to encode WebP files
    pub fn generate_poster(
        video_path: &Path,
        asset_id: &str,
        target_shard_dir: &Path,
    ) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        let thumb_filename = format!("{}_thumb.webp", asset_id);
        let preview_filename = format!("{}_preview.webp", asset_id);

        let thumb_dest = target_shard_dir.join(&thumb_filename);
        let preview_dest = target_shard_dir.join(&preview_filename);

        // FFmpeg writes 1 JPEG frame directly to stdout buffer (no temp file needed)
        let output = Command::new("ffmpeg")
            .args([
                "-ss", "00:00:00.100",
                "-i", video_path.to_str().unwrap(),
                "-vframes", "1",
                "-f", "image2pipe",
                "-vcodec", "mjpeg",
                "-",
            ])
            .output()?;

        if !output.status.success() || output.stdout.is_empty() {
            return Err("ffmpeg failed to extract video frame".into());
        }

        // Load the JPEG frame directly from memory buffer
        let img = image::load(Cursor::new(output.stdout), ImageFormat::Jpeg)?;

        // 1. Grid Thumbnail (320px WebP)
        let thumb = img.thumbnail(320, 320);
        let mut thumb_file = std::fs::File::create(&thumb_dest)?;
        thumb.write_to(&mut thumb_file, ImageFormat::WebP)?;

        // 2. High-res Single View Preview (max 1600px WebP)
        let preview = img.resize(1600, 1600, FilterType::Triangle);
        let mut preview_file = std::fs::File::create(&preview_dest)?;
        preview.write_to(&mut preview_file, ImageFormat::WebP)?;

        let shard = target_shard_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("00");

        Ok((
            format!("thumbs/{}/{}", shard, thumb_filename),
            format!("thumbs/{}/{}", shard, preview_filename),
        ))
    }
}