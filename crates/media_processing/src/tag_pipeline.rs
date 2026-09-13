use image::DynamicImage;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;

use super::tag_engine::TagEngine;

#[derive(Debug, Clone)]
pub struct TagPrediction {
    pub name: String,
    pub confidence: f32,
}

pub struct TagPipeline;

impl TagPipeline {
    pub const CONFIDENCE_THRESHOLD: f32 = 0.35;

    /// 1. Synchronous CPU/ONNX computation — safe for `spawn_blocking`
    pub fn compute_tags_sync(
        engine: &TagEngine,
        img: &DynamicImage,
    ) -> Result<Vec<TagPrediction>, Box<dyn std::error::Error + Send + Sync>> {
        let ranked_tags = engine.tag_image(img, Self::CONFIDENCE_THRESHOLD)?;
        let predictions = ranked_tags
            .into_iter()
            .map(|(name, confidence)| TagPrediction { name, confidence })
            .collect();
        Ok(predictions)
    }

    /// 2. Fast Async Database Persistence (Single Transaction)
    pub async fn persist_tags(
        pool: &SqlitePool,
        asset_id: &str,
        predictions: Vec<TagPrediction>,
    ) -> Result<usize, sqlx::Error> {
        let mut tx = pool.begin().await?;

        // Mark asset as tagged
        sqlx::query("UPDATE assets SET tags_processed = 1 WHERE id = ?1")
            .bind(asset_id)
            .execute(&mut *tx)
            .await?;

        if predictions.is_empty() {
            tx.commit().await?;
            return Ok(0);
        }

        let count = predictions.len();

        for pred in predictions {
            // Find or insert tag
            let tag_id: i64 = match sqlx::query("SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE")
                .bind(&pred.name)
                .fetch_optional(&mut *tx)
                .await?
            {
                Some(row) => row.try_get("id")?,
                None => {
                    let res = sqlx::query("INSERT INTO tags (name, source) VALUES (?1, 'model')")
                        .bind(&pred.name)
                        .execute(&mut *tx)
                        .await?;
                    res.last_insert_rowid()
                }
            };

            sqlx::query(
                r#"
                INSERT INTO asset_tags (asset_id, tag_id, confidence, source)
                VALUES (?1, ?2, ?3, 'AI')
                ON CONFLICT(asset_id, tag_id) DO UPDATE SET
                    confidence = excluded.confidence,
                    source = 'AI'
                "#,
            )
            .bind(asset_id)
            .bind(tag_id)
            .bind(pred.confidence as f64)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(count)
    }

    /// Helper to run the entire pipeline cleanly off-thread
    pub async fn process_asset_tags(
        engine: Arc<TagEngine>,
        pool: &SqlitePool,
        asset_id: &str,
        img: Arc<DynamicImage>,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        // Offload ONNX to blocking pool
        let predictions = tokio::task::spawn_blocking(move || {
            Self::compute_tags_sync(&engine, &img)
        })
        .await??;

        // Persist via async DB pool
        let applied = Self::persist_tags(pool, asset_id, predictions).await?;
        Ok(applied)
    }
}