// photo-app/crates/db/src/job_repo.rs

use crate::domain::job_repo::{DbJob, JobPayload};
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;

pub struct JobRepo;

impl JobRepo {
    /// Inserts a new job. Status can be 'pending' (ready to run) or 'staged' (waiting for batch finish).
    pub async fn enqueue_with_status(
        pool: &SqlitePool,
        job: &DbJob,
        status: &str,
        batch_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let job_type = if job.job_type.trim().is_empty() {
            "assemble"
        } else {
            &job.job_type
        };

        // Inject batch_id into payload JSON if present
        let mut payload_val = match &job.payload {
            Some(p) => serde_json::to_value(p).unwrap_or_else(|_| serde_json::json!({})),
            None => serde_json::json!({}),
        };

        if let Some(bid) = batch_id {
            if let Some(obj) = payload_val.as_object_mut() {
                obj.insert("batch_id".to_string(), serde_json::Value::String(bid.to_string()));
            }
        }

        let payload_json = serde_json::to_string(&payload_val).ok();

        sqlx::query(
            r#"
            INSERT INTO processing_jobs (
                id, user_id, asset_id, file_name, rel_path, folder_path,
                disk_path, sha256, file_size_bytes, job_type, payload, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(job_type)
        .bind(payload_json)
        .bind(status)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Default enqueue with 'pending' status
    pub async fn enqueue(pool: &SqlitePool, job: &DbJob) -> Result<(), sqlx::Error> {
        Self::enqueue_with_status(pool, job, "pending", None).await
    }

    /// Atomic batch release: Promotes 'staged' jobs of a given batch_id to 'pending'
    /// and returns the list of job IDs so they can be pushed into the mpsc channel.
    pub async fn commit_staged_batch(
        pool: &SqlitePool,
        user_id: &str,
        batch_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let mut tx = pool.begin().await?;

        // 1. Fetch IDs of staged jobs belonging to this batch
        let rows = sqlx::query(
            r#"
            SELECT id FROM processing_jobs
            WHERE user_id = ?
              AND status = 'staged'
              AND json_extract(payload, '$.batch_id') = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(user_id)
        .bind(batch_id)
        .fetch_all(&mut *tx)
        .await?;

        let ids: Vec<String> = rows.into_iter().map(|r| r.get("id")).collect();

        // 2. Mark them as pending
        if !ids.is_empty() {
            sqlx::query(
                r#"
                UPDATE processing_jobs
                SET status = 'pending', updated_at = CURRENT_TIMESTAMP
                WHERE user_id = ?
                  AND status = 'staged'
                  AND json_extract(payload, '$.batch_id') = ?
                "#,
            )
            .bind(user_id)
            .bind(batch_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(ids)
    }

    /// Startup Recovery: Resets 'processing' jobs to 'pending' and returns
    /// all uncompleted jobs grouped by job_type so mpsc channels can be refilled.
    pub async fn recover_uncompleted_jobs(
        pool: &SqlitePool,
        job_type: &str,
    ) -> Result<Vec<DbJob>, sqlx::Error> {
        // Reset any zombie jobs from previous crash
        sqlx::query(
            "UPDATE processing_jobs SET status = 'pending' WHERE status = 'processing' AND job_type = ?",
        )
        .bind(job_type)
        .execute(pool)
        .await?;

        let rows = sqlx::query(
            r#"
            SELECT id, user_id, asset_id, file_name, rel_path, folder_path,
                   disk_path, sha256, file_size_bytes, job_type, payload
            FROM processing_jobs
            WHERE status = 'pending' AND job_type = ? AND attempts < 3
            ORDER BY created_at ASC
            "#,
        )
        .bind(job_type)
        .fetch_all(pool)
        .await?;

        let mut jobs = Vec::with_capacity(rows.len());
        for r in rows {
            let payload_raw: Option<String> = r.get("payload");
            let payload: Option<JobPayload> = payload_raw
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok());

            jobs.push(DbJob {
                id: r.get("id"),
                user_id: r.get("user_id"),
                asset_id: r.get("asset_id"),
                file_name: r.get("file_name"),
                rel_path: r.get("rel_path"),
                folder_path: r.get("folder_path"),
                disk_path: PathBuf::from(r.get::<String, _>("disk_path")),
                sha256: r.get("sha256"),
                job_type: r.get("job_type"),
                file_size_bytes: r.get("file_size_bytes"),
                payload,
            });
        }

        Ok(jobs)
    }

    /// Fetch a single job by ID and atomically transition status to 'processing'
    pub async fn acquire_job_by_id(
        pool: &SqlitePool,
        job_id: &str,
    ) -> Result<Option<DbJob>, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            r#"
            SELECT id, user_id, asset_id, file_name, rel_path, folder_path,
                   disk_path, sha256, file_size_bytes, job_type, payload
            FROM processing_jobs
            WHERE id = ? AND attempts < 3
            LIMIT 1
            "#,
        )
        .bind(job_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(r) = row {
            let payload_raw: Option<String> = r.get("payload");
            let payload: Option<JobPayload> = payload_raw
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok());

            let job = DbJob {
                id: r.get("id"),
                user_id: r.get("user_id"),
                asset_id: r.get("asset_id"),
                file_name: r.get("file_name"),
                rel_path: r.get("rel_path"),
                folder_path: r.get("folder_path"),
                disk_path: PathBuf::from(r.get::<String, _>("disk_path")),
                sha256: r.get("sha256"),
                job_type: r.get("job_type"),
                file_size_bytes: r.get("file_size_bytes"),
                payload,
            };

            sqlx::query(
                "UPDATE processing_jobs SET status = 'processing', attempts = attempts + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(job_id)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            Ok(Some(job))
        } else {
            Ok(None)
        }
    }

    pub async fn mark_completed(pool: &SqlitePool, job_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE processing_jobs SET status = 'completed', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(job_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn mark_failed(
        pool: &SqlitePool,
        job_id: &str,
        error: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE processing_jobs SET status = 'failed', last_error = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(error)
        .bind(job_id)
        .execute(pool)
        .await?;

        Ok(())
    }
}