// photo-app/crates/db/src/upload_repo.rs

use sqlx::{PgPool, Result};
use uuid::Uuid;

use crate::domain::job_repo::DbJob;

pub struct UploadRepo;

impl UploadRepo {
    /// Generate a unique batch tracking ID for the client session
    pub fn new_batch_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Insert directly into `processing_jobs`.
    /// If `batch_id` is supplied, insert with status = 'staged'
    /// so workers ignore it while uploads are in flight.
    pub async fn enqueue_staged_job(
        pool: &PgPool,
        job: &DbJob,
        batch_id: Option<&str>,
    ) -> Result<()> {
        let status = if batch_id.is_some() { "staged" } else { "pending" };

        let mut payload_value = match &job.payload {
            Some(p) => serde_json::to_value(p).unwrap_or_else(|_| serde_json::json!({})),
            None => serde_json::json!({}),
        };

        if let Some(bid) = batch_id {
            if let Some(obj) = payload_value.as_object_mut() {
                obj.insert("batch_id".to_string(), serde_json::Value::String(bid.to_string()));
            }
        }

        sqlx::query(
            r#"
            INSERT INTO processing_jobs (
                id, user_id, asset_id, file_name, rel_path, folder_path,
                disk_path, sha256, file_size_bytes, status, current_stage, payload,
                assemble_done, thumb_done, ai_faces_done, ai_clip_done, ai_tags_done, ai_poses_done
            )
            VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8, $9, $10, $11, $12,
                $13, $14, $15, $16, $17, $18
            )
            ON CONFLICT (asset_id) DO UPDATE SET
                disk_path = EXCLUDED.disk_path,
                payload = EXCLUDED.payload,
                status = EXCLUDED.status,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(job.id)
        .bind(job.user_id)
        .bind(job.asset_id)
        .bind(&job.file_name)
        .bind(&job.rel_path)
        .bind(&job.folder_path)
        .bind(job.disk_path.to_string_lossy().as_ref())
        .bind(&job.sha256)
        .bind(job.file_size_bytes)
        .bind(status)
        .bind(&job.current_stage)
        .bind(payload_value)
        .bind(job.assemble_done)
        .bind(job.thumb_done)
        .bind(job.ai_faces_done)
        .bind(job.ai_clip_done)
        .bind(job.ai_tags_done)
        .bind(job.ai_poses_done)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Releases all staged jobs belonging to this batch_id.
    /// Flips status from 'staged' -> 'pending' using Postgres JSONB traversal (`->>`).
    pub async fn commit_batch(
        pool: &PgPool,
        user_id: Uuid,
        batch_id: &str,
    ) -> Result<u64> {
        let res = sqlx::query(
            r#"
            UPDATE processing_jobs
            SET status = 'pending',
                updated_at = CURRENT_TIMESTAMP
            WHERE user_id = $1
              AND status = 'staged'
              AND payload->>'batch_id' = $2
            "#,
        )
        .bind(user_id)
        .bind(batch_id)
        .execute(pool)
        .await?;

        Ok(res.rows_affected())
    }
}