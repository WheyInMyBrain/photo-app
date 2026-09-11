use image::DynamicImage;
use sqlx::{Row, SqlitePool};

use super::tag_engine::TagEngine;

pub struct TagPipeline;

impl TagPipeline {
    /// Minimum probability threshold to associate a WD tag (0.35 is optimal for WD14)
    pub const CONFIDENCE_THRESHOLD: f32 = 0.35;

    pub async fn process_asset_tags(
        engine: &TagEngine,
        pool: &SqlitePool,
        asset_id: &str,
        img: &DynamicImage,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let ranked_tags = engine.tag_image(img, Self::CONFIDENCE_THRESHOLD)?;

        // 1. Mark asset as processed in SQLite
        sqlx::query(
            r#"
            UPDATE assets
            SET tags_processed = 1
            WHERE id = ?1
            "#,
        )
        .bind(asset_id)
        .execute(pool)
        .await?;

        let mut applied_count = 0;

        // 2. Insert predicted tags into database
        for (tag_name, confidence) in ranked_tags {
            let tag_id: i64 = match sqlx::query("SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE")
                .bind(&tag_name)
                .fetch_optional(pool)
                .await?
            {
                Some(row) => row.try_get::<i64, _>("id")?,
                None => {
                    let res = sqlx::query("INSERT INTO tags (name, source) VALUES (?1, 'model')")
                        .bind(&tag_name)
                        .execute(pool)
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
            .bind(confidence as f64)
            .execute(pool)
            .await?;

            applied_count += 1;
        }

        Ok(applied_count)
    }
}