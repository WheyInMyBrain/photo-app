use sqlx::{Result, Row, SqlitePool};

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubAlbumRecord {
    pub name: String,
    pub full_path: String,
    pub media_count: i64,
    pub cover_thumb: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlbumRecord {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub description: Option<String>,
    pub album_type: String,
    pub cover_asset_id: Option<String>,
    pub cover_thumb: Option<String>,
    pub media_count: i64,
    pub filter_criteria: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct AlbumRepo;

impl AlbumRepo {
    // =======================================================================
    // 1. Virtual Folder Navigation (Physical folders on disk)
    // =======================================================================

    /// Retrieve sub-folders within `current_path` for a specific user
    pub async fn get_sub_albums(
        pool: &SqlitePool,
        user_id: &str,
        current_path: &str,
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
                WHERE user_id = ?1 AND deleted_at IS NULL AND folder_path != 'root' AND folder_path != ''
                ORDER BY album_name ASC
                "#,
            )
            .bind(user_id)
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
                WHERE user_id = ?2 AND deleted_at IS NULL AND folder_path LIKE ?3 || '/%'
                ORDER BY album_name ASC
                "#,
            )
            .bind(prefix_len)
            .bind(user_id)
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
                        WHERE user_id = ?1 AND deleted_at IS NULL AND (folder_path = ?2 OR folder_path LIKE ?2 || '/%')
                        ORDER BY captured_at DESC, created_at DESC 
                        LIMIT 1
                    ) as cover_thumb
                FROM assets
                WHERE user_id = ?1 AND deleted_at IS NULL AND (folder_path = ?2 OR folder_path LIKE ?2 || '/%')
                "#,
            )
            .bind(user_id)
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

    /// Autocomplete suggestions for folder paths and album titles
    pub async fn get_all_folder_paths(pool: &SqlitePool, user_id: &str) -> Result<Vec<String>> {
        let asset_folders: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT folder_path FROM assets WHERE user_id = ?1 AND deleted_at IS NULL AND folder_path != ''"
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let album_titles: Vec<String> = sqlx::query_scalar(
            "SELECT title FROM albums WHERE user_id = ?1"
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(asset_folders.into_iter().chain(album_titles).collect())
    }

    // =======================================================================
    // 2. Custom & Smart Albums (Using `albums` & `album_assets` tables)
    // =======================================================================

    /// List all custom/smart albums for a user with dynamic cover resolution
    pub async fn list_custom_albums(pool: &SqlitePool, user_id: &str) -> Result<Vec<AlbumRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                a.id,
                a.user_id,
                a.title,
                a.description,
                a.album_type,
                a.cover_asset_id,
                a.filter_criteria,
                a.created_at,
                a.updated_at,
                (SELECT COUNT(*) FROM album_assets aa WHERE aa.album_id = a.id) AS media_count,
                COALESCE(
                    (SELECT thumb_path FROM assets WHERE id = a.cover_asset_id AND deleted_at IS NULL),
                    (
                        SELECT sub_a.thumb_path 
                        FROM album_assets sub_aa
                        JOIN assets sub_a ON sub_aa.asset_id = sub_a.id
                        WHERE sub_aa.album_id = a.id AND sub_a.deleted_at IS NULL
                        ORDER BY sub_aa.position ASC, sub_aa.added_at DESC
                        LIMIT 1
                    )
                ) AS cover_thumb
            FROM albums a
            WHERE a.user_id = ?1
            ORDER BY a.created_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let mut albums = Vec::with_capacity(rows.len());
        for r in rows {
            albums.push(AlbumRecord {
                id: r.get("id"),
                user_id: r.get("user_id"),
                title: r.get("title"),
                description: r.get("description"),
                album_type: r.get("album_type"),
                cover_asset_id: r.get("cover_asset_id"),
                cover_thumb: r.get("cover_thumb"),
                media_count: r.get("media_count"),
                filter_criteria: r.get("filter_criteria"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            });
        }

        Ok(albums)
    }

    /// Fetch a single album by ID
    pub async fn get_album_by_id(
        pool: &SqlitePool,
        album_id: &str,
        user_id: &str,
    ) -> Result<Option<AlbumRecord>> {
        let row = sqlx::query(
            r#"
            SELECT 
                a.id,
                a.user_id,
                a.title,
                a.description,
                a.album_type,
                a.cover_asset_id,
                a.filter_criteria,
                a.created_at,
                a.updated_at,
                (SELECT COUNT(*) FROM album_assets aa WHERE aa.album_id = a.id) AS media_count,
                COALESCE(
                    (SELECT thumb_path FROM assets WHERE id = a.cover_asset_id AND deleted_at IS NULL),
                    (
                        SELECT sub_a.thumb_path 
                        FROM album_assets sub_aa
                        JOIN assets sub_a ON sub_aa.asset_id = sub_a.id
                        WHERE sub_aa.album_id = a.id AND sub_a.deleted_at IS NULL
                        ORDER BY sub_aa.position ASC, sub_aa.added_at DESC
                        LIMIT 1
                    )
                ) AS cover_thumb
            FROM albums a
            WHERE a.id = ?1 AND a.user_id = ?2
            LIMIT 1
            "#,
        )
        .bind(album_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        Ok(match row {
            Some(r) => Some(AlbumRecord {
                id: r.try_get("id")?,
                user_id: r.try_get("user_id")?,
                title: r.try_get("title")?,
                description: r.try_get("description")?,
                album_type: r.try_get("album_type")?,
                cover_asset_id: r.try_get("cover_asset_id")?,
                cover_thumb: r.try_get("cover_thumb")?,
                media_count: r.try_get("media_count")?,
                filter_criteria: r.try_get("filter_criteria")?,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
            }),
            None => None,
        })
    }

    /// Create a custom or smart album.
    pub async fn create_album(
        pool: &SqlitePool,
        user_id: &str,
        raw_title: &str,
        description: Option<&str>,
        album_type: &str,
        filter_criteria: Option<&str>,
    ) -> Result<String> {
        let cleaned = raw_title.trim().trim_matches('/');
        if cleaned.is_empty() {
            return Err(sqlx::Error::Protocol("Album title cannot be empty".into()));
        }

        let segments: Vec<&str> = cleaned.split('/').filter(|s| !s.is_empty()).collect();
        let mut tx = pool.begin().await?;
        let mut leaf_album_id = String::new();

        for (idx, seg) in segments.iter().enumerate() {
            let is_leaf = idx == segments.len() - 1;
            let album_title = *seg;

            // Check if album with this name already exists for this user
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT id FROM albums WHERE user_id = ?1 AND title = ?2 LIMIT 1"
            )
            .bind(user_id)
            .bind(album_title)
            .fetch_optional(&mut *tx)
            .await?;

            let current_id = match existing {
                Some(id) => {
                    // If leaf node, optionally update description or metadata
                    if is_leaf && description.is_some() {
                        sqlx::query(
                            "UPDATE albums SET description = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2"
                        )
                        .bind(description)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await?;
                    }
                    id
                }
                None => {
                    let new_id = uuid::Uuid::new_v4().to_string();
                    let desc = if is_leaf { description } else { None };
                    let crit = if is_leaf { filter_criteria } else { None };

                    sqlx::query(
                        r#"
                        INSERT INTO albums (id, user_id, title, description, album_type, filter_criteria)
                        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                        "#,
                    )
                    .bind(&new_id)
                    .bind(user_id)
                    .bind(album_title)
                    .bind(desc)
                    .bind(album_type)
                    .bind(crit)
                    .execute(&mut *tx)
                    .await?;

                    new_id
                }
            };

            if is_leaf {
                leaf_album_id = current_id;
            }
        }

        tx.commit().await?;
        Ok(leaf_album_id)
    }

    /// Add assets into an album
    pub async fn add_assets(
        pool: &SqlitePool,
        album_id: &str,
        user_id: &str,
        asset_ids: &[String],
    ) -> Result<usize> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM albums WHERE id = ?1 AND user_id = ?2)"
        )
        .bind(album_id)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        if !exists {
            return Ok(0);
        }

        let mut tx = pool.begin().await?;
        let mut added = 0;

        for (idx, asset_id) in asset_ids.iter().enumerate() {
            let res = sqlx::query(
                r#"
                INSERT INTO album_assets (album_id, asset_id, position)
                SELECT ?1, id, ?2 FROM assets WHERE id = ?3 AND user_id = ?4 AND deleted_at IS NULL
                ON CONFLICT(album_id, asset_id) DO NOTHING
                "#,
            )
            .bind(album_id)
            .bind(idx as i32)
            .bind(asset_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

            if res.rows_affected() > 0 {
                added += 1;
            }
        }

        tx.commit().await?;
        Ok(added)
    }

    /// Remove assets from an album
    pub async fn remove_assets(
        pool: &SqlitePool,
        album_id: &str,
        user_id: &str,
        asset_ids: &[String],
    ) -> Result<usize> {
        let mut tx = pool.begin().await?;
        let mut removed = 0;

        for asset_id in asset_ids {
            let res = sqlx::query(
                r#"
                DELETE FROM album_assets 
                WHERE album_id = ?1 
                  AND asset_id = ?2 
                  AND album_id IN (SELECT id FROM albums WHERE id = ?1 AND user_id = ?3)
                "#,
            )
            .bind(album_id)
            .bind(asset_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

            removed += res.rows_affected();
        }

        tx.commit().await?;
        Ok(removed as usize)
    }

    /// Delete an album (cascade removes join rows)
    pub async fn delete_album(pool: &SqlitePool, album_id: &str, user_id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM albums WHERE id = ?1 AND user_id = ?2")
            .bind(album_id)
            .bind(user_id)
            .execute(pool)
            .await?;

        Ok(res.rows_affected() > 0)
    }

    /// Set an explicit cover photo for an album
    pub async fn set_cover(
        pool: &SqlitePool,
        album_id: &str,
        user_id: &str,
        asset_id: Option<&str>,
    ) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE albums SET cover_asset_id = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2 AND user_id = ?3"
        )
        .bind(asset_id)
        .bind(album_id)
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(res.rows_affected() > 0)
    }

    pub async fn link_asset_to_folder_albums_tx(
        tx: &mut sqlx::SqliteConnection,
        user_id: &str,
        asset_id: &str,
        folder_path: &str,
    ) -> Result<(), sqlx::Error> {
        let trimmed = folder_path.trim().trim_matches('/');
        if trimmed.is_empty() || trimmed == "root" {
            return Ok(());
        }

        // Split "vacation/2026/japan" -> creates/links "vacation", "vacation/2026", "vacation/2026/japan"
        // (or simply the full folder name and its leaf node)
        let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
        let mut accumulated_path = String::new();

        for (idx, seg) in segments.iter().enumerate() {
            if idx > 0 {
                accumulated_path.push('/');
            }
            accumulated_path.push_str(seg);

            // We use the segment name or accumulated path as the album title
            let album_title = seg;

            // 1. Fetch or create the album row for this user
            let album_id: String = match sqlx::query_scalar::<_, String>(
                "SELECT id FROM albums WHERE user_id = ?1 AND title = ?2 LIMIT 1"
            )
            .bind(user_id)
            .bind(album_title)
            .fetch_optional(&mut *tx)
            .await?
            {
                Some(id) => id,
                None => {
                    let new_id = uuid::Uuid::new_v4().to_string();
                    sqlx::query(
                        r#"
                        INSERT INTO albums (id, user_id, title, album_type)
                        VALUES (?1, ?2, ?3, 'MANUAL')
                        "#,
                    )
                    .bind(&new_id)
                    .bind(user_id)
                    .bind(album_title)
                    .execute(&mut *tx)
                    .await?;
                    new_id
                }
            };

            // 2. Link this asset into album_assets
            sqlx::query(
                r#"
                INSERT INTO album_assets (album_id, asset_id, position)
                VALUES (?1, ?2, 0)
                ON CONFLICT(album_id, asset_id) DO NOTHING
                "#,
            )
            .bind(&album_id)
            .bind(asset_id)
            .execute(&mut *tx)
            .await?;
        }

        Ok(())
    }
}