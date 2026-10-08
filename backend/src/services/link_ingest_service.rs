// photo-app/backend/src/services/link_ingest_service.rs

use std::time::Duration;
use media_downloader::utils::rate_limiter::StealthRateLimiter;
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
    /// Deduplicates active URLs so rapid submissions do not trigger duplicate processing.
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

    /// Background worker loop that pulls links sequentially and applies
    /// StealthRateLimiter behavior.
    pub async fn run_worker_loop(state: AppState) {
        info!("Background Link Ingest Worker started (Strict Concurrency = 1 with StealthRateLimiter)");

        let limiter = StealthRateLimiter::new();

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
                    // Queue is idle: pause before re-checking
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

            // 2. Pre-scrape pacing: simulates natural user delay before requesting metadata
            limiter.wait_for_new_link().await;

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

                    // 4. Brief pause between metadata parsing and downloading media files
                    limiter.wait_before_download().await;

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

                            let (is_rate_limit, backoff_secs) =
                                StealthRateLimiter::calculate_backoff(&err_msg, item.attempts);

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

                    let (is_rate_limit, backoff_secs) =
                        StealthRateLimiter::calculate_backoff(&err_msg, item.attempts);

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

            // 6. Natural inter-post pause before picking the next link
            limiter.wait_lightweight().await;
        }
    }
}