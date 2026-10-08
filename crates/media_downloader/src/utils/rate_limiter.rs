// photo-app/crates/media_downloader/src/utils/rate_limiter.rs

use rand::RngExt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub struct StealthRateLimiter {
    request_counter: Arc<AtomicU32>,
    gate: Arc<Mutex<()>>,
}

impl Default for StealthRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl StealthRateLimiter {
    pub fn new() -> Self {
        Self {
            request_counter: Arc::new(AtomicU32::new(0)),
            gate: Arc::new(Mutex::new(())),
        }
    }

    /// Natural delay before hitting a new external link / post.
    /// Emulates someone finding a URL, opening a new tab, and waiting for it to render.
    pub async fn wait_for_new_link(&self) {
        let _guard = self.gate.lock().await;
        let count = self.request_counter.fetch_add(1, Ordering::SeqCst) + 1;

        // Long pause every 5-7 links (user reading content or stepping away)
        if count % 6 == 0 {
            let break_secs: f64 = rand::rng().random_range(16.0..32.0);
            info!(
                completed_items = count,
                break_duration = format!("{:.2}s", break_secs),
                "Simulating human reading intermission before next item"
            );
            tokio::time::sleep(Duration::from_secs_f64(break_secs)).await;
            return;
        }

        // Varied link navigation delay: 4.5s to 9.5s
        let delay_secs: f64 = rand::rng().random_range(4.5..9.5);
        tokio::time::sleep(Duration::from_secs_f64(delay_secs)).await;
    }

    /// Intermission between resolving metadata and starting actual media file downloads.
    pub async fn wait_before_download(&self) {
        let _guard = self.gate.lock().await;
        let pause_secs: f64 = rand::rng().random_range(1.8..4.2);
        tokio::time::sleep(Duration::from_secs_f64(pause_secs)).await;
    }

    /// Pause between pagination pages (for carousels or infinite scroll feeds)
    pub async fn wait_for_next_page(&self) {
        let _guard = self.gate.lock().await;
        let count = self.request_counter.fetch_add(1, Ordering::SeqCst) + 1;

        if count % 8 == 0 {
            let break_secs: f64 = rand::rng().random_range(14.0..28.0);
            info!(
                page_count = count,
                break_duration = format!("{:.2}s", break_secs),
                "Pacing threshold reached. Taking micro-break."
            );
            tokio::time::sleep(Duration::from_secs_f64(break_secs)).await;
            return;
        }

        let jitter_secs: f64 = rand::rng().random_range(2.8..6.2);
        tokio::time::sleep(Duration::from_secs_f64(jitter_secs)).await;
    }

    /// Shorter jitter for ancillary calls (e.g. location, author info)
    pub async fn wait_lightweight(&self) {
        let _guard = self.gate.lock().await;
        let jitter_secs: f64 = rand::rng().random_range(1.0..2.5);
        tokio::time::sleep(Duration::from_secs_f64(jitter_secs)).await;
    }

    /// Progressive backoff triggered on temporary network retries.
    pub async fn handle_backoff(&self, attempt: u32) {
        let base_delay = 30.0;
        let factor = 2_f64.powi(attempt.min(4) as i32);
        let jitter: f64 = rand::rng().random_range(3.0..10.0);
        let total_secs = (base_delay * factor) + jitter;

        warn!(
            attempt,
            sleep_duration_secs = format!("{:.1}", total_secs),
            "Entering protective retry backoff"
        );

        tokio::time::sleep(Duration::from_secs_f64(total_secs)).await;
    }

    /// Evaluates errors to calculate recommended backoff time.
    /// Returns `(is_rate_limit, backoff_seconds)`.
    pub fn calculate_backoff(err_msg: &str, attempt: i32) -> (bool, i64) {
        let lower = err_msg.to_lowercase();
        let is_rate_limit = lower.contains("429")
            || lower.contains("feedback_required")
            || lower.contains("checkpoint")
            || lower.contains("rate limit");

        if is_rate_limit {
            // Exponential progression: 180s, 360s, 720s with heavy randomization
            let factor = 2_i64.pow((attempt.max(1) - 1).min(3) as u32);
            let base = 180 * factor;
            let jitter: i64 = rand::rng().random_range(20..80);
            (true, base + jitter)
        } else {
            // Transient network failure: 35s to 70s
            let jitter: i64 = rand::rng().random_range(35..70);
            (false, jitter)
        }
    }
}