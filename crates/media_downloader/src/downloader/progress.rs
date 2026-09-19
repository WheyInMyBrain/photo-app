// src/downloader/progress.rs

use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_sec: f64,
    pub percentage: Option<f32>,
}

pub type ProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

pub struct ProgressTracker {
    downloaded: u64,
    total: Option<u64>,
    start_time: Instant,
    last_update: Instant,
    last_downloaded: u64,
    current_speed: f64,
    callback: Option<ProgressCallback>,
}

impl ProgressTracker {
    pub fn new(total: Option<u64>, callback: Option<ProgressCallback>) -> Self {
        let now = Instant::now();
        Self {
            downloaded: 0,
            total,
            start_time: now,
            last_update: now,
            last_downloaded: 0,
            current_speed: 0.0,
            callback,
        }
    }

    pub fn update(&mut self, chunk_len: usize) {
        self.downloaded += chunk_len as u64;
        let now = Instant::now();
        let elapsed_since_last = now.duration_since(self.last_update).as_secs_f64();

        // Calculate moving speed every 0.5s to prevent jitter
        if elapsed_since_last >= 0.5 {
            let bytes_delta = self.downloaded - self.last_downloaded;
            self.current_speed = bytes_delta as f64 / elapsed_since_last;
            self.last_update = now;
            self.last_downloaded = self.downloaded;

            if let Some(ref cb) = self.callback {
                let pct = self
                    .total
                    .map(|t| (self.downloaded as f32 / t as f32) * 100.0);
                cb(DownloadProgress {
                    downloaded_bytes: self.downloaded,
                    total_bytes: self.total,
                    speed_bytes_per_sec: self.current_speed,
                    percentage: pct,
                });
            }
        }
    }

    pub fn finish(&self) {
        if let Some(ref cb) = self.callback {
            let elapsed = self.start_time.elapsed().as_secs_f64();
            let avg_speed = if elapsed > 0.0 {
                self.downloaded as f64 / elapsed
            } else {
                0.0
            };

            cb(DownloadProgress {
                downloaded_bytes: self.downloaded,
                total_bytes: self.total.or(Some(self.downloaded)),
                speed_bytes_per_sec: avg_speed,
                percentage: Some(100.0),
            });
        }
    }
}