// photo-app/crates/db/src/scrapes_repo.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedPostRecord {
    pub id: Uuid,
    pub platform: String,
    pub external_post_id: String,
    pub source_url: String,
    pub author: String,
    pub caption: Option<String>,
    pub tags: Vec<String>,
    pub location_name: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub published_at: Option<DateTime<Utc>>,
    pub next_page_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedMediaItemRecord {
    pub id: Uuid,
    pub media_type: String,
    pub cdn_url: String,
    pub audio_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub suggested_filename: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub variants: Vec<ScrapedVariantRecord>,

    // Per-item metadata overrides
    pub caption: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub location_name: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub tags: Vec<String>,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedVariantRecord {
    pub url: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub label: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub is_master: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapedItemContext {
    pub platform: String,
    pub author: String,
    pub caption: Option<String>,
    pub tags: Vec<String>,
    pub location_name: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub source_url: String,
    pub published_at: Option<DateTime<Utc>>,
}

pub struct ScrapesRepo;

impl ScrapesRepo {
    /// Check if a post was already scraped by external ID
    pub async fn find_post_by_external_id(
        pool: &PgPool,
        user_id: Uuid,
        platform: &str,
        external_post_id: &str,
    ) -> Result<Option<ScrapedPostRecord>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, platform, external_post_id, source_url, author, caption, tags,
                   location_name, latitude, longitude, published_at, next_page_url
            FROM scraped_posts
            WHERE user_id = $1 AND platform = $2 AND external_post_id = $3
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(platform)
        .bind(external_post_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| {
            let tags_json: serde_json::Value = r.get("tags");
            let tags: Vec<String> = serde_json::from_value(tags_json).unwrap_or_default();

            ScrapedPostRecord {
                id: r.get("id"),
                platform: r.get("platform"),
                external_post_id: r.get("external_post_id"),
                source_url: r.get("source_url"),
                author: r.get("author"),
                caption: r.get("caption"),
                tags,
                location_name: r.get("location_name"),
                latitude: r.get("latitude"),
                longitude: r.get("longitude"),
                published_at: r.get("published_at"),
                next_page_url: r.get("next_page_url"),
            }
        }))
    }

    /// Fetches all media items for a post and their nested variants in ONE single database query
    pub async fn fetch_items_for_post(
        pool: &PgPool,
        post_id: Uuid,
    ) -> Result<Vec<ScrapedMediaItemRecord>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT 
                smi.id,
                smi.media_type,
                smi.cdn_url,
                smi.audio_url,
                smi.thumbnail_url,
                smi.suggested_filename,
                smi.width,
                smi.height,
                smi.caption,
                smi.published_at,
                smi.location_name,
                smi.latitude,
                smi.longitude,
                smi.tags,
                smi.source_url,
                COALESCE(
                    jsonb_agg(
                        jsonb_build_object(
                            'url', smv.url,
                            'width', smv.width,
                            'height', smv.height,
                            'label', smv.label,
                            'file_size_bytes', smv.file_size_bytes,
                            'is_master', smv.is_master
                        ) ORDER BY smv.width DESC NULLS LAST
                    ) FILTER (WHERE smv.id IS NOT NULL),
                    '[]'::jsonb
                ) AS variants
            FROM scraped_media_items smi
            LEFT JOIN scraped_media_variants smv ON smi.id = smv.media_item_id
            WHERE smi.scraped_post_id = $1
            GROUP BY smi.id
            ORDER BY smi.item_index ASC
            "#,
        )
        .bind(post_id)
        .fetch_all(pool)
        .await?;

        let items = rows
            .into_iter()
            .map(|r| {
                let tags_json: serde_json::Value = r.get("tags");
                let tags: Vec<String> = serde_json::from_value(tags_json).unwrap_or_default();

                let variants_json: serde_json::Value = r.get("variants");
                let variants: Vec<ScrapedVariantRecord> =
                    serde_json::from_value(variants_json).unwrap_or_default();

                ScrapedMediaItemRecord {
                    id: r.get("id"),
                    media_type: r.get("media_type"),
                    cdn_url: r.get("cdn_url"),
                    audio_url: r.get("audio_url"),
                    thumbnail_url: r.get("thumbnail_url"),
                    suggested_filename: r.get("suggested_filename"),
                    width: r.get("width"),
                    height: r.get("height"),
                    variants,
                    caption: r.get("caption"),
                    published_at: r.get("published_at"),
                    location_name: r.get("location_name"),
                    latitude: r.get("latitude"),
                    longitude: r.get("longitude"),
                    tags,
                    source_url: r.get("source_url"),
                }
            })
            .collect();

        Ok(items)
    }

    /// Full atomic save: post metadata with coordinates, discovered links, media items, and variants
    pub async fn save_scraped_post_and_items(
        pool: &PgPool,
        post_id: Uuid,
        user_id: Uuid,
        platform: &str,
        external_post_id: &str,
        source_url: &str,
        author: &str,
        caption: Option<&str>,
        tags: &[String],
        location_name: Option<&str>,
        latitude: Option<f64>,
        longitude: Option<f64>,
        published_at: Option<DateTime<Utc>>,
        discovered_urls: &[String],
        next_page_url: Option<&str>,
        items: &[ScrapedMediaItemRecord],
    ) -> Result<Uuid, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let tags_json = serde_json::to_value(tags).unwrap_or_else(|_| serde_json::json!([]));

        // 1. Upsert Post Record
        let resolved_post_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO scraped_posts (
                id, user_id, platform, external_post_id, source_url, author, caption, tags,
                location_name, latitude, longitude, published_at, next_page_url
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (user_id, platform, external_post_id) DO UPDATE SET
                source_url = EXCLUDED.source_url,
                author = EXCLUDED.author,
                caption = EXCLUDED.caption,
                tags = EXCLUDED.tags,
                location_name = EXCLUDED.location_name,
                latitude = EXCLUDED.latitude,
                longitude = EXCLUDED.longitude,
                published_at = EXCLUDED.published_at,
                next_page_url = EXCLUDED.next_page_url
            RETURNING id
            "#,
        )
        .bind(post_id)
        .bind(user_id)
        .bind(platform)
        .bind(external_post_id)
        .bind(source_url)
        .bind(author)
        .bind(caption)
        .bind(tags_json)
        .bind(location_name)
        .bind(latitude)
        .bind(longitude)
        .bind(published_at)
        .bind(next_page_url)
        .fetch_one(&mut *tx)
        .await?;

        // 2. Batch insert discovered URLs into crawler queue
        if !discovered_urls.is_empty() {
            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO discovered_queue (id, user_id, parent_post_id, discovered_url, platform, depth) ",
            );

            qb.push_values(discovered_urls, |mut b, url| {
                b.push_bind(Uuid::new_v4())
                    .push_bind(user_id)
                    .push_bind(resolved_post_id)
                    .push_bind(url)
                    .push_bind(platform)
                    .push_bind(1i32);
            });

            qb.push(" ON CONFLICT (user_id, discovered_url) DO NOTHING");
            qb.build().execute(&mut *tx).await?;
        }

        // 3. Upsert media items on (scraped_post_id, item_index)
        for (idx, item) in items.iter().enumerate() {
            let item_tags_json = serde_json::to_value(&item.tags).unwrap_or_else(|_| serde_json::json!([]));

            let resolved_item_id: Uuid = sqlx::query_scalar(
                r#"
                INSERT INTO scraped_media_items (
                    id, scraped_post_id, item_index, media_type, cdn_url,
                    audio_url, thumbnail_url, suggested_filename, width, height, status,
                    caption, published_at, location_name, latitude, longitude, tags, source_url
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'pending', $11, $12, $13, $14, $15, $16, $17)
                ON CONFLICT (scraped_post_id, item_index) DO UPDATE SET
                    cdn_url = EXCLUDED.cdn_url,
                    audio_url = EXCLUDED.audio_url,
                    thumbnail_url = EXCLUDED.thumbnail_url,
                    suggested_filename = EXCLUDED.suggested_filename,
                    width = EXCLUDED.width,
                    height = EXCLUDED.height,
                    caption = EXCLUDED.caption,
                    published_at = EXCLUDED.published_at,
                    location_name = EXCLUDED.location_name,
                    latitude = EXCLUDED.latitude,
                    longitude = EXCLUDED.longitude,
                    tags = EXCLUDED.tags,
                    source_url = EXCLUDED.source_url
                RETURNING id
                "#,
            )
            .bind(item.id)
            .bind(resolved_post_id)
            .bind(idx as i32)
            .bind(&item.media_type)
            .bind(&item.cdn_url)
            .bind(&item.audio_url)
            .bind(&item.thumbnail_url)
            .bind(&item.suggested_filename)
            .bind(item.width)
            .bind(item.height)
            .bind(&item.caption)
            .bind(item.published_at)
            .bind(&item.location_name)
            .bind(item.latitude)
            .bind(item.longitude)
            .bind(item_tags_json)
            .bind(&item.source_url)
            .fetch_one(&mut *tx)
            .await?;

            // 4. Batch insert alternate variants
            if !item.variants.is_empty() {
                let mut v_qb: QueryBuilder<Postgres> = QueryBuilder::new(
                    "INSERT INTO scraped_media_variants (media_item_id, url, width, height, label, file_size_bytes, is_master) ",
                );

                v_qb.push_values(&item.variants, |mut b, v| {
                    b.push_bind(resolved_item_id)
                        .push_bind(&v.url)
                        .push_bind(v.width)
                        .push_bind(v.height)
                        .push_bind(&v.label)
                        .push_bind(v.file_size_bytes)
                        .push_bind(v.is_master);
                });

                v_qb.push(" ON CONFLICT (media_item_id, url) DO NOTHING");
                v_qb.build().execute(&mut *tx).await?;
            }
        }

        tx.commit().await?;
        Ok(resolved_post_id)
    }

    pub async fn mark_item_downloaded(
        pool: &PgPool,
        item_id: Uuid,
        asset_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE scraped_media_items SET status = 'downloaded', asset_id = $1 WHERE id = $2",
        )
        .bind(asset_id)
        .bind(item_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn find_post_by_url(
        pool: &PgPool,
        user_id: Uuid,
        source_url: &str,
    ) -> Result<Option<ScrapedPostRecord>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, platform, external_post_id, source_url, author, caption, tags,
                   location_name, latitude, longitude, published_at, next_page_url
            FROM scraped_posts
            WHERE user_id = $1 AND source_url = $2
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(source_url)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| {
            let tags_json: serde_json::Value = r.get("tags");
            let tags: Vec<String> = serde_json::from_value(tags_json).unwrap_or_default();

            ScrapedPostRecord {
                id: r.get("id"),
                platform: r.get("platform"),
                external_post_id: r.get("external_post_id"),
                source_url: r.get("source_url"),
                author: r.get("author"),
                caption: r.get("caption"),
                tags,
                location_name: r.get("location_name"),
                latitude: r.get("latitude"),
                longitude: r.get("longitude"),
                published_at: r.get("published_at"),
                next_page_url: r.get("next_page_url"),
            }
        }))
    }

    /// Fetch contextual metadata for an item: item-level metadata takes priority over post-level
    pub async fn get_item_context(
        pool: &PgPool,
        item_id: Uuid,
    ) -> Result<Option<ScrapedItemContext>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT
                sp.platform,
                sp.author,
                COALESCE(smi.caption, sp.caption) AS caption,
                COALESCE(smi.tags, sp.tags) AS tags,
                COALESCE(smi.location_name, sp.location_name) AS location_name,
                COALESCE(smi.latitude, sp.latitude) AS latitude,
                COALESCE(smi.longitude, sp.longitude) AS longitude,
                COALESCE(smi.source_url, sp.source_url) AS source_url,
                COALESCE(smi.published_at, sp.published_at) AS published_at
            FROM scraped_media_items smi
            JOIN scraped_posts sp ON smi.scraped_post_id = sp.id
            WHERE smi.id = $1
            LIMIT 1
            "#,
        )
        .bind(item_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| {
            let tags_json: serde_json::Value = r.get("tags");
            let tags: Vec<String> = serde_json::from_value(tags_json).unwrap_or_default();

            ScrapedItemContext {
                platform: r.get("platform"),
                author: r.get("author"),
                caption: r.get("caption"),
                tags,
                location_name: r.get("location_name"),
                latitude: r.get("latitude"),
                longitude: r.get("longitude"),
                source_url: r.get("source_url"),
                published_at: r.get("published_at"),
            }
        }))
    }

    /// Fast set retrieval of existing post IDs for a specific user & author
    pub async fn get_existing_post_ids(
        pool: &PgPool,
        user_id: Uuid,
        platform: &str,
        author: &str,
    ) -> Result<HashSet<String>, sqlx::Error> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT external_post_id
            FROM scraped_posts
            WHERE user_id = $1 AND platform = $2 AND LOWER(author) = LOWER($3)
            "#,
        )
        .bind(user_id)
        .bind(platform)
        .bind(author)
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().collect())
    }
}