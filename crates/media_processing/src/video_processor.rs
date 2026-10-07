use image::{DynamicImage, ImageFormat};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufWriter, Cursor};
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

#[derive(Debug, Clone)]
pub struct VideoDerivatives {
    pub thumb_rel: String,    // Static WebP
    pub motion_rel: String,   // 480p silent 3-second MP4 loop
    pub preview_rel: String,  // 720p H.264 FastStart MP4
}

pub struct VideoProcessor;

impl VideoProcessor {
    /// Determines whether the file should be routed through the video/motion pipeline
    pub fn is_video_or_anim(ext: &str) -> bool {
        matches!(
            ext.to_lowercase().as_str(),
            "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" | "gif"
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
            return Err("ffprobe failed to read video/gif metadata".into());
        }

        let parsed: ProbeOutput = serde_json::from_slice(&output.stdout)?;
        let mut meta = VideoMetadata::default();

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

        if let Some(format) = parsed.format {
            if let Some(dur_str) = format.duration {
                meta.duration_seconds = dur_str.parse::<f64>().unwrap_or(0.0);
            }

            if let Some(tags) = format.tags {
                let creation = tags
                    .get("creation_time")
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

    pub fn sample_frames(
        video_path: &Path,
        duration_seconds: f64,
        sample_count: usize,
    ) -> Vec<DynamicImage> {
        let dur = if duration_seconds <= 0.0 { 1.0 } else { duration_seconds };
        if sample_count == 0 {
            return Vec::new();
        }

        let mut frames = Vec::with_capacity(sample_count);
        let interval = dur / (sample_count + 1) as f64;

        for i in 1..=sample_count {
            let timestamp = interval * i as f64;
            let time_str = format!("{:.3}", timestamp);

            let res = Command::new("ffmpeg")
                .args([
                    "-ss", &time_str,
                    "-i", match video_path.to_str() {
                        Some(p) => p,
                        None => continue,
                    },
                    "-vframes", "1",
                    "-f", "image2pipe",
                    "-vcodec", "mjpeg",
                    "-",
                ])
                .output();

            if let Ok(output) = res {
                if output.status.success() && !output.stdout.is_empty() {
                    if let Ok(img) = image::load(Cursor::new(&output.stdout), ImageFormat::Jpeg) {
                        frames.push(img);
                    }
                }
            }
        }

        frames
    }

    /// Generates the complete 3-tier delivery payload: Static WebP, Motion MP4, and 720p H.264 FastStart proxy
    pub fn generate_all_derivatives(
        input_path: &Path,
        asset_id: &str,
        target_shard_dir: &Path,
        duration_seconds: f64,
    ) -> Result<VideoDerivatives, Box<dyn std::error::Error + Send + Sync>> {
        let path_str = input_path.to_str().ok_or("Invalid path string")?;

        // Ensure the 2-tier target directory exists before executing encoders
        std::fs::create_dir_all(target_shard_dir)?;

        let thumb_name = format!("{}_thumb.webp", asset_id);
        let motion_name = format!("{}_motion.mp4", asset_id);
        let preview_name = format!("{}_preview.mp4", asset_id);

        let thumb_dest = target_shard_dir.join(&thumb_name);
        let motion_dest = target_shard_dir.join(&motion_name);
        let preview_dest = target_shard_dir.join(&preview_name);

        // Extract 2-tier shard components from asset_id (e.g., "a1" and "b2")
        let clean_id = asset_id.replace('-', "");
        let shard_1 = clean_id.get(0..2).unwrap_or("00");
        let shard_2 = clean_id.get(2..4).unwrap_or("00");

        // 1. Static Poster Thumbnail (Fast seek to 0.5s or 0s)
        let seek_time = if duration_seconds > 1.0 { "0.500" } else { "0.000" };
        let thumb_output = Command::new("ffmpeg")
            .args([
                "-ss", seek_time,
                "-i", path_str,
                "-vframes", "1",
                "-f", "image2pipe",
                "-vcodec", "mjpeg",
                "-",
            ])
            .output()?;

        if !thumb_output.status.success() || thumb_output.stdout.is_empty() {
            let err = String::from_utf8_lossy(&thumb_output.stderr);
            return Err(format!("FFmpeg failed generating poster frame: {err}").into());
        }

        let img = image::load(Cursor::new(&thumb_output.stdout), ImageFormat::Jpeg)?;
        let thumb = img.thumbnail(320, 320);
        {
            let f = File::create(&thumb_dest)?;
            let mut writer = BufWriter::with_capacity(64 * 1024, f);
            thumb.write_to(&mut writer, ImageFormat::WebP)?;
        }

        // Bounded scale filter: maintains aspect ratio while bounding within target box
        // and guarantees even dimensions for yuv420p
        let scale_480p = "scale=480:480:force_original_aspect_ratio=decrease,scale=trunc(iw/2)*2:trunc(ih/2)*2";
        let scale_720p = "scale=1280:720:force_original_aspect_ratio=decrease,scale=trunc(iw/2)*2:trunc(ih/2)*2";

        // 2. Motion Hover Clip (480p silent loop, max 4s, CRF 28)
        let motion_dur = if duration_seconds > 0.0 { duration_seconds.min(4.0) } else { 4.0 };
        let motion_status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss", seek_time,
                "-t", &format!("{:.2}", motion_dur),
                "-i", path_str,
                "-an",
                "-vf", scale_480p,
                "-c:v", "libx264",
                "-preset", "veryfast",
                "-crf", "28",
                "-pix_fmt", "yuv420p",
                "-movflags", "+faststart",
                motion_dest.to_str().ok_or("Invalid motion path")?,
            ])
            .output()?;

        if !motion_status.status.success() {
            let err = String::from_utf8_lossy(&motion_status.stderr);
            return Err(format!("FFmpeg failed generating motion preview: {err}").into());
        }

        // 3. Web Streaming Proxy (720p, H.264 + AAC, FastStart)
        let preview_status = Command::new("ffmpeg")
            .args([
                "-y",
                "-i", path_str,
                "-vf", scale_720p,
                "-c:v", "libx264",
                "-preset", "veryfast",
                "-crf", "23",
                "-c:a", "aac",
                "-b:a", "128k",
                "-ac", "2",
                "-pix_fmt", "yuv420p",
                "-movflags", "+faststart",
                preview_dest.to_str().ok_or("Invalid preview path")?,
            ])
            .output()?;

        if !preview_status.status.success() {
            let err = String::from_utf8_lossy(&preview_status.stderr);
            return Err(format!("FFmpeg failed generating preview video: {err}").into());
        }

        Ok(VideoDerivatives {
            thumb_rel: format!("{}/{}/{}", shard_1, shard_2, thumb_name),
            motion_rel: format!("{}/{}/{}", shard_1, shard_2, motion_name),
            preview_rel: format!("{}/{}/{}", shard_1, shard_2, preview_name),
        })
    }
}