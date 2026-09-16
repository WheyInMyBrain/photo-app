use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

#[derive(Debug, Clone)]
pub struct ScrapedPostRecord {
    pub id: String,
    pub platform: String,
    pub author: String,
    pub caption: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScrapedMediaItemRecord {
    pub id: String,
    pub media_type: String,
    pub cdn_url: String,
    pub audio_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub suggested_filename: String,
}

pub struct ScrapesRepo;

impl ScrapesRepo {
    pub async fn find_post_by_external_id(
        pool: &SqlitePool,
        user_id: &str,
        external_post_id: &str,
    ) -> Result<Option<ScrapedPostRecord>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, platform, author, caption FROM scraped_posts WHERE user_id = ",
        );
        qb.push_bind(user_id);
        qb.push(" AND external_post_id = ");
        qb.push_bind(external_post_id);

        let row = qb.build().fetch_optional(pool).await?;
        Ok(row.map(|r| ScrapedPostRecord {
            id: r.get("id"),
            platform: r.get("platform"),
            author: r.get("author"),
            caption: r.get("caption"),
        }))
    }

    pub async fn fetch_items_for_post(
        pool: &SqlitePool,
        post_id: &str,
    ) -> Result<Vec<ScrapedMediaItemRecord>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, media_type, cdn_url, audio_url, thumbnail_url, suggested_filename \
             FROM scraped_media_items WHERE scraped_post_id = ",
        );
        qb.push_bind(post_id);
        qb.push(" ORDER BY item_index ASC");

        let rows = qb.build().fetch_all(pool).await?;
        Ok(rows
            .into_iter()
            .map(|r| ScrapedMediaItemRecord {
                id: r.get("id"),
                media_type: r.get("media_type"),
                cdn_url: r.get("cdn_url"),
                audio_url: r.get("audio_url"),
                thumbnail_url: r.get("thumbnail_url"),
                suggested_filename: r.get("suggested_filename"),
            })
            .collect())
    }

    pub async fn save_scraped_post_and_items(
        pool: &SqlitePool,
        post_id: &str,
        user_id: &str,
        platform: &str,
        external_post_id: &str,
        author: &str,
        caption: &str,
        items: &[ScrapedMediaItemRecord],
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;

        let mut post_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "INSERT INTO scraped_posts (id, user_id, platform, external_post_id, author, caption) ",
        );
        post_qb.push_values(
            std::iter::once((post_id, user_id, platform, external_post_id, author, caption)),
            |mut b, (id, u_id, plat, ext_id, auth, cap)| {
                b.push_bind(id)
                    .push_bind(u_id)
                    .push_bind(plat)
                    .push_bind(ext_id)
                    .push_bind(auth)
                    .push_bind(cap);
            },
        );
        post_qb.push(" ON CONFLICT(user_id, platform, external_post_id) DO NOTHING");
        post_qb.build().execute(&mut *tx).await?;

        if !items.is_empty() {
            let indexed_items: Vec<(i64, &ScrapedMediaItemRecord)> = items
                .iter()
                .enumerate()
                .map(|(idx, item)| (idx as i64, item))
                .collect();

            let mut items_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO scraped_media_items \
                 (id, scraped_post_id, item_index, media_type, cdn_url, audio_url, thumbnail_url, suggested_filename, status) ",
            );

            items_qb.push_values(indexed_items, |mut b, (idx, item)| {
                b.push_bind(&item.id)
                    .push_bind(post_id)
                    .push_bind(idx)
                    .push_bind(&item.media_type)
                    .push_bind(&item.cdn_url)
                    .push_bind(&item.audio_url)
                    .push_bind(&item.thumbnail_url)
                    .push_bind(&item.suggested_filename)
                    .push_bind("pending");
            });
            items_qb.push(" ON CONFLICT(scraped_post_id, item_index) DO NOTHING");
            items_qb.build().execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_item_downloaded(pool: &SqlitePool, item_id: &str) -> Result<(), sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "UPDATE scraped_media_items SET status = 'downloaded' WHERE id = ",
        );
        qb.push_bind(item_id);
        qb.build().execute(pool).await?;
        Ok(())
    }
}