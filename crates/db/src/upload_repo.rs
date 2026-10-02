use sqlx::{Result, SqlitePool};
use uuid::Uuid;

use crate::domain::DbJob;

pub struct UploadRepo;

impl UploadRepo {
    /// Generate a unique batch tracking ID for the client session
    pub fn new_batch_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Insert directly into your existing `processing_jobs` table.
    /// If `batch_id` is supplied, insert with status = 'staged'
    /// so Worker 0 completely ignores it while uploads are in flight.
    pub async fn enqueue_staged_job(
        pool: &SqlitePool,
        job: &DbJob,
        batch_id: Option<&str>,
    ) -> Result<()> {
        let status = if batch_id.is_some() { "staged" } else { "pending" };

        // Embed batch_id into the existing payload JSON
        let mut payload_value = match &job.payload {
            Some(p) => serde_json::to_value(p).unwrap_or_else(|_| serde_json::json!({})),
            None => serde_json::json!({}),
        };

        if let Some(bid) = batch_id {
            if let Some(obj) = payload_value.as_object_mut() {
                obj.insert("batch_id".to_string(), serde_json::Value::String(bid.to_string()));
            }
        }

        let payload_json = serde_json::to_string(&payload_value).ok();

        sqlx::query(
            r#"
            INSERT INTO processing_jobs (
                id, user_id, asset_id, file_name, rel_path, folder_path,
                disk_path, sha256, file_size_bytes, job_type, status, payload
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
        )
        .bind(&job.id)
        .bind(&job.user_id)
        .bind(&job.asset_id)
        .bind(&job.file_name)
        .bind(&job.rel_path)
        .bind(&job.folder_path)
        .bind(job.disk_path.to_string_lossy().to_string())
        .bind(&job.sha256)
        .bind(job.file_size_bytes)
        .bind(&job.job_type)
        .bind(status)
        .bind(payload_json)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Releases all staged jobs belonging to this batch_id.
    /// Flips status from 'staged' -> 'pending' using SQLite's native JSON path query.
    pub async fn commit_batch(
        pool: &SqlitePool,
        user_id: &str,
        batch_id: &str,
    ) -> Result<u64> {
        let res = sqlx::query(
            r#"
            UPDATE processing_jobs
            SET status = 'pending',
                updated_at = CURRENT_TIMESTAMP
            WHERE user_id = ?1
              AND status = 'staged'
              AND json_extract(payload, '$.batch_id') = ?2
            "#,
        )
        .bind(user_id)
        .bind(batch_id)
        .execute(pool)
        .await?;

        Ok(res.rows_affected())
    }
}