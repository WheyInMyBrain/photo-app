// photo-app/crates/db/src/tag_repo.rs

use crate::domain::tag::AssetTagItem;
use sqlx::{PgConnection, PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct IngestionTagInput {
    pub name: String,
    pub confidence: f32,
    pub category: i32,  // 0 = General/ML, 1 = Hashtag, 2 = Author/Creator
    pub source: String, // "model", "manual", "scraped"
}

impl IngestionTagInput {
    pub fn new_ai(name: impl Into<String>, confidence: f32) -> Self {
        Self {
            name: name.into(),
            confidence: Self::sanitize_confidence(confidence),
            category: 0,
            source: "model".to_string(),
        }
    }

    pub fn new_hashtag(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            confidence: 1.0,
            category: 1,
            source: "scraped".to_string(),
        }
    }

    pub fn new_author(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            confidence: 1.0,
            category: 2,
            source: "scraped".to_string(),
        }
    }

    #[inline]
    fn sanitize_confidence(val: f32) -> f32 {
        if val.is_nan() || val.is_infinite() {
            0.0
        } else {
            val.clamp(0.0, 1.0)
        }
    }
}

pub struct TagRepo;

impl TagRepo {
    /// Fetch all tags attached to a specific asset.
    /// Explicitly decodes `confidence` as `f32` to match PostgreSQL `REAL` / `FLOAT4`.
    pub async fn get_by_asset(
        pool: &PgPool,
        asset_id: Uuid,
    ) -> Result<Vec<AssetTagItem>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT 
                t.id AS tag_id,
                t.name,
                at.confidence,
                at.source
            FROM asset_tags at
            JOIN tags t ON at.tag_id = t.id
            WHERE at.asset_id = $1
            ORDER BY at.confidence DESC
            "#,
        )
        .bind(asset_id)
        .fetch_all(pool)
        .await?;

        let tags = rows
            .into_iter()
            .map(|r| AssetTagItem {
                tag_id: r.get("tag_id"),
                name: r.get("name"),
                // Match Postgres REAL (FLOAT4) explicitly to prevent ColumnDecode panic
                confidence: r.get::<f32, _>("confidence") as f64,
                source: r.get("source"),
            })
            .collect();

        Ok(tags)
    }

    /// High-throughput atomic upsert and linkage.
    /// Uses two clean, safe batch operations within the active transaction.
    pub async fn save_asset_tags_tx(
        conn: &mut PgConnection,
        user_id: Uuid,
        asset_id: Uuid,
        tags: &[IngestionTagInput],
    ) -> Result<(), sqlx::Error> {
        if tags.is_empty() {
            return Ok(());
        }

        // 1. Strictly deduplicate by lowercased tag name, retaining highest confidence
        let mut deduped: HashMap<String, IngestionTagInput> = HashMap::with_capacity(tags.len());
        for tag in tags {
            let lower = tag.name.trim().to_lowercase();
            if lower.is_empty() {
                continue;
            }

            let conf = IngestionTagInput::sanitize_confidence(tag.confidence);

            match deduped.get_mut(&lower) {
                Some(existing) => {
                    if conf > existing.confidence {
                        existing.confidence = conf;
                        existing.category = tag.category;
                        existing.source = tag.source.clone();
                    }
                }
                None => {
                    deduped.insert(
                        lower.clone(),
                        IngestionTagInput {
                            name: lower,
                            confidence: conf,
                            category: tag.category,
                            source: tag.source.clone(),
                        },
                    );
                }
            }
        }

        if deduped.is_empty() {
            return Ok(());
        }

        let mut names = Vec::with_capacity(deduped.len());
        let mut categories = Vec::with_capacity(deduped.len());
        let mut confidences = Vec::with_capacity(deduped.len());
        let mut sources = Vec::with_capacity(deduped.len());
        let mut link_sources = Vec::with_capacity(deduped.len());

        for t in deduped.into_values() {
            let link_src = match t.source.as_str() {
                "scraped" => "SCRAPE",
                "manual" => "USER",
                _ => "AI",
            };
            names.push(t.name);
            categories.push(t.category);
            confidences.push(t.confidence);
            sources.push(t.source);
            link_sources.push(link_src.to_string());
        }

        // Step 2a: Upsert all unique tags into the user's tags catalog
        sqlx::query(
            r#"
            INSERT INTO tags (user_id, name, category, usage_count, source)
            SELECT 
                $1, 
                u.name, 
                u.category, 
                1,
                u.source
            FROM UNNEST($2::text[], $3::int[], $4::text[]) AS u(name, category, source)
            ON CONFLICT (user_id, name) DO UPDATE SET 
                usage_count = tags.usage_count + 1;
            "#,
        )
        .bind(user_id)
        .bind(&names)
        .bind(&categories)
        .bind(&sources)
        .execute(&mut *conn)
        .await?;

        // Step 2b: Link tags into asset_tags cleanly via tag table join
        sqlx::query(
            r#"
            INSERT INTO asset_tags (user_id, asset_id, tag_id, confidence, source)
            SELECT 
                $1, 
                $2, 
                t.id, 
                u.confidence, 
                u.link_source
            FROM UNNEST($3::text[], $4::real[], $5::text[]) AS u(name, confidence, link_source)
            JOIN tags t ON t.user_id = $1 AND t.name = u.name
            ON CONFLICT (asset_id, tag_id) DO UPDATE SET 
                confidence = GREATEST(EXCLUDED.confidence, asset_tags.confidence);
            "#,
        )
        .bind(user_id)
        .bind(asset_id)
        .bind(&names)
        .bind(&confidences)
        .bind(&link_sources)
        .execute(&mut *conn)
        .await?;

        Ok(())
    }

    /// Convenience wrapper for AI tag tuples `(name, confidence)`
    pub async fn save_model_tags_tx(
        conn: &mut PgConnection,
        user_id: Uuid,
        asset_id: Uuid,
        tags: &[(String, f32)],
    ) -> Result<(), sqlx::Error> {
        let inputs: Vec<IngestionTagInput> = tags
            .iter()
            .map(|(name, conf)| IngestionTagInput::new_ai(name.clone(), *conf))
            .collect();

        Self::save_asset_tags_tx(conn, user_id, asset_id, &inputs).await
    }
}