use sqlx::{SqlitePool, Result, Row};

pub struct SubAlbumRecord {
    pub name: String,
    pub full_path: String,
    pub media_count: i64,
    pub cover_thumb: Option<String>,
}

pub struct AlbumRepo;

impl AlbumRepo {
    pub async fn get_sub_albums(
        pool: &SqlitePool,
        current_path: &str,
        privacy_level: i64,
    ) -> Result<Vec<SubAlbumRecord>> {
        let names: Vec<String> = if current_path.is_empty() {
            let rows = sqlx::query(
                r#"
                SELECT DISTINCT 
                    CASE 
                        WHEN INSTR(folder_path, '/') > 0 
                        THEN SUBSTR(folder_path, 1, INSTR(folder_path, '/') - 1)
                        ELSE folder_path 
                    END as album_name
                FROM assets 
                WHERE is_private <= ?1 AND folder_path != 'root'
                ORDER BY album_name ASC
                "#,
            )
            .bind(privacy_level)
            .fetch_all(pool)
            .await?;

            rows.into_iter()
                .filter_map(|r| r.try_get::<String, _>("album_name").ok())
                .collect()
        } else {
            let prefix_len = current_path.len() as i32;
            let rows = sqlx::query(
                r#"
                SELECT DISTINCT 
                    CASE 
                        WHEN INSTR(SUBSTR(folder_path, ?1 + 2), '/') > 0 
                        THEN SUBSTR(SUBSTR(folder_path, ?1 + 2), 1, INSTR(SUBSTR(folder_path, ?1 + 2), '/') - 1)
                        ELSE SUBSTR(folder_path, ?1 + 2)
                    END as album_name
                FROM assets 
                WHERE is_private <= ?2 AND folder_path LIKE ?3 || '/%'
                ORDER BY album_name ASC
                "#,
            )
            .bind(prefix_len)
            .bind(privacy_level)
            .bind(current_path)
            .fetch_all(pool)
            .await?;

            rows.into_iter()
                .filter_map(|r| r.try_get::<String, _>("album_name").ok())
                .collect()
        };

        let mut cards = Vec::with_capacity(names.len());
        for name in names {
            let child_full_path = if current_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", current_path, name)
            };

            let row = sqlx::query(
                r#"
                SELECT 
                    COUNT(*) as count,
                    (
                        SELECT thumb_path 
                        FROM assets 
                        WHERE is_private <= ?1 AND (folder_path = ?2 OR folder_path LIKE ?2 || '/%')
                        ORDER BY captured_at DESC, created_at DESC 
                        LIMIT 1
                    ) as cover_thumb
                FROM assets
                WHERE is_private <= ?1 AND (folder_path = ?2 OR folder_path LIKE ?2 || '/%')
                "#,
            )
            .bind(privacy_level)
            .bind(&child_full_path)
            .fetch_one(pool)
            .await?;

            cards.push(SubAlbumRecord {
                name,
                full_path: child_full_path,
                media_count: row.try_get("count").unwrap_or(0),
                cover_thumb: row.try_get("cover_thumb").ok(),
            });
        }

        Ok(cards)
    }

    pub async fn get_all_folder_paths(pool: &SqlitePool, privacy_level: i64) -> Result<Vec<String>> {
        let asset_folders: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT folder_path FROM assets WHERE is_private <= ?1 AND folder_path IS NOT NULL AND folder_path != ''"
        )
        .bind(privacy_level)
        .fetch_all(pool)
        .await?;

        let album_titles: Vec<String> = sqlx::query_scalar(
            "SELECT title FROM albums WHERE is_private <= ?1 AND title IS NOT NULL AND title != ''"
        )
        .bind(privacy_level)
        .fetch_all(pool)
        .await?;

        Ok(asset_folders.into_iter().chain(album_titles).collect())
    }
}