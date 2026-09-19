// src/downloader/mux.rs

use super::engine::stream_direct_to_disk;
use super::DownloadRequest;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

pub async fn download_and_mux_direct<P: AsRef<Path>>(
    req: &DownloadRequest,
    destination: P,
) -> Result<()> {
    let audio_url = req.audio_url.as_ref().context("Missing audio URL")?;
    let dest_ref = destination.as_ref();
    let parent = dest_ref.parent().unwrap_or_else(|| Path::new("."));

    let stem = dest_ref.file_stem().and_then(|s| s.to_str()).unwrap_or("tmp");
    let tmp_video = parent.join(format!(".tmp_{stem}_v.mp4"));
    let tmp_audio = parent.join(format!(".tmp_{stem}_a.mp4"));

    // Stream both directly to temp files on disk concurrently
    let (v_res, a_res) = tokio::join!(
        stream_direct_to_disk(&req.url, &tmp_video, req.referer.as_deref()),
        stream_direct_to_disk(audio_url, &tmp_audio, req.referer.as_deref())
    );

    v_res.context("Failed streaming temporary video track to disk")?;
    a_res.context("Failed streaming temporary audio track to disk")?;

    // Mux with ffmpeg
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(&tmp_video)
        .arg("-i")
        .arg(&tmp_audio)
        .arg("-c")
        .arg("copy")
        .arg("-movflags")
        .arg("+faststart")
        .arg(dest_ref)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .context("Failed running ffmpeg muxer")?;

    // Cleanup temp files
    let _ = tokio::fs::remove_file(&tmp_video).await;
    let _ = tokio::fs::remove_file(&tmp_audio).await;

    if !status.success() {
        bail!("ffmpeg muxing failed");
    }

    Ok(())
}