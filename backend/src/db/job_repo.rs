use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use crate::domain::job_repo::DbJob;

pub struct JobRepo;

impl JobRepo {
    pub async fn enqueue(pool: &SqlitePool, job: &DbJob) -> Result<(), sqlx::Error> {
        let is_private_int: i64 = if job.is_private { 1 } else { 0 };

        sqlx::query(
            r#"
            INSERT INTO processing_jobs (
                id, asset_id, file_name, rel_path, folder_path,
                disk_path, sha256, file_size_bytes, is_private, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')
            "#,
        )
        .bind(&job.id)
        .bind(&job.asset_id)
        .bind(&job.file_name)
        .bind(&job.rel_path)
        .bind(&job.folder_path)
        .bind(job.disk_path.to_string_lossy().to_string())
        .bind(&job.sha256)
        .bind(job.file_size_bytes)
        .bind(is_private_int)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn reset_interrupted(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
        let res = sqlx::query(
            "UPDATE processing_jobs SET status = 'pending' WHERE status = 'processing'",
        )
        .execute(pool)
        .await?;

        Ok(res.rows_affected())
    }

    pub async fn fetch_next_job(pool: &SqlitePool) -> Result<Option<DbJob>, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            r#"
            SELECT id, asset_id, file_name, rel_path, folder_path,
                   disk_path, sha256, file_size_bytes, is_private
            FROM processing_jobs
            WHERE status = 'pending' AND attempts < 3
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(r) = row {
            let job = DbJob {
                id: r.get("id"),
                asset_id: r.get("asset_id"),
                file_name: r.get("file_name"),
                rel_path: r.get("rel_path"),
                folder_path: r.get("folder_path"),
                disk_path: PathBuf::from(r.get::<String, _>("disk_path")),
                sha256: r.get("sha256"),
                file_size_bytes: r.get("file_size_bytes"),
                is_private: r.get::<i64, _>("is_private") == 1,
            };

            sqlx::query(
                "UPDATE processing_jobs SET status = 'processing', attempts = attempts + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(&job.id)
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