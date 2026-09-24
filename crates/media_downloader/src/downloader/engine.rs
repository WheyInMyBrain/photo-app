// src/downloader/engine.rs

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;

const BROWSER_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:155.0) Gecko/20100101 Firefox/155.0";

/// Universal download dispatcher: handles direct files, HLS streams, and dual-track muxing
pub async fn download_to_disk<P: AsRef<Path>>(
    video_url: &str,
    audio_url: Option<&str>,
    destination: P,
    referer: Option<&str>,
) -> Result<PathBuf> {
    let dest = destination.as_ref();
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if let Some(audio) = audio_url {
        mux_streams(video_url, audio, dest, referer).await?;
    } else if video_url.contains(".m3u8") {
        download_single_hls(video_url, dest, referer).await?;
    } else {
        stream_direct_to_disk(video_url, dest, referer).await?;
    }

    Ok(dest.to_path_buf())
}

// -----------------------------------------------------------------------------
// Direct Progressive File Streaming (No RAM Buffer, No extra cargo features)
// -----------------------------------------------------------------------------
pub async fn stream_direct_to_disk<P: AsRef<Path>>(
    url: &str,
    destination: P,
    referer: Option<&str>,
) -> Result<u64> {
    let client = reqwest::Client::new();
    let mut req = client.get(url).header("User-Agent", BROWSER_UA);

    if let Some(ref_val) = referer {
        req = req.header("Referer", ref_val);
    }

    let mut resp = req.send().await.context("Failed to send request")?;
    if !resp.status().is_success() {
        bail!("Server returned HTTP {}", resp.status());
    }

    let dest_ref = destination.as_ref();
    if let Some(parent) = dest_ref.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut file = File::create(dest_ref)
        .await
        .with_context(|| format!("Failed to create destination file: {:?}", dest_ref))?;

    let mut total_bytes: u64 = 0;

    // Stream directly using native resp.chunk()
    while let Some(chunk) = resp.chunk().await.context("Error reading response chunk")? {
        file.write_all(&chunk).await?;
        total_bytes += chunk.len() as u64;
    }

    file.flush().await?;
    Ok(total_bytes)
}

// -----------------------------------------------------------------------------
// Dual Stream Muxer (Video + Audio)
// -----------------------------------------------------------------------------
async fn mux_streams(
    video_url: &str,
    audio_url: &str,
    destination: &Path,
    referer: Option<&str>,
) -> Result<()> {
    let ref_header = referer.unwrap_or("https://www.reddit.com/");
    let header_arg = format!("Referer: {ref_header}\r\nUser-Agent: {BROWSER_UA}\r\n");

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y");

    // Input 0: Video stream
    cmd.arg("-headers").arg(&header_arg)
       .arg("-i").arg(video_url);

    // Input 1: Audio stream
    cmd.arg("-headers").arg(&header_arg)
       .arg("-i").arg(audio_url);

    cmd.args([
        "-map", "0:v:0",
        "-map", "1:a:0?",
        "-c:v", "copy",
        "-c:a", "copy",
        "-bsf:a", "aac_adtstoasc",
        "-movflags", "+faststart",
    ]);

    cmd.arg(destination);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().await.context("Failed to execute ffmpeg")?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        bail!("ffmpeg muxing failed: {err_msg}");
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Single HLS Stream (.m3u8)
// -----------------------------------------------------------------------------
async fn download_single_hls(
    hls_url: &str,
    destination: &Path,
    referer: Option<&str>,
) -> Result<()> {
    let ref_header = referer.unwrap_or("https://www.reddit.com/");
    let header_arg = format!("Referer: {ref_header}\r\nUser-Agent: {BROWSER_UA}\r\n");

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
       .arg("-headers").arg(&header_arg)
       .arg("-protocol_whitelist").arg("file,http,https,tcp,tls,crypto")
       .arg("-i").arg(hls_url)
       .args([
           "-c", "copy",
           "-bsf:a", "aac_adtstoasc",
           "-movflags", "+faststart",
       ])
       .arg(destination);

    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().await.context("Failed to execute ffmpeg")?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        bail!("ffmpeg HLS download failed: {err_msg}");
    }

    Ok(())
}

/// Fetches media directly into memory without any byte size limits.
pub async fn stream_to_memory(
    url: &str,
    referer: Option<&str>,
) -> Result<(Bytes, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let mut req = client.get(url).header("User-Agent", BROWSER_UA);
    if let Some(ref_val) = referer {
        req = req.header("Referer", ref_val);
    }

    let resp = req.send().await.context("Failed to send in-memory stream request")?;
    if !resp.status().is_success() {
        bail!("Server returned HTTP {}", resp.status());
    }

    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    let bytes = resp.bytes().await.context("Failed to read stream bytes")?;

    Ok((bytes, mime))
}

/// Fetches full media bytes and returns the raw base64 string with NO size cap,
/// formatted without the data: prefix so Apple Shortcuts can decode it natively.
pub async fn stream_thumbnail_base64(url: &str, referer: Option<&str>) -> Option<String> {
    let (bytes, _mime) = stream_to_memory(url, referer).await.ok()?;
    Some(STANDARD.encode(&bytes))
}