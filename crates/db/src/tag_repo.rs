// photo-app/crates/db/src/tag_repo.rs

use crate::domain::tag::AssetTagItem;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct IngestionTagInput {
    pub name: String,
    pub confidence: f32,
    pub category: i32, // 0 = General/ML, 1 = Hashtag, 2 = Author/Creator
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
        pool: &SqlitePool,
        asset_id: &str,
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
            WHERE at.asset_id = ?1
            ORDER BY at.confidence DESC
            "#,
        )
        .bind(asset_id)
        .fetch_all(pool)
        .await?;

        let tags = rows
            .into_iter()
            .filter_map(|r| {
                Some(AssetTagItem {
                    tag_id: r.try_get("tag_id").ok()?,
                    name: r.try_get("name").ok()?,
                    confidence: r.try_get("confidence").unwrap_or(0.0),
                    source: r.try_get("source").unwrap_or_else(|_| "AI".to_string()),
                })
            })
            .collect();

        Ok(tags)
    }

    /// Flexible transactional batch insert for AI tags, hashtags, and authors
    pub async fn save_asset_tags_tx(
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        asset_id: &str,
        tags: &[IngestionTagInput],
    ) -> Result<(), sqlx::Error> {
        if tags.is_empty() {
            return Ok(());
        }

        // Deduplicate input by lowercased name, prioritizing higher confidence
        let mut deduped: HashMap<String, IngestionTagInput> = HashMap::new();
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

        let clean_tags: Vec<IngestionTagInput> = deduped.into_values().collect();
        if clean_tags.is_empty() {
            return Ok(());
        }

        // 1. Batch upsert unique tags per user, bumping usage count on conflict
        let mut tag_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO tags (user_id, name, category, usage_count, source) ",
        );

        tag_qb.push_values(&clean_tags, |mut b, t| {
            b.push_bind(user_id)
                .push_bind(&t.name)
                .push_bind(t.category)
                .push_bind(1i64)
                .push_bind(&t.source);
        });

        tag_qb.push(
            " ON CONFLICT(user_id, name COLLATE NOCASE) DO UPDATE SET \
             usage_count = tags.usage_count + 1",
        );
        tag_qb.build().execute(&mut **tx).await?;

        // 2. Fetch tag IDs for linking
        let mut fetch_qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("SELECT id, name FROM tags WHERE user_id = ");
        fetch_qb.push_bind(user_id);
        fetch_qb.push(" AND name IN (");

        let mut separated = fetch_qb.separated(", ");
        for t in &clean_tags {
            separated.push_bind(&t.name);
        }
        separated.push_unseparated(")");

        let tag_rows = fetch_qb.build().fetch_all(&mut **tx).await?;

        let tag_map: HashMap<String, i64> = tag_rows
            .into_iter()
            .filter_map(|r| {
                let id: i64 = r.try_get("id").ok()?;
                let name: String = r.try_get("name").ok()?;
                Some((name.to_lowercase(), id))
            })
            .collect();

        // 3. Link tags into asset_tags
        let valid_links: Vec<(i64, f64, String)> = clean_tags
            .iter()
            .filter_map(|t| {
                let tag_id = *tag_map.get(&t.name)?;
                let link_source = match t.source.as_str() {
                    "scraped" => "SCRAPE",
                    "manual" => "USER",
                    _ => "AI",
                };
                Some((tag_id, t.confidence as f64, link_source.to_string()))
            })
            .collect();

        if !valid_links.is_empty() {
            let mut link_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO asset_tags (asset_id, tag_id, confidence, source) ",
            );

            link_qb.push_values(valid_links, |mut b, (tag_id, conf, src)| {
                b.push_bind(asset_id)
                    .push_bind(tag_id)
                    .push_bind(conf)
                    .push_bind(src);
            });

            link_qb.push(
                " ON CONFLICT(asset_id, tag_id) DO UPDATE SET \
                 confidence = MAX(excluded.confidence, asset_tags.confidence)",
            );
            link_qb.build().execute(&mut **tx).await?;
        }

        // 4. Update FTS5 search index (Delete then insert because FTS5 doesn't support UPSERT)
        sqlx::query("DELETE FROM asset_search_index WHERE asset_id = ?1")
            .bind(asset_id)
            .execute(&mut **tx)
            .await?;

        let tag_terms: Vec<String> = clean_tags.iter().map(|t| t.name.clone()).collect();
        let space_separated = tag_terms.join(" ");

        sqlx::query(
            r#"
            INSERT INTO asset_search_index (asset_id, user_id, tags)
            VALUES (?1, ?2, ?3)
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .bind(space_separated)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    /// Convenience wrapper for AI tags tuples `(name, confidence)`
    pub async fn save_model_tags_tx(
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        asset_id: &str,
        tags: &[(String, f32)],
    ) -> Result<(), sqlx::Error> {
        let inputs: Vec<IngestionTagInput> = tags
            .iter()
            .map(|(name, conf)| IngestionTagInput::new_ai(name.clone(), *conf))
            .collect();

        Self::save_asset_tags_tx(tx, user_id, asset_id, &inputs).await
    }
}