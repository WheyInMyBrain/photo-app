// src/downloader/mod.rs

pub mod engine;
pub mod hls;
pub mod mux;

use anyhow::Result;
use std::path::{Path, PathBuf};

/// The decoupled download request: completely agnostic of scrapers or platforms.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// Direct file link (.jpg, .mp4) or manifest link (.m3u8)
    pub url: String,

    /// Optional separate audio track (e.g., DASH audio, Reddit CMAF audio, HLS audio stream)
    pub audio_url: Option<String>,

    /// Destination directory (e.g., "./downloads" or "./downloads/instagram")
    pub output_dir: PathBuf,

    /// Final file name with extension (e.g., "my_photo.jpg", "video_4k.mp4")
    pub file_name: String,

    /// HTTP Referer if the CDN checks hotlink protection
    pub referer: Option<String>,
}

impl DownloadRequest {
    pub fn new<P: AsRef<Path>>(url: impl Into<String>, output_dir: P, file_name: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            audio_url: None,
            output_dir: output_dir.as_ref().to_path_buf(),
            file_name: file_name.into(),
            referer: None,
        }
    }

    pub fn with_audio(mut self, audio_url: impl Into<String>) -> Self {
        self.audio_url = Some(audio_url.into());
        self
    }

    pub fn with_referer(mut self, referer: impl Into<String>) -> Self {
        self.referer = Some(referer.into());
        self
    }

    /// Full canonical path to target destination
    pub fn target_path(&self) -> PathBuf {
        self.output_dir.join(&self.file_name)
    }
}

/// The universal download function. 
/// Analyzes input links and routes to chunked streaming, HLS parsing, or ffmpeg muxing.
pub async fn download(req: DownloadRequest) -> Result<PathBuf> {
    tokio::fs::create_dir_all(&req.output_dir).await?;
    let target = req.target_path();

    let has_audio = req.audio_url.is_some();
    let is_hls = req.url.contains(".m3u8") 
        || req.audio_url.as_deref().map(|a| a.contains(".m3u8")).unwrap_or(false);

    match (is_hls, has_audio) {
        // Case 1: HLS Stream(s) -> Let FFmpeg stream and assemble segments without RAM bloat
        (true, _) => {
            hls::download_hls_stream(&req, &target).await?;
        }

        // Case 2: Direct Dual Streams (Video + Audio MP4s like Reddit DASH) -> Download chunks, then mux
        (false, true) => {
            mux::download_and_mux_direct(&req, &target).await?;
        }

        // Case 3: Standard single direct URL (Images, MP4, WebM) -> Stream chunks directly to disk
        (false, false) => {
            engine::stream_direct_to_disk(&req.url, &target, req.referer.as_deref()).await?;
        }
    }

    Ok(target)
}