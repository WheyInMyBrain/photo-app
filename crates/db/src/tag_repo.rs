use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
use crate::domain::tag::AssetTagItem;

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

    /// Transactional batch insert for tags during asset ingestion
    pub async fn save_asset_tags_tx(
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        asset_id: &str,
        tags: &[(String, f32)], // (name, confidence)
    ) -> Result<(), sqlx::Error> {
        if tags.is_empty() {
            return Ok(());
        }

        // 1. Batch insert new tags
        let mut tag_qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("INSERT INTO tags (user_id, name, source) ");

        tag_qb.push_values(tags, |mut b, (name, _)| {
            b.push_bind(user_id)
                .push_bind(name)
                .push_bind("model");
        });
        tag_qb.push(" ON CONFLICT(user_id, name) DO NOTHING");

        tag_qb.build().execute(&mut **tx).await?;

        // 2. Fetch tag IDs for linking
        let mut fetch_qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("SELECT id, name FROM tags WHERE user_id = ");
        fetch_qb.push_bind(user_id);
        fetch_qb.push(" AND name IN (");

        let mut separated = fetch_qb.separated(", ");
        for (name, _) in tags {
            separated.push_bind(name);
        }
        separated.push_unseparated(")");

        let tag_rows = fetch_qb.build().fetch_all(&mut **tx).await?;

        let tag_map: std::collections::HashMap<String, i64> = tag_rows
            .into_iter()
            .filter_map(|r| {
                let id: i64 = r.try_get("id").ok()?;
                let name: String = r.try_get("name").ok()?;
                Some((name.to_lowercase(), id))
            })
            .collect();

        // 3. Link tags to asset
        let valid_tags: Vec<(i64, f64)> = tags
            .iter()
            .filter_map(|(name, conf)| {
                tag_map
                    .get(&name.to_lowercase())
                    .map(|&id| (id, *conf as f64))
            })
            .collect();

        if !valid_tags.is_empty() {
            let mut link_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO asset_tags (asset_id, tag_id, confidence, source) ",
            );

            link_qb.push_values(valid_tags, |mut b, (tag_id, conf)| {
                b.push_bind(asset_id)
                    .push_bind(tag_id)
                    .push_bind(conf)
                    .push_bind("AI");
            });

            link_qb.push(" ON CONFLICT(asset_id, tag_id) DO UPDATE SET confidence = excluded.confidence");
            link_qb.build().execute(&mut **tx).await?;
        }

        Ok(())
    }
}