use image::{imageops::FilterType, ImageFormat};
use serde::Deserialize;
use std::fs::File;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

#[derive(Deserialize, Debug, Default)]
pub struct VideoStreamSideData {
    pub rotation: Option<i64>,
}

#[derive(Deserialize, Debug, Default)]
pub struct VideoStreamInfo {
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub side_data_list: Option<Vec<VideoStreamSideData>>,
    pub tags: Option<serde_json::Value>,
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

    pub fn extract_metadata(path: &Path) -> Result<VideoMetadata, Box<dyn std::error::Error + Send + Sync>> {
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-print_format", "json",
                "-show_format",
                "-show_streams",
                path.to_str().ok_or("Invalid path string")?,
            ])
            .output()?;

        if !output.status.success() {
            return Err("ffprobe failed to read video metadata".into());
        }

        let parsed: ProbeOutput = serde_json::from_slice(&output.stdout)?;
        let mut meta = VideoMetadata::default();

        // 1. Resolution & Rotation Handling
        if let Some(streams) = parsed.streams {
            if let Some(video_stream) = streams.iter().find(|s| s.width.is_some() && s.height.is_some()) {
                let mut w = video_stream.width.unwrap_or(0);
                let mut h = video_stream.height.unwrap_or(0);

                let mut rotation = 0;
                if let Some(ref side_data) = video_stream.side_data_list {
                    if let Some(entry) = side_data.iter().find(|sd| sd.rotation.is_some()) {
                        rotation = entry.rotation.unwrap_or(0);
                    }
                }

                if rotation == 0 {
                    if let Some(ref tags) = video_stream.tags {
                        if let Some(rot_str) = tags.get("rotate").and_then(|v| v.as_str()) {
                            rotation = rot_str.parse::<i64>().unwrap_or(0);
                        }
                    }
                }

                if rotation.abs() == 90 || rotation.abs() == 270 {
                    std::mem::swap(&mut w, &mut h);
                }

                meta.width = w;
                meta.height = h;
            }
        }

        // 2. Duration & Metadata (with Apple QuickTime fallbacks)
        if let Some(format) = parsed.format {
            if let Some(dur_str) = format.duration {
                meta.duration_seconds = dur_str.parse::<f64>().unwrap_or(0.0);
            }

            if let Some(tags) = format.tags {
                let creation = tags.get("creation_time")
                    .or_else(|| tags.get("com.apple.quicktime.creationdate"))
                    .and_then(|v| v.as_str());

                if let Some(c) = creation {
                    meta.captured_at = Some(c.to_string());
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

    pub fn generate_poster(
        video_path: &Path,
        asset_id: &str,
        target_shard_dir: &Path,
    ) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        let thumb_filename = format!("{}_thumb.webp", asset_id);
        let preview_filename = format!("{}_preview.webp", asset_id);

        let thumb_dest = target_shard_dir.join(&thumb_filename);
        let preview_dest = target_shard_dir.join(&preview_filename);

        let output = Command::new("ffmpeg")
            .args([
                "-ss", "00:00:00.500",
                "-i", video_path.to_str().ok_or("Invalid path string")?,
                "-vframes", "1",
                "-f", "image2pipe",
                "-vcodec", "mjpeg",
                "-",
            ])
            .output()?;

        if !output.status.success() || output.stdout.is_empty() {
            let fallback_output = Command::new("ffmpeg")
                .args([
                    "-i", video_path.to_str().ok_or("Invalid path string")?,
                    "-vframes", "1",
                    "-f", "image2pipe",
                    "-vcodec", "mjpeg",
                    "-",
                ])
                .output()?;

            if !fallback_output.status.success() || fallback_output.stdout.is_empty() {
                return Err("ffmpeg failed to extract video frame".into());
            }

            return Self::write_derivatives_from_jpeg_bytes(
                &fallback_output.stdout,
                &thumb_dest,
                &preview_dest,
                target_shard_dir,
                &thumb_filename,
                &preview_filename,
            );
        }

        Self::write_derivatives_from_jpeg_bytes(
            &output.stdout,
            &thumb_dest,
            &preview_dest,
            target_shard_dir,
            &thumb_filename,
            &preview_filename,
        )
    }

    fn write_derivatives_from_jpeg_bytes(
        bytes: &[u8],
        thumb_dest: &Path,
        preview_dest: &Path,
        target_shard_dir: &Path,
        thumb_filename: &str,
        preview_filename: &str,
    ) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        let img = image::load(Cursor::new(bytes), ImageFormat::Jpeg)?;

        // 1. Grid Thumbnail (320px WebP)
        let thumb = img.thumbnail(320, 320);
        let mut thumb_file = File::create(thumb_dest)?;
        thumb.write_to(&mut thumb_file, ImageFormat::WebP)?;
        thumb_file.sync_all().ok();
        drop(thumb_file);

        // 2. Preview Poster (1600px WebP)
        let preview = img.resize(1600, 1600, FilterType::Triangle);
        let mut preview_file = File::create(preview_dest)?;
        preview.write_to(&mut preview_file, ImageFormat::WebP)?;
        preview_file.sync_all().ok();
        drop(preview_file);

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