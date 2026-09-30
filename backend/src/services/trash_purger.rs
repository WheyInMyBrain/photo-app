// photo-app/backend/src/services/trash_purger.rs

use db::asset_repo::AssetRepo;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::time::interval;
use tracing::{error, info, warn};

pub struct TrashPurgerService;

impl TrashPurgerService {
    pub fn start(pool: SqlitePool, storage_root: PathBuf) {
        tokio::spawn(async move {
            info!("Storage background maintenance service initialized (Trash purge + Temp cleaner)");
            // Runs once every 24 hours
            let mut ticker = interval(Duration::from_secs(24 * 3600));

            // Retention period for abandoned temp chunks/files (e.g., 24 hours or 7 days)
            let temp_retention = Duration::from_secs(24 * 3600);

            loop {
                ticker.tick().await;

                // 1. Purge expired trash from SQLite and disk
                match AssetRepo::fetch_and_purge_expired_trash(&pool, &storage_root).await {
                    Ok(count) if count > 0 => {
                        info!("Auto-purge completed: permanently purged {} expired assets", count);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Auto-purge service database query failed: {}", e);
                    }
                }

                // 2. Clean orphaned temporary uploads / chunk directories
                let temp_dirs = [
                    storage_root.join("temp"),
                    storage_root.join("temp_chunks"),
                ];

                for dir in &temp_dirs {
                    if let Err(e) = Self::cleanup_stale_temp_entries(dir, temp_retention).await {
                        warn!(
                            path = %dir.display(),
                            error = %e,
                            "Failed to clean temporary directory"
                        );
                    }
                }
            }
        });
    }

    /// Recursively removes files and subdirectories older than `max_age` within `dir`
    async fn cleanup_stale_temp_entries(dir: &Path, max_age: Duration) -> std::io::Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        let mut reader = tokio::fs::read_dir(dir).await?;
        let now = SystemTime::now();

        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if let Ok(meta) = entry.metadata().await {
                if let Ok(modified) = meta.modified() {
                    if let Ok(elapsed) = now.duration_since(modified) {
                        if elapsed > max_age {
                            if meta.is_dir() {
                                let _ = tokio::fs::remove_dir_all(&path).await;
                            } else {
                                let _ = tokio::fs::remove_file(&path).await;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}