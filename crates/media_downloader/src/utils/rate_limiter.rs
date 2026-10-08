// src/utils/rate_limiter.rs

use rand::RngExt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub struct StealthRateLimiter {
    request_counter: Arc<AtomicU32>,
    /// Global lock to serialize requests across threads if desired
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

    /// Natural pause between sequential requests (e.g. pagination)
    pub async fn wait_for_next_page(&self) {
        let _guard = self.gate.lock().await;

        let count = self.request_counter.fetch_add(1, Ordering::SeqCst) + 1;

        // Micro-break: Every 6-9 requests, take a longer pause (simulating a human reading or pausing)
        if count % 8 == 0 {
            let break_secs: f64 = rand::rng().random_range(12.0..24.0);
            info!(
                request_count = count,
                break_seconds = format!("{:.2}", break_secs),
                "Pacing threshold reached. Taking human-like micro-break."
            );
            tokio::time::sleep(Duration::from_secs_f64(break_secs)).await;
            return;
        }

        // Standard jitter: randomized between 2.2s and 5.5s
        let jitter_secs: f64 = rand::rng().random_range(2.2..5.5);
        tokio::time::sleep(Duration::from_secs_f64(jitter_secs)).await;
    }

    /// Shorter jitter for lightweight calls (e.g., location info or item details)
    pub async fn wait_lightweight(&self) {
        let _guard = self.gate.lock().await;
        let jitter_secs: f64 = rand::rng().random_range(0.8..2.0);
        tokio::time::sleep(Duration::from_secs_f64(jitter_secs)).await;
    }

    /// Exponential backoff triggered when Instagram signals rate limiting (429 or feedback_required)
    pub async fn handle_backoff(&self, attempt: u32) {
        let base_delay = 30.0;
        let factor = 2_f64.powi(attempt.min(4) as i32);
        let jitter: f64 = rand::rng().random_range(3.0..10.0);
        let total_secs = (base_delay * factor) + jitter;

        warn!(
            attempt,
            sleep_duration_secs = format!("{:.1}", total_secs),
            "Rate limit detected. Entering protective cooldown."
        );

        tokio::time::sleep(Duration::from_secs_f64(total_secs)).await;
    }
}