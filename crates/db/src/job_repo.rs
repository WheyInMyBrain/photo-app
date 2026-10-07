// photo-app/crates/db/src/job_repo.rs

use crate::domain::job_repo::{DbJob, JobPayload};
use sqlx::{PgPool, Row};
use std::path::PathBuf;
use uuid::Uuid;

pub struct JobRepo;

impl JobRepo {
    /// Inserts a new job (1 row per asset).
    pub async fn enqueue_with_status(
        pool: &PgPool,
        job: &DbJob,
        status: &str,
        batch_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let mut payload_val = match &job.payload {
            Some(p) => serde_json::to_value(p).unwrap_or_else(|_| serde_json::json!({})),
            None => serde_json::json!({}),
        };

        if let Some(bid) = batch_id {
            if let Some(obj) = payload_val.as_object_mut() {
                obj.insert("batch_id".to_string(), serde_json::Value::String(bid.to_string()));
            }
        }

        sqlx::query(
            r#"
            INSERT INTO processing_jobs (
                id, user_id, asset_id, file_name, rel_path, folder_path,
                disk_path, sha256, file_size_bytes, status, current_stage, payload,
                assemble_done, thumb_done, ai_faces_done, ai_clip_done, ai_tags_done, ai_poses_done
            ) VALUES (
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
        .bind(job.disk_path.to_string_lossy().to_string())
        .bind(&job.sha256)
        .bind(job.file_size_bytes)
        .bind(status)
        .bind(&job.current_stage)
        .bind(payload_val)
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

    pub async fn enqueue(pool: &PgPool, job: &DbJob) -> Result<(), sqlx::Error> {
        Self::enqueue_with_status(pool, job, "pending", None).await
    }

    /// Atomically releases all staged jobs of a batch to 'pending'.
    pub async fn commit_staged_batch(
        pool: &PgPool,
        user_id: Uuid,
        batch_id: &str,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            UPDATE processing_jobs
            SET status = 'pending', updated_at = CURRENT_TIMESTAMP
            WHERE user_id = $1
              AND status = 'staged'
              AND payload->>'batch_id' = $2
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(batch_id)
        .fetch_all(pool)
        .await?;

        let ids: Vec<Uuid> = rows.into_iter().map(|r| r.get("id")).collect();
        Ok(ids)
    }

    /// Stage 0 Completion: Marks assemble finished, updates sha256/rel_path/size, sets next stage to thumbnail.
    pub async fn mark_assemble_done(
        pool: &PgPool,
        job_id: Uuid,
        sha256: &str,
        rel_path: &str,
        disk_path: &PathBuf,
        file_size_bytes: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE processing_jobs
            SET assemble_done = TRUE,
                current_stage = 'thumbnail',
                sha256 = $2,
                rel_path = $3,
                disk_path = $4,
                file_size_bytes = $5,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .bind(sha256)
        .bind(rel_path)
        .bind(disk_path.to_string_lossy().to_string())
        .bind(file_size_bytes)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Stage 1 Completion: Marks thumbnail finished. If AI is disabled, completes the job; otherwise sets stage to ai_enrichment.
    pub async fn mark_thumb_done(
        pool: &PgPool,
        job_id: Uuid,
        ai_enabled: bool,
    ) -> Result<(), sqlx::Error> {
        let next_stage = if ai_enabled { "ai_enrichment" } else { "completed" };
        let next_status = if ai_enabled { "pending" } else { "completed" };

        sqlx::query(
            r#"
            UPDATE processing_jobs
            SET thumb_done = TRUE,
                current_stage = $2,
                status = $3,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .bind(next_stage)
        .bind(next_status)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Stage 2 Checkpoints: Marks granular AI components and completes job when all configured tasks are done.
    pub async fn update_ai_checkpoints(
        pool: &PgPool,
        job_id: Uuid,
        faces_done: bool,
        clip_done: bool,
        tags_done: bool,
        poses_done: bool,
        require_faces: bool,
        require_clip: bool,
        require_tags: bool,
        require_poses: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE processing_jobs
            SET ai_faces_done = CASE WHEN $1 = TRUE THEN TRUE ELSE ai_faces_done END,
                ai_clip_done  = CASE WHEN $2 = TRUE THEN TRUE ELSE ai_clip_done END,
                ai_tags_done  = CASE WHEN $3 = TRUE THEN TRUE ELSE ai_tags_done END,
                ai_poses_done = CASE WHEN $4 = TRUE THEN TRUE ELSE ai_poses_done END,
                status = CASE
                    WHEN ($5 = FALSE OR $1 = TRUE OR ai_faces_done = TRUE)
                     AND ($6 = FALSE OR $2 = TRUE OR ai_clip_done = TRUE)
                     AND ($7 = FALSE OR $3 = TRUE OR ai_tags_done = TRUE)
                     AND ($8 = FALSE OR $4 = TRUE OR ai_poses_done = TRUE)
                    THEN 'completed'
                    ELSE 'pending'
                END,
                current_stage = CASE
                    WHEN ($5 = FALSE OR $1 = TRUE OR ai_faces_done = TRUE)
                     AND ($6 = FALSE OR $2 = TRUE OR ai_clip_done = TRUE)
                     AND ($7 = FALSE OR $3 = TRUE OR ai_tags_done = TRUE)
                     AND ($8 = FALSE OR $4 = TRUE OR ai_poses_done = TRUE)
                    THEN 'completed'
                    ELSE 'ai_enrichment'
                END,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $9
            "#,
        )
        .bind(faces_done)
        .bind(clip_done)
        .bind(tags_done)
        .bind(poses_done)
        .bind(require_faces)
        .bind(require_clip)
        .bind(require_tags)
        .bind(require_poses)
        .bind(job_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Server Startup Recovery: Recovers uncompleted jobs based on stage flags rather than redundant rows.
    pub async fn recover_uncompleted_jobs(
        pool: &PgPool,
        stage: &str,
    ) -> Result<Vec<DbJob>, sqlx::Error> {
        let rows = match stage {
            "assemble" => {
                sqlx::query(
                    r#"
                    SELECT id, user_id, asset_id, file_name, rel_path, folder_path,
                           disk_path, sha256, file_size_bytes, status, current_stage, payload,
                           assemble_done, thumb_done, ai_faces_done, ai_clip_done, ai_tags_done, ai_poses_done
                    FROM processing_jobs
                    WHERE status = 'pending' AND assemble_done = FALSE AND attempts < 3
                    ORDER BY created_at ASC
                    "#,
                )
                .fetch_all(pool)
                .await?
            }
            "thumbnail" => {
                sqlx::query(
                    r#"
                    SELECT id, user_id, asset_id, file_name, rel_path, folder_path,
                           disk_path, sha256, file_size_bytes, status, current_stage, payload,
                           assemble_done, thumb_done, ai_faces_done, ai_clip_done, ai_tags_done, ai_poses_done
                    FROM processing_jobs
                    WHERE status = 'pending' AND assemble_done = TRUE AND thumb_done = FALSE AND attempts < 3
                    ORDER BY created_at ASC
                    "#,
                )
                .fetch_all(pool)
                .await?
            }
            "ai_enrichment" => {
                sqlx::query(
                    r#"
                    SELECT id, user_id, asset_id, file_name, rel_path, folder_path,
                           disk_path, sha256, file_size_bytes, status, current_stage, payload,
                           assemble_done, thumb_done, ai_faces_done, ai_clip_done, ai_tags_done, ai_poses_done
                    FROM processing_jobs
                    WHERE status = 'pending' AND thumb_done = TRUE 
                      AND (ai_faces_done = FALSE OR ai_clip_done = FALSE OR ai_tags_done = FALSE OR ai_poses_done = FALSE)
                      AND attempts < 3
                    ORDER BY created_at ASC
                    "#,
                )
                .fetch_all(pool)
                .await?
            }
            _ => Vec::new(),
        };

        let mut jobs = Vec::with_capacity(rows.len());
        for r in rows {
            let payload: Option<JobPayload> = r
                .try_get::<serde_json::Value, _>("payload")
                .ok()
                .and_then(|val| serde_json::from_value(val).ok());

            jobs.push(DbJob {
                id: r.get("id"),
                user_id: r.get("user_id"),
                asset_id: r.get("asset_id"),
                file_name: r.get("file_name"),
                rel_path: r.get("rel_path"),
                folder_path: r.get("folder_path"),
                disk_path: PathBuf::from(r.get::<String, _>("disk_path")),
                sha256: r.get("sha256"),
                file_size_bytes: r.get("file_size_bytes"),
                status: r.get("status"),
                current_stage: r.get("current_stage"),
                payload,
                assemble_done: r.get("assemble_done"),
                thumb_done: r.get("thumb_done"),
                ai_faces_done: r.get("ai_faces_done"),
                ai_clip_done: r.get("ai_clip_done"),
                ai_tags_done: r.get("ai_tags_done"),
                ai_poses_done: r.get("ai_poses_done"),
            });
        }

        Ok(jobs)
    }

    pub async fn acquire_job_by_id(
        pool: &PgPool,
        job_id: Uuid,
    ) -> Result<Option<DbJob>, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            r#"
            SELECT id, user_id, asset_id, file_name, rel_path, folder_path,
                   disk_path, sha256, file_size_bytes, status, current_stage, payload,
                   assemble_done, thumb_done, ai_faces_done, ai_clip_done, ai_tags_done, ai_poses_done
            FROM processing_jobs
            WHERE id = $1 AND attempts < 3
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(job_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(r) = row {
            let payload: Option<JobPayload> = r
                .try_get::<serde_json::Value, _>("payload")
                .ok()
                .and_then(|val| serde_json::from_value(val).ok());

            let job = DbJob {
                id: r.get("id"),
                user_id: r.get("user_id"),
                asset_id: r.get("asset_id"),
                file_name: r.get("file_name"),
                rel_path: r.get("rel_path"),
                folder_path: r.get("folder_path"),
                disk_path: PathBuf::from(r.get::<String, _>("disk_path")),
                sha256: r.get("sha256"),
                file_size_bytes: r.get("file_size_bytes"),
                status: r.get("status"),
                current_stage: r.get("current_stage"),
                payload,
                assemble_done: r.get("assemble_done"),
                thumb_done: r.get("thumb_done"),
                ai_faces_done: r.get("ai_faces_done"),
                ai_clip_done: r.get("ai_clip_done"),
                ai_tags_done: r.get("ai_tags_done"),
                ai_poses_done: r.get("ai_poses_done"),
            };

            sqlx::query(
                "UPDATE processing_jobs SET status = 'processing', attempts = attempts + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
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

    pub async fn mark_completed(pool: &PgPool, job_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE processing_jobs SET status = 'completed', current_stage = 'completed', updated_at = CURRENT_TIMESTAMP WHERE id = $1",
        )
        .bind(job_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn mark_failed(
        pool: &PgPool,
        job_id: Uuid,
        error: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE processing_jobs SET status = 'failed', last_error = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
        )
        .bind(error)
        .bind(job_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn reclaim_stalled_jobs(
        pool: &PgPool,
        timeout_minutes: i64,
    ) -> Result<u64, sqlx::Error> {
        let res = sqlx::query(
            r#"
            UPDATE processing_jobs
            SET status = CASE 
                    WHEN attempts >= 3 THEN 'failed'
                    ELSE 'pending'
                END,
                last_error = CASE 
                    WHEN attempts >= 3 THEN 'Worker timeout; exceeded retry limit'
                    ELSE 'Worker heartbeat timeout; requeued'
                END,
                updated_at = CURRENT_TIMESTAMP
            WHERE status = 'processing'
              AND updated_at < (CURRENT_TIMESTAMP - ($1 * INTERVAL '1 minute'))
            "#,
        )
        .bind(timeout_minutes)
        .execute(pool)
        .await?;

        Ok(res.rows_affected())
    }

    pub async fn touch_job_heartbeat(
        pool: &PgPool,
        job_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE processing_jobs SET updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND status = 'processing'"
        )
        .bind(job_id)
        .execute(pool)
        .await?;

        Ok(())
    }
}