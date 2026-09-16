use sqlx::SqlitePool;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info};
use db::asset_repo::AssetRepo;

pub struct TrashPurgerService;

impl TrashPurgerService {
    pub fn start(pool: SqlitePool, storage_root: PathBuf) {
        tokio::spawn(async move {
            info!("Trash auto-purge background service initialized (30-day retention window)");
            let mut interval = tokio::time::interval(Duration::from_secs(24 * 3600));

            loop {
                interval.tick().await;

                match AssetRepo::fetch_and_purge_expired_trash(&pool, &storage_root).await {
                    Ok(count) if count > 0 => {
                        info!("Auto-purge completed: permanently purged {} expired assets", count);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Auto-purge service database query failed: {}", e);
                    }
                }
            }
        });
    }
}