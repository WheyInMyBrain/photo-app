use anyhow::{bail, Context, Result};
use bytes::Bytes;
use reqwest::Client;
use std::process::Command;

const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:155.0) Gecko/20100101 Firefox/155.0";

pub async fn download_asset(
    high_res_url: &str,
    audio_url: Option<&str>,
    media_type: &str,
) -> Result<Bytes> {
    if media_type == "video" && (audio_url.is_some() || high_res_url.contains(".m3u8")) {
        let temp_dir = std::env::temp_dir();
        let out_path = temp_dir.join(format!("remux_{}.mp4", uuid::Uuid::new_v4()));

        if let Some(initial_audio) = audio_url {
            // Include CMAF and DASH patterns with different bitrates
            let is_cmaf = initial_audio.contains("CMAF_");
            let audio_candidates = if is_cmaf {
                vec![
                    initial_audio.to_string(),
                    initial_audio.replace("CMAF_AUDIO_128.mp4", "CMAF_audio.mp4"),
                    initial_audio.replace("CMAF_AUDIO_128.mp4", "CMAF_AUDIO_64.mp4"),
                    initial_audio.replace("CMAF_AUDIO_128.mp4", "DASH_AUDIO_128.mp4"),
                ]
            } else {
                vec![
                    initial_audio.to_string(),
                    initial_audio.replace("DASH_AUDIO_128.mp4", "DASH_audio.mp4"),
                    initial_audio.replace("DASH_AUDIO_128.mp4", "DASH_AUDIO_64.mp4"),
                    initial_audio.replace("DASH_AUDIO_128.mp4", "CMAF_AUDIO_128.mp4"),
                ]
            };

            let mut mux_succeeded = false;
            let mut last_error = String::new();

            for audio_candidate in audio_candidates {
                let mut cmd = Command::new("ffmpeg");
                cmd.args([
                    "-y",
                    "-headers", "Referer: https://www.reddit.com/\r\n",
                    "-user_agent", BROWSER_UA,
                    "-i", high_res_url,
                    "-headers", "Referer: https://www.reddit.com/\r\n",
                    "-user_agent", BROWSER_UA,
                    "-i", &audio_candidate,
                    "-map", "0:v:0",
                    "-map", "1:a:0",
                    "-c:v", "copy",
                    "-c:a", "aac",
                    "-movflags", "+faststart",
                    out_path.to_str().unwrap(),
                ]);

                let output = cmd.output().context("Failed executing ffmpeg command")?;

                if output.status.success() {
                    mux_succeeded = true;
                    break;
                } else {
                    last_error = String::from_utf8_lossy(&output.stderr).to_string();
                }
            }

            if !mux_succeeded {
                eprintln!("[WARN] Audio muxing failed across all candidates: {last_error}");

                // Fallback to video-only if audio track fails
                let mut fallback_cmd = Command::new("ffmpeg");
                fallback_cmd.args([
                    "-y",
                    "-headers", "Referer: https://www.reddit.com/\r\n",
                    "-user_agent", BROWSER_UA,
                    "-i", high_res_url,
                    "-c", "copy",
                    "-movflags", "+faststart",
                    out_path.to_str().unwrap(),
                ]);

                let fb_out = fallback_cmd.output().context("Fallback ffmpeg video-only failed")?;
                if !fb_out.status.success() {
                    let _ = std::fs::remove_file(&out_path);
                    bail!("ffmpeg remux failed: {}", String::from_utf8_lossy(&fb_out.stderr));
                }
            }
        } else {
            // Case B: Master HLS Stream (.m3u8)
            let mut cmd = Command::new("ffmpeg");
            cmd.args([
                "-y",
                "-headers", "Referer: https://www.reddit.com/\r\n",
                "-user_agent", BROWSER_UA,
                "-protocol_whitelist", "file,http,https,tcp,tls,crypto",
                "-i", high_res_url,
                "-c", "copy",
                "-bsf:a", "aac_adtstoasc",
                "-movflags", "+faststart",
                out_path.to_str().unwrap(),
            ]);

            let output = cmd.output().context("Failed executing ffmpeg command")?;
            if !output.status.success() {
                let _ = std::fs::remove_file(&out_path);
                bail!("ffmpeg HLS remux failed: {}", String::from_utf8_lossy(&output.stderr));
            }
        }

        let data = std::fs::read(&out_path).context("Failed reading remuxed file")?;
        let _ = std::fs::remove_file(&out_path);

        return Ok(Bytes::from(data));
    }

    // Direct fetch (Images, Reels, etc.)
    let client = Client::builder()
        .user_agent(BROWSER_UA)
        .build()?;

    let resp = client
        .get(high_res_url)
        .header("Referer", "https://www.reddit.com/")
        .send()
        .await
        .context(format!("Failed to stream asset from {high_res_url}"))?;

    if !resp.status().is_success() {
        bail!("CDN returned HTTP {} for URL: {}", resp.status(), high_res_url);
    }

    Ok(resp.bytes().await?)
}