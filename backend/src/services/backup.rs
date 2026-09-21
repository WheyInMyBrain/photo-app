use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use aws_config::BehaviorVersion;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use rand::rngs::SysRng;
use rand::TryRng;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{error, info, warn};

use crate::config::B2Config;
use db::backup_repo::{BackupCandidate, BackupRepo};

pub struct BackupService;

impl BackupService {
    pub fn start_scheduler(
        pool: SqlitePool,
        storage_root: PathBuf,
        b2_config: B2Config,
    ) {
        if !b2_config.is_configured() {
            info!("B2 credentials not fully configured. Backup scheduler disabled.");
            return;
        }

        let bucket = b2_config.bucket_name.clone();
        let interval_hours = b2_config.backup_interval_hours;
        let encryption_key = b2_config.encryption_key;

        tokio::spawn(async move {
            info!(
                interval_hours = interval_hours,
                bucket = %bucket,
                encrypted = encryption_key.is_some(),
                "Backup service initialized (Backblaze B2 incremental mirror)"
            );

            let s3_client = Self::create_s3_client(
                &b2_config.key_id,
                &b2_config.application_key,
                &b2_config.endpoint,
            )
            .await;

            let mut ticker = tokio::time::interval(Duration::from_secs(interval_hours * 3600));

            loop {
                ticker.tick().await;
                info!("Starting scheduled backup routine...");

                if let Err(e) = Self::run_backup_sweep(
                    &pool,
                    &storage_root,
                    &s3_client,
                    &bucket,
                    encryption_key,
                )
                .await
                {
                    error!(error = %e, "Backup cycle encountered an error");
                } else {
                    info!("Backup cycle completed successfully.");
                }
            }
        });
    }

    async fn create_s3_client(key_id: &str, app_key: &str, endpoint: &str) -> S3Client {
        let creds = Credentials::new(key_id, app_key, None, None, "b2_static");
        let config = aws_config::defaults(BehaviorVersion::latest())
            .credentials_provider(creds)
            .region(Region::new("us-east-1"))
            .endpoint_url(endpoint)
            .load()
            .await;

        S3Client::new(&config)
    }

    pub async fn run_backup_sweep(
        pool: &SqlitePool,
        storage_root: &Path,
        client: &S3Client,
        bucket: &str,
        encryption_key: Option<[u8; 32]>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let items = BackupRepo::fetch_pending_sync_items(pool).await?;
        if items.is_empty() {
            info!("No pending backup items to synchronize.");
            return Ok(());
        }

        info!(total_items = items.len(), "Beginning backup synchronization");

        for item in items {
            if item.is_deleted {
                Self::handle_deletion(pool, client, bucket, &item, encryption_key.is_some()).await;
            } else {
                Self::handle_upload(pool, storage_root, client, bucket, &item, encryption_key).await;
            }
        }

        Ok(())
    }

    async fn handle_upload(
        pool: &SqlitePool,
        storage_root: &Path,
        client: &S3Client,
        bucket: &str,
        item: &BackupCandidate,
        encryption_key: Option<[u8; 32]>,
    ) {
        let local_file_path = storage_root.join(&item.disk_rel_path);

        if !local_file_path.is_file() {
            warn!(
                asset_id = %item.asset_id,
                path = %local_file_path.display(),
                "Local file missing from disk, skipping backup"
            );
            return;
        }

        let (body_stream, remote_path) = match encryption_key {
            Some(key_bytes) => {
                let plain_data = match tokio::fs::read(&local_file_path).await {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Failed reading {:?} for encryption: {}", local_file_path, e);
                        return;
                    }
                };

                let cipher = match Aes256Gcm::new_from_slice(&key_bytes) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to initialize AES-256-GCM cipher: {:?}", e);
                        return;
                    }
                };

                let mut nonce_bytes = [0u8; 12];
                SysRng.try_fill_bytes(&mut nonce_bytes).expect("Failed to get system entropy");
                let nonce = Nonce::from(nonce_bytes);

                let ciphertext = match cipher.encrypt(&nonce, plain_data.as_ref()) {
                    Ok(ct) => ct,
                    Err(e) => {
                        error!("AES-256-GCM encryption failed for {}: {:?}", item.asset_id, e);
                        return;
                    }
                };

                let mut payload = Vec::with_capacity(12 + ciphertext.len());
                payload.extend_from_slice(&nonce_bytes);
                payload.extend_from_slice(&ciphertext);

                let enc_remote_path = format!("{}.enc", item.suggested_remote_path);
                (ByteStream::from(payload), enc_remote_path)
            }
            None => {
                let byte_stream = match ByteStream::from_path(&local_file_path).await {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed streaming {:?}: {}", local_file_path, e);
                        return;
                    }
                };
                (byte_stream, item.suggested_remote_path.clone())
            }
        };

        let res = client
            .put_object()
            .bucket(bucket)
            .key(&remote_path)
            .body(body_stream)
            .send()
            .await;

        match res {
            Ok(_) => {
                let _ = BackupRepo::mark_synced(
                    pool,
                    &item.asset_id,
                    &item.user_id,
                    &remote_path,
                    &item.sha256,
                )
                .await;
                info!(
                    asset_id = %item.asset_id,
                    remote = %remote_path,
                    encrypted = encryption_key.is_some(),
                    "Backed up asset to B2"
                );
            }
            Err(e) => {
                error!(
                    asset_id = %item.asset_id,
                    error = %e,
                    "Failed to upload asset to B2"
                );
            }
        }
    }

    async fn handle_deletion(
        pool: &SqlitePool,
        client: &S3Client,
        bucket: &str,
        item: &BackupCandidate,
        is_encrypted: bool,
    ) {
        // suggested_remote_path from the query is already the stored remote_path
        let remote_key = if is_encrypted && !item.suggested_remote_path.ends_with(".enc") {
            format!("{}.enc", item.suggested_remote_path)
        } else {
            item.suggested_remote_path.clone()
        };

        let _ = client
            .delete_object()
            .bucket(bucket)
            .key(&remote_key)
            .send()
            .await;

        let _ = BackupRepo::remove_backup_record(pool, &item.asset_id).await;
        info!(
            asset_id = %item.asset_id,
            remote = %remote_key,
            "Purged hard-deleted asset from B2 backup"
        );
    }
}