// src/downloader/stream.rs

use super::progress::{ProgressCallback, ProgressTracker};
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, REFERER, USER_AGENT};
use reqwest::Client;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

const DOWNLOADER_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:155.0) Gecko/20100101 Firefox/155.0";

/// Streams a file directly from a URL to a specified file path
pub async fn download_file_stream<P: AsRef<Path>>(
    url: &str,
    destination: P,
    referer: Option<&str>,
    callback: Option<ProgressCallback>,
) -> Result<u64> {
    let client = Client::builder().build()?;

    let mut req = client.get(url).header(USER_AGENT, DOWNLOADER_UA);

    if let Some(ref_url) = referer {
        req = req.header(REFERER, ref_url);
    }

    let resp = req.send().await.context("Failed to send download request")?;

    if !resp.status().is_success() {
        bail!("Server rejected download request with HTTP {}", resp.status());
    }

    let total_bytes = resp
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    // Ensure parent directories exist
    if let Some(parent) = destination.as_ref().parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut file = File::create(&destination)
        .await
        .with_context(|| format!("Failed to create destination file: {:?}", destination.as_ref()))?;

    let mut stream = resp.bytes_stream();
    let mut tracker = ProgressTracker::new(total_bytes, callback);

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.context("Error occurred while reading stream chunk")?;
        file.write_all(&chunk).await?;
        tracker.update(chunk.len());
    }

    file.flush().await?;
    tracker.finish();

    let meta = tokio::fs::metadata(&destination).await?;
    Ok(meta.len())
}