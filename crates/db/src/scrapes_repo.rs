// photo-app/crates/db/src/scrapes_repo.rs

use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

#[derive(Debug, Clone)]
pub struct ScrapedPostRecord {
    pub id: String,
    pub platform: String,
    pub external_post_id: String,
    pub source_url: String,
    pub author: String,
    pub caption: Option<String>,
    pub tags: Vec<String>,
    pub published_at: Option<String>, 
    pub next_page_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScrapedMediaItemRecord {
    pub id: String,
    pub media_type: String,
    pub cdn_url: String,
    pub audio_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub suggested_filename: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub variants: Vec<ScrapedVariantRecord>,
}

#[derive(Debug, Clone)]
pub struct ScrapedVariantRecord {
    pub url: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub label: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub is_master: bool,
}

#[derive(Debug, Clone)]
pub struct ScrapedItemContext {
    pub platform: String,
    pub author: String,
    pub caption: Option<String>,
    pub tags: Vec<String>,
    pub source_url: String,
    pub published_at: Option<String>, 
}

pub struct ScrapesRepo;

impl ScrapesRepo {
    /// Check if a post was already scraped by external ID
    pub async fn find_post_by_external_id(
        pool: &SqlitePool,
        user_id: &str,
        platform: &str,
        external_post_id: &str,
    ) -> Result<Option<ScrapedPostRecord>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, platform, external_post_id, source_url, author, caption, tags, published_at, next_page_url
            FROM scraped_posts
            WHERE user_id = ?1 AND platform = ?2 AND external_post_id = ?3
            "#,
        )
        .bind(user_id)
        .bind(platform)
        .bind(external_post_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| {
            let tags_raw: String = r.get("tags");
            let tags: Vec<String> = serde_json::from_str(&tags_raw).unwrap_or_default();

            ScrapedPostRecord {
                id: r.get("id"),
                platform: r.get("platform"),
                external_post_id: r.get("external_post_id"),
                source_url: r.get("source_url"),
                author: r.get("author"),
                caption: r.get("caption"),
                tags,
                published_at: r.get("published_at"),
                next_page_url: r.get("next_page_url"),
            }
        }))
    }

    pub async fn fetch_items_for_post(
        pool: &SqlitePool,
        post_id: &str,
    ) -> Result<Vec<ScrapedMediaItemRecord>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, media_type, cdn_url, audio_url, thumbnail_url, suggested_filename, width, height
            FROM scraped_media_items
            WHERE scraped_post_id = ?1
            ORDER BY item_index ASC
            "#,
        )
        .bind(post_id)
        .fetch_all(pool)
        .await?;

        let mut items = Vec::new();

        for r in rows {
            let item_id: String = r.get("id");

            let variant_rows = sqlx::query(
                r#"
                SELECT url, width, height, label, file_size_bytes, is_master
                FROM scraped_media_variants
                WHERE media_item_id = ?1
                ORDER BY width DESC
                "#,
            )
            .bind(&item_id)
            .fetch_all(pool)
            .await?;

            let variants = variant_rows
                .into_iter()
                .map(|vr| ScrapedVariantRecord {
                    url: vr.get("url"),
                    width: vr.get("width"),
                    height: vr.get("height"),
                    label: vr.get("label"),
                    file_size_bytes: vr.get("file_size_bytes"),
                    is_master: vr.get::<i32, _>("is_master") == 1,
                })
                .collect();

            items.push(ScrapedMediaItemRecord {
                id: item_id,
                media_type: r.get("media_type"),
                cdn_url: r.get("cdn_url"),
                audio_url: r.get("audio_url"),
                thumbnail_url: r.get("thumbnail_url"),
                suggested_filename: r.get("suggested_filename"),
                width: r.get("width"),
                height: r.get("height"),
                variants,
            });
        }

        Ok(items)
    }

    /// Full atomic save: post metadata, discovered links, media items, and all variant streams
    pub async fn save_scraped_post_and_items(
        pool: &SqlitePool,
        post_id: &str,
        user_id: &str,
        platform: &str,
        external_post_id: &str,
        source_url: &str,
        author: &str,
        caption: Option<&str>,
        tags: &[String],
        published_at: Option<&str>, // Added parameter
        discovered_urls: &[String],
        next_page_url: Option<&str>,
        items: &[ScrapedMediaItemRecord],
    ) -> Result<String, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());

        // 1. Upsert Post Record with published_at
        let resolved_post_id: String = sqlx::query_scalar(
            r#"
            INSERT INTO scraped_posts (
                id, user_id, platform, external_post_id, source_url, author, caption, tags, published_at, next_page_url
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(user_id, platform, external_post_id) DO UPDATE SET
                source_url = excluded.source_url,
                author = excluded.author,
                caption = excluded.caption,
                tags = excluded.tags,
                published_at = excluded.published_at,
                next_page_url = excluded.next_page_url
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
        .bind(published_at)
        .bind(next_page_url)
        .fetch_one(&mut *tx)
        .await?;

        // 2. Batch insert discovered URLs into crawler queue
        if !discovered_urls.is_empty() {
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO discovered_queue (id, user_id, parent_post_id, discovered_url, platform, depth) ",
            );

            qb.push_values(discovered_urls, |mut b, url| {
                b.push_bind(uuid::Uuid::new_v4().to_string())
                    .push_bind(user_id)
                    .push_bind(&resolved_post_id)
                    .push_bind(url)
                    .push_bind(platform)
                    .push_bind(1i32);
            });

            qb.push(" ON CONFLICT(user_id, discovered_url) DO NOTHING");
            qb.build().execute(&mut *tx).await?;
        }

        // 3. Insert items and their alternate resolution variants
        for (idx, item) in items.iter().enumerate() {
            sqlx::query(
                r#"
                INSERT INTO scraped_media_items (
                    id, scraped_post_id, item_index, media_type, cdn_url,
                    audio_url, thumbnail_url, suggested_filename, width, height, status
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'pending')
                ON CONFLICT(id) DO UPDATE SET
                    cdn_url = excluded.cdn_url,
                    audio_url = excluded.audio_url,
                    thumbnail_url = excluded.thumbnail_url,
                    suggested_filename = excluded.suggested_filename,
                    width = excluded.width,
                    height = excluded.height
                "#,
            )
            .bind(&item.id)
            .bind(&resolved_post_id)
            .bind(idx as i64)
            .bind(&item.media_type)
            .bind(&item.cdn_url)
            .bind(&item.audio_url)
            .bind(&item.thumbnail_url)
            .bind(&item.suggested_filename)
            .bind(item.width)
            .bind(item.height)
            .execute(&mut *tx)
            .await?;

            // 4. Batch insert alternate variants for this media item
            if !item.variants.is_empty() {
                let mut v_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                    "INSERT INTO scraped_media_variants (media_item_id, url, width, height, label, file_size_bytes, is_master) ",
                );

                v_qb.push_values(&item.variants, |mut b, v| {
                    b.push_bind(&item.id)
                        .push_bind(&v.url)
                        .push_bind(v.width)
                        .push_bind(v.height)
                        .push_bind(&v.label)
                        .push_bind(v.file_size_bytes)
                        .push_bind(if v.is_master { 1i32 } else { 0i32 });
                });

                v_qb.push(" ON CONFLICT(media_item_id, url) DO NOTHING");
                v_qb.build().execute(&mut *tx).await?;
            }
        }

        tx.commit().await?;
        Ok(resolved_post_id)
    }

    pub async fn mark_item_downloaded(
        pool: &SqlitePool,
        item_id: &str,
        asset_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE scraped_media_items SET status = 'downloaded', asset_id = ?1 WHERE id = ?2",
        )
        .bind(asset_id)
        .bind(item_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn find_post_by_url(
        pool: &SqlitePool,
        user_id: &str,
        source_url: &str,
    ) -> Result<Option<ScrapedPostRecord>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, platform, external_post_id, source_url, author, caption, tags, published_at, next_page_url
            FROM scraped_posts
            WHERE user_id = ?1 AND source_url = ?2
            "#,
        )
        .bind(user_id)
        .bind(source_url)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| {
            let tags_raw: String = r.get("tags");
            let tags: Vec<String> = serde_json::from_str(&tags_raw).unwrap_or_default();

            ScrapedPostRecord {
                id: r.get("id"),
                platform: r.get("platform"),
                external_post_id: r.get("external_post_id"),
                source_url: r.get("source_url"),
                author: r.get("author"),
                caption: r.get("caption"),
                tags,
                published_at: r.get("published_at"),
                next_page_url: r.get("next_page_url"),
            }
        }))
    }

    /// Fetch parent post metadata for a specific media item
    pub async fn get_item_context(
        pool: &SqlitePool,
        item_id: &str,
    ) -> Result<Option<ScrapedItemContext>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT sp.platform, sp.author, sp.caption, sp.tags, sp.source_url, sp.published_at
            FROM scraped_media_items smi
            JOIN scraped_posts sp ON smi.scraped_post_id = sp.id
            WHERE smi.id = ?1
            LIMIT 1
            "#,
        )
        .bind(item_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| {
            let tags_raw: String = r.get("tags");
            let tags: Vec<String> = serde_json::from_str(&tags_raw).unwrap_or_default();

            ScrapedItemContext {
                platform: r.get("platform"),
                author: r.get("author"),
                caption: r.get("caption"),
                tags,
                source_url: r.get("source_url"),
                published_at: r.get("published_at"),
            }
        }))
    }
}