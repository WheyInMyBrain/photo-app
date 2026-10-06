// photo-app/backend/src/services/trash_purger.rs

use async_recursion::async_recursion;
use db::album_repo::AlbumRepo;
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

            // Retention period for abandoned temp chunks/files (e.g., 24 hours)
            let temp_retention = Duration::from_secs(24 * 3600);

            loop {
                ticker.tick().await;

                // 1. Purge expired trash media from SQLite and disk
                match AssetRepo::fetch_and_purge_expired_trash(&pool, &storage_root).await {
                    Ok(count) if count > 0 => {
                        info!("Auto-purge completed: permanently purged {} expired assets", count);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Auto-purge service database query failed: {}", e);
                    }
                }

                // 2. Clean up trashed albums that are now empty or older than 30 days
                match AlbumRepo::cleanup_all_expired_and_empty_trashed_albums(&pool).await {
                    Ok(count) if count > 0 => {
                        info!("Auto-purge completed: permanently removed {} empty/expired trashed albums", count);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Auto-purge failed to clean trashed albums: {}", e);
                    }
                }

                // 3. Clean orphaned temporary uploads & chunk directories
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

                // 4. Clean orphaned assembly temporary files (*.tmp) inside library originals
                let users_dir = storage_root.join("users");
                if let Err(e) = Self::cleanup_stale_library_tmp_files(&users_dir, temp_retention).await {
                    warn!(
                        path = %users_dir.display(),
                        error = %e,
                        "Failed to sweep stale assembly .tmp files in users directory"
                    );
                }
            }
        });
    }

    /// Recursively sweeps files and empty directories older than `max_age`
    #[async_recursion]
    async fn cleanup_stale_temp_entries(dir: &Path, max_age: Duration) -> std::io::Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        let mut reader = match tokio::fs::read_dir(dir).await {
            Ok(r) => r,
            Err(e) => return Err(e),
        };

        let now = SystemTime::now();

        while let Ok(Some(entry)) = reader.next_entry().await {
            let path = entry.path();
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };

            if meta.is_dir() {
                // Recursively clean children first
                let _ = Self::cleanup_stale_temp_entries(&path, max_age).await;

                // Prune directory if it is old enough and now empty
                if let Ok(modified) = meta.modified() {
                    if let Ok(elapsed) = now.duration_since(modified) {
                        if elapsed > max_age {
                            // remove_dir only succeeds if the folder is empty
                            let _ = tokio::fs::remove_dir(&path).await;
                        }
                    }
                }
            } else {
                // Delete stale temporary file
                if let Ok(modified) = meta.modified() {
                    if let Ok(elapsed) = now.duration_since(modified) {
                        if elapsed > max_age {
                            let _ = tokio::fs::remove_file(&path).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Recursively checks users/*/originals for abandoned `.tmp` assembly files
    #[async_recursion]
    async fn cleanup_stale_library_tmp_files(dir: &Path, max_age: Duration) -> std::io::Result<()> {
        if !dir.exists() {
            return Ok(());
        }

        let mut reader = match tokio::fs::read_dir(dir).await {
            Ok(r) => r,
            Err(e) => return Err(e),
        };

        let now = SystemTime::now();

        while let Ok(Some(entry)) = reader.next_entry().await {
            let path = entry.path();
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };

            if meta.is_dir() {
                let _ = Self::cleanup_stale_library_tmp_files(&path, max_age).await;
            } else if path.extension().and_then(|s| s.to_str()) == Some("tmp") {
                if let Ok(modified) = meta.modified() {
                    if let Ok(elapsed) = now.duration_since(modified) {
                        if elapsed > max_age {
                            let _ = tokio::fs::remove_file(&path).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}