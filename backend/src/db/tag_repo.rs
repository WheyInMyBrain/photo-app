use sqlx::{Row, SqlitePool};
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
}