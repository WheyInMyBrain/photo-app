// src/downloader/hls.rs

use super::DownloadRequest;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

pub async fn download_hls_stream<P: AsRef<Path>>(req: &DownloadRequest, destination: P) -> Result<()> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y"); // Overwrite if exists

    // Pass custom referer / UA headers to ffmpeg so CDNs don't block segment calls
    if let Some(ref r) = req.referer {
        cmd.arg("-headers").arg(format!("Referer: {r}\r\n"));
    }

    // Input 1: Primary video or multiplexed stream
    cmd.arg("-i").arg(&req.url);

    // Input 2: Optional separate audio stream
    if let Some(ref audio_url) = req.audio_url {
        if let Some(ref r) = req.referer {
            cmd.arg("-headers").arg(format!("Referer: {r}\r\n"));
        }
        cmd.arg("-i").arg(audio_url);
    }

    // Stream copy without re-encoding (instant and low CPU)
    cmd.arg("-c").arg("copy");
    cmd.arg("-movflags").arg("+faststart");
    cmd.arg(destination.as_ref());

    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context("Failed to spawn ffmpeg for HLS processing")?;
    let status = child.wait().await?;

    if !status.success() {
        bail!("ffmpeg failed assembling HLS stream into target file");
    }

    Ok(())
}