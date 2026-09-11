use anyhow::{bail, Context, Result};
use bytes::Bytes;
use reqwest::Client;
use std::process::Command;

const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:155.0) Gecko/20100101 Firefox/155.0";

/// Global download function: Takes the high-res URL and optional audio URL,
/// automatically remuxes with ffmpeg if audio is separate or stream is HLS (.m3u8),
/// or performs a direct CDN stream download for standard images/videos.
pub async fn download_asset(
    high_res_url: &str,
    audio_url: Option<&str>,
    media_type: &str,
) -> Result<Bytes> {
    // 1. FFmpeg Remux Case: Master HLS stream (.m3u8)
    if media_type == "video" && high_res_url.contains(".m3u8") {
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-user_agent", BROWSER_UA,
                "-i", high_res_url,
                "-c", "copy",
                "-bsf:a", "aac_adtstoasc",
                "-movflags", "frag_keyframe+empty_moov",
                "-f", "mp4",
                "pipe:1",
            ])
            .output()
            .context("Failed executing ffmpeg for HLS stream")?;

        if !output.status.success() {
            bail!("ffmpeg HLS remux failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        return Ok(Bytes::from(output.stdout));
    }

    // 2. FFmpeg Remux Case: Split DASH streams (Video URL + Audio URL)
    if let Some(audio) = audio_url {
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-user_agent", BROWSER_UA,
                "-i", high_res_url,
                "-i", audio,
                "-c", "copy",
                "-movflags", "frag_keyframe+empty_moov",
                "-f", "mp4",
                "pipe:1",
            ])
            .output()
            .context("Failed executing ffmpeg for split DASH streams")?;

        if output.status.success() {
            return Ok(Bytes::from(output.stdout));
        }
        // If FFmpeg fails on split streams, it will fall through to direct video fetch
    }

    // 3. Direct CDN Fetch: Images, multiplexed MP4s (Instagram reels/stories), and fallback video
    let client = Client::builder()
        .user_agent(BROWSER_UA)
        .build()?;

    let resp = client
        .get(high_res_url)
        .send()
        .await
        .context(format!("Failed to stream asset from {high_res_url}"))?;

    if !resp.status().is_success() {
        bail!("CDN returned HTTP {} for URL: {}", resp.status(), high_res_url);
    }

    Ok(resp.bytes().await?)
}