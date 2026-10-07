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
            confidence,
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
}

pub struct TagRepo;

impl TagRepo {
    /// Fetch all tags attached to a specific asset
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
                confidence: r.get("confidence"),
                source: r.get("source"),
            })
            .collect();

        Ok(tags)
    }

    /// High-throughput atomic upsert and linkage using a single CTE statement.
    /// Eliminates Rust-side ID lookups and avoids multiple database roundtrips.
    pub async fn save_asset_tags_tx(
        conn: &mut PgConnection,
        user_id: Uuid,
        asset_id: Uuid,
        tags: &[IngestionTagInput],
    ) -> Result<(), sqlx::Error> {
        if tags.is_empty() {
            return Ok(());
        }

        // Deduplicate input by lowercased name, preserving the highest confidence entry
        let mut deduped: HashMap<String, IngestionTagInput> = HashMap::with_capacity(tags.len());
        for tag in tags {
            let lower = tag.name.trim().to_lowercase();
            if lower.is_empty() {
                continue;
            }
            match deduped.get_mut(&lower) {
                Some(existing) => {
                    if tag.confidence > existing.confidence {
                        *existing = tag.clone();
                    }
                }
                None => {
                    let mut t = tag.clone();
                    t.name = lower.clone();
                    deduped.insert(lower, t);
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

        // Single roundtrip: Upserts tags into the catalog and links them into asset_tags directly
        sqlx::query(
            r#"
            WITH input_data AS (
                SELECT * FROM UNNEST(
                    $3::text[], 
                    $4::int[], 
                    $5::real[], 
                    $6::text[], 
                    $7::text[]
                ) AS u(name, category, confidence, source, link_source)
            ),
            upserted_tags AS (
                INSERT INTO tags (user_id, name, category, usage_count, source)
                SELECT 
                    $1, 
                    inp.name, 
                    inp.category, 
                    1, 
                    inp.source
                FROM input_data inp
                ON CONFLICT (user_id, name) DO UPDATE SET 
                    usage_count = tags.usage_count + 1
                RETURNING id, name
            )
            INSERT INTO asset_tags (user_id, asset_id, tag_id, confidence, source)
            SELECT 
                $1, 
                $2, 
                ut.id, 
                inp.confidence, 
                inp.link_source
            FROM input_data inp
            JOIN upserted_tags ut ON ut.name = inp.name
            ON CONFLICT (asset_id, tag_id) DO UPDATE SET 
                confidence = GREATEST(EXCLUDED.confidence, asset_tags.confidence);
            "#,
        )
        .bind(user_id)
        .bind(asset_id)
        .bind(&names)
        .bind(&categories)
        .bind(&confidences)
        .bind(&sources)
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