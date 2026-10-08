// photo-app/backend/src/services/link_ingest_service.rs

use std::time::Duration;
use rand::RngExt;
use sqlx::PgPool;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::middleware::auth::AuthUser;
use crate::routes::upload::downloader::{execute_item_downloads, resolve_or_scrape_manifest};
use crate::AppState;
use db::scrapes_repo::ScrapesRepo;

pub struct LinkIngestService;

impl LinkIngestService {
    /// Enqueues a target URL into the database queue.
    /// Deduplicates active URLs so rapid successive hits do not trigger concurrent scrapes.
    pub async fn enqueue(
        pool: &PgPool,
        user_id: Uuid,
        target_url: &str,
        requested_folder: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        let platform = if target_url.contains("instagram.com") || target_url.contains("instagr.am") {
            "instagram"
        } else {
            "generic"
        };

        ScrapesRepo::enqueue_link(pool, user_id, target_url, platform, requested_folder).await
    }

    /// Background runner that continuously processes links strictly one by one.
    /// Applies randomized human delays and handles error backoff.
    pub async fn run_worker_loop(state: AppState) {
        info!("Background Link Ingest Worker started (Strict Sequential Concurrency = 1)");

        loop {
            // 1. Atomically claim the next pending link using SKIP LOCKED
            let item_opt = match ScrapesRepo::claim_next_pending_link(&state.db).await {
                Ok(item) => item,
                Err(e) => {
                    error!(error = %e, "Database error claiming link from queue; sleeping 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

            let item = match item_opt {
                Some(it) => it,
                None => {
                    // Queue is idle: sleep briefly before polling again
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    continue;
                }
            };

            info!(
                url = %item.target_url,
                queue_id = %item.id,
                attempt = item.attempts,
                "Picked up link from ingest queue"
            );

            // 2. Pre-scrape Human Jitter: Wait 3.5s - 7.5s before making network calls
            let pre_jitter: f64 = rand::rng().random_range(3.5..7.5);
            info!(
                delay_secs = format!("{:.2}", pre_jitter),
                "Simulating natural human pause before scraping manifest"
            );
            tokio::time::sleep(Duration::from_secs_f64(pre_jitter)).await;

            let auth_user = AuthUser {
                id: item.user_id,
                username: "queued_ingest_worker".to_string(),
            };

            // 3. Resolve & scrape media manifest
            match resolve_or_scrape_manifest(&state, &auth_user, &item.target_url).await {
                Ok(manifest) => {
                    let target_folder = item
                        .requested_folder
                        .clone()
                        .unwrap_or(manifest.suggested_folder);

                    // 4. Intermediate Jitter: Short pause before downloading binary assets
                    let cdn_jitter: f64 = rand::rng().random_range(1.5..3.5);
                    tokio::time::sleep(Duration::from_secs_f64(cdn_jitter)).await;

                    // 5. Download media files sequentially
                    match execute_item_downloads(
                        &state,
                        &auth_user,
                        manifest.items,
                        &target_folder,
                        &manifest.platform,
                    )
                    .await
                    {
                        Ok(_) => {
                            if let Err(e) = ScrapesRepo::mark_link_completed(&state.db, item.id).await {
                                error!(queue_id = %item.id, error = %e, "Failed to mark link completed");
                            } else {
                                info!(url = %item.target_url, "Successfully processed and stored media");
                            }
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            error!(url = %item.target_url, error = %err_msg, "Item download execution failed");
                            
                            let (is_rate_limit, backoff_secs) = Self::evaluate_error(&err_msg);
                            let _ = ScrapesRepo::mark_link_retry_backoff(
                                &state.db,
                                item.id,
                                &err_msg,
                                backoff_secs,
                                is_rate_limit,
                            )
                            .await;
                        }
                    }
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    warn!(url = %item.target_url, error = %err_msg, "Manifest resolution failed");

                    let (is_rate_limit, backoff_secs) = Self::evaluate_error(&err_msg);
                    if is_rate_limit {
                        warn!(
                            backoff_seconds = backoff_secs,
                            "External rate limit encountered. Scheduling backoff delay."
                        );
                    }

                    let _ = ScrapesRepo::mark_link_retry_backoff(
                        &state.db,
                        item.id,
                        &err_msg,
                        backoff_secs,
                        is_rate_limit,
                    )
                    .await;
                }
            }

            // 6. Post-Job Cooldown: Natural pause of 6.0s - 14.0s before considering the next link
            let post_cooldown: f64 = rand::rng().random_range(6.0..14.0);
            info!(
                cooldown_secs = format!("{:.2}", post_cooldown),
                "Resting before processing the next item in queue"
            );
            tokio::time::sleep(Duration::from_secs_f64(post_cooldown)).await;
        }
    }

    /// Evaluates errors to determine whether to trigger a defensive rate-limit backoff
    fn evaluate_error(err: &str) -> (bool, i64) {
        let lower = err.to_lowercase();
        if lower.contains("429")
            || lower.contains("feedback_required")
            || lower.contains("checkpoint")
            || lower.contains("rate limit")
        {
            // Back off 3 to 6 minutes for external rate limits
            let jitter: i64 = rand::rng().random_range(180..360);
            (true, jitter)
        } else {
            // Standard network failure retry: 30 to 60 seconds
            let jitter: i64 = rand::rng().random_range(30..60);
            (false, jitter)
        }
    }
}