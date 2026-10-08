use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAlbumRecord {
    pub name: String,
    pub full_path: String,
    pub media_count: i64,
    pub cover_thumb: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub album_type: String,
    pub cover_asset_id: Option<Uuid>,
    pub cover_thumb: Option<String>,
    pub media_count: i64,
    pub filter_criteria: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct AlbumRepo;

impl AlbumRepo {
    // =======================================================================
    // 1. Virtual Folder Navigation (Disk-backed path explorer)
    // =======================================================================

    /// Retrieves all immediate sub-folders under `current_path` in ONE query, sorted alphabetically.
    pub async fn get_sub_albums(
        pool: &PgPool,
        user_id: Uuid,
        current_path: &str,
    ) -> Result<Vec<SubAlbumRecord>, sqlx::Error> {
        let clean_path = current_path.trim().trim_matches('/');

        let rows = if clean_path.is_empty() {
            // Root level: Extract first path component before '/'
            sqlx::query(
                r#"
                WITH direct_folders AS (
                    SELECT 
                        SPLIT_PART(folder_path, '/', 1) AS album_name,
                        thumb_path,
                        captured_at,
                        created_at
                    FROM assets
                    WHERE user_id = $1 
                    AND deleted_at IS NULL 
                    AND folder_path <> '' 
                    AND folder_path <> 'root'
                ),
                ranked_covers AS (
                    SELECT 
                        album_name,
                        COUNT(*)::bigint AS count,
                        (
                            ARRAY_AGG(thumb_path ORDER BY captured_at DESC NULLS LAST, created_at DESC)
                        )[1] AS cover_thumb
                    FROM direct_folders
                    WHERE album_name <> ''
                    GROUP BY album_name
                )
                SELECT album_name, count, cover_thumb
                FROM ranked_covers
                ORDER BY LOWER(album_name) ASC, album_name ASC;
                "#,
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?
        } else {
            // Sub-level: Find immediate child segment directly following the prefix
            let prefix_pattern = format!("{}/%", clean_path);
            let prefix_depth = clean_path.split('/').count() + 1;

            sqlx::query(
                r#"
                WITH direct_folders AS (
                    SELECT 
                        SPLIT_PART(folder_path, '/', $3::int) AS album_name,
                        thumb_path,
                        captured_at,
                        created_at
                    FROM assets
                    WHERE user_id = $1 
                    AND deleted_at IS NULL 
                    AND folder_path LIKE $2
                ),
                ranked_covers AS (
                    SELECT 
                        album_name,
                        COUNT(*)::bigint AS count,
                        (
                            ARRAY_AGG(thumb_path ORDER BY captured_at DESC NULLS LAST, created_at DESC)
                        )[1] AS cover_thumb
                    FROM direct_folders
                    WHERE album_name <> ''
                    GROUP BY album_name
                )
                SELECT album_name, count, cover_thumb
                FROM ranked_covers
                ORDER BY LOWER(album_name) ASC, album_name ASC;
                "#,
            )
            .bind(user_id)
            .bind(prefix_pattern)
            .bind(prefix_depth as i32)
            .fetch_all(pool)
            .await?
        };

        let records = rows
            .into_iter()
            .map(|r| {
                let name: String = r.get("album_name");
                let full_path = if clean_path.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", clean_path, name)
                };

                SubAlbumRecord {
                    name,
                    full_path,
                    media_count: r.get("count"),
                    cover_thumb: r.get("cover_thumb"),
                }
            })
            .collect();

        Ok(records)
    }

    /// Autocomplete suggestions for folder paths and album titles
    pub async fn get_all_folder_paths(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<String>, sqlx::Error> {
        let asset_folders: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT folder_path 
            FROM assets 
            WHERE user_id = $1 AND deleted_at IS NULL AND folder_path <> ''
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let album_titles: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT title 
            FROM albums 
            WHERE user_id = $1 AND deleted_at IS NULL
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(asset_folders.into_iter().chain(album_titles).collect())
    }

    // =======================================================================
    // 2. Custom & Smart Albums
    // =======================================================================

    /// List all custom/smart albums for a user with lateral cover extraction
    pub async fn list_custom_albums(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<AlbumRecord>, sqlx::Error> {
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
                COALESCE(counts.media_count, 0)::bigint AS media_count,
                COALESCE(c_explicit.thumb_path, c_fallback.thumb_path) AS cover_thumb
            FROM albums a
            -- Fast Media Count Aggregation
            LEFT JOIN LATERAL (
                SELECT COUNT(*)::bigint AS media_count
                FROM album_assets aa
                JOIN assets ast ON aa.asset_id = ast.id
                WHERE aa.album_id = a.id AND ast.deleted_at IS NULL
            ) counts ON TRUE
            -- Explicit Cover Lookup
            LEFT JOIN assets c_explicit 
                ON a.cover_asset_id = c_explicit.id AND c_explicit.deleted_at IS NULL
            -- Fallback Cover Lookup (Latest Asset in Album)
            LEFT JOIN LATERAL (
                SELECT sub_a.thumb_path
                FROM album_assets sub_aa
                JOIN assets sub_a ON sub_aa.asset_id = sub_a.id
                WHERE sub_aa.album_id = a.id AND sub_a.deleted_at IS NULL
                ORDER BY sub_aa.position ASC, sub_aa.added_at DESC
                LIMIT 1
            ) c_fallback ON TRUE
            WHERE a.user_id = $1 AND a.deleted_at IS NULL
            ORDER BY a.created_at DESC;
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let albums = rows
            .into_iter()
            .map(|r| AlbumRecord {
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
            })
            .collect();

        Ok(albums)
    }

    /// Fetch a single album by ID
    pub async fn get_album_by_id(
        pool: &PgPool,
        album_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<AlbumRecord>, sqlx::Error> {
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
                COALESCE(counts.media_count, 0)::bigint AS media_count,
                COALESCE(c_explicit.thumb_path, c_fallback.thumb_path) AS cover_thumb
            FROM albums a
            LEFT JOIN LATERAL (
                SELECT COUNT(*)::bigint AS media_count
                FROM album_assets aa
                JOIN assets ast ON aa.asset_id = ast.id
                WHERE aa.album_id = a.id AND ast.deleted_at IS NULL
            ) counts ON TRUE
            LEFT JOIN assets c_explicit 
                ON a.cover_asset_id = c_explicit.id AND c_explicit.deleted_at IS NULL
            LEFT JOIN LATERAL (
                SELECT sub_a.thumb_path
                FROM album_assets sub_aa
                JOIN assets sub_a ON sub_aa.asset_id = sub_a.id
                WHERE sub_aa.album_id = a.id AND sub_a.deleted_at IS NULL
                ORDER BY sub_aa.position ASC, sub_aa.added_at DESC
                LIMIT 1
            ) c_fallback ON TRUE
            WHERE a.id = $1 AND a.user_id = $2 AND a.deleted_at IS NULL;
            "#,
        )
        .bind(album_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| AlbumRecord {
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
        }))
    }

    /// Create a custom or smart album with hierarchical support
    pub async fn create_album(
        pool: &PgPool,
        user_id: Uuid,
        raw_title: &str,
        description: Option<&str>,
        album_type: &str,
        filter_criteria: Option<serde_json::Value>,
    ) -> Result<Uuid, sqlx::Error> {
        let cleaned = raw_title.trim().trim_matches('/');
        if cleaned.is_empty() {
            return Err(sqlx::Error::Protocol("Album title cannot be empty".into()));
        }

        let segments: Vec<&str> = cleaned.split('/').filter(|s| !s.is_empty()).collect();
        let mut tx = pool.begin().await?;
        let mut leaf_album_id = Uuid::nil();
        let mut accumulated_path = String::new();

        for (idx, seg) in segments.iter().enumerate() {
            if idx > 0 {
                accumulated_path.push('/');
            }
            accumulated_path.push_str(seg);

            let is_leaf = idx == segments.len() - 1;
            let album_title = &accumulated_path;

            let existing: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM albums WHERE user_id = $1 AND title = $2 LIMIT 1",
            )
            .bind(user_id)
            .bind(album_title)
            .fetch_optional(&mut *tx)
            .await?;

            let current_id = match existing {
                Some(id) => {
                    if is_leaf && description.is_some() {
                        sqlx::query(
                            r#"
                            UPDATE albums 
                            SET description = $1, updated_at = CURRENT_TIMESTAMP 
                            WHERE id = $2
                            "#,
                        )
                        .bind(description)
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                    }
                    id
                }
                None => {
                    let desc = if is_leaf { description } else { None };
                    let crit = if is_leaf { filter_criteria.clone() } else { None };

                    let inserted_id: Uuid = sqlx::query_scalar(
                        r#"
                        INSERT INTO albums (user_id, title, description, album_type, filter_criteria)
                        VALUES ($1, $2, $3, $4, $5)
                        RETURNING id
                        "#,
                    )
                    .bind(user_id)
                    .bind(album_title)
                    .bind(desc)
                    .bind(album_type)
                    .bind(crit)
                    .fetch_one(&mut *tx)
                    .await?;

                    inserted_id
                }
            };

            if is_leaf {
                leaf_album_id = current_id;
            }
        }

        tx.commit().await?;
        Ok(leaf_album_id)
    }

    /// Links an asset into all matching album folder paths recursively within an active transaction
    pub async fn link_asset_to_folder_albums_tx(
        tx: &mut PgConnection,
        user_id: Uuid,
        asset_id: Uuid,
        folder_path: &str,
    ) -> Result<(), sqlx::Error> {
        let trimmed = folder_path.trim().trim_matches('/');
        if trimmed.is_empty() || trimmed == "root" {
            return Ok(());
        }

        let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
        let mut accumulated_path = String::new();

        for (idx, seg) in segments.iter().enumerate() {
            if idx > 0 {
                accumulated_path.push('/');
            }
            accumulated_path.push_str(seg);

            let album_title = &accumulated_path;

            // Upsert album hierarchy
            let album_id: Uuid = sqlx::query_scalar(
                r#"
                INSERT INTO albums (user_id, title, album_type)
                VALUES ($1, $2, 'MANUAL')
                ON CONFLICT (user_id, title) DO UPDATE SET updated_at = CURRENT_TIMESTAMP
                RETURNING id
                "#,
            )
            .bind(user_id)
            .bind(album_title)
            .fetch_one(&mut *tx)
            .await?;

            // Link asset into album_assets
            sqlx::query(
                r#"
                INSERT INTO album_assets (album_id, asset_id, user_id, position)
                VALUES ($1, $2, $3, 0)
                ON CONFLICT (album_id, asset_id) DO NOTHING
                "#,
            )
            .bind(album_id)
            .bind(asset_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }

        Ok(())
    }

    /// Add assets into an album in a single vector operation
    pub async fn add_assets(
        pool: &PgPool,
        album_id: Uuid,
        user_id: Uuid,
        asset_ids: &[Uuid],
    ) -> Result<usize, sqlx::Error> {
        if asset_ids.is_empty() {
            return Ok(0);
        }

        // Verify ownership
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM albums WHERE id = $1 AND user_id = $2)",
        )
        .bind(album_id)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        if !exists {
            return Ok(0);
        }

        let res = sqlx::query(
            r#"
            INSERT INTO album_assets (album_id, asset_id, user_id, position)
            SELECT $1, a.id, $2, (ordinality - 1)::int
            FROM UNNEST($3::uuid[]) WITH ORDINALITY AS u(id, ordinality)
            JOIN assets a ON a.id = u.id AND a.user_id = $2 AND a.deleted_at IS NULL
            ON CONFLICT (album_id, asset_id) DO NOTHING
            "#,
        )
        .bind(album_id)
        .bind(user_id)
        .bind(asset_ids)
        .execute(pool)
        .await?;

        Ok(res.rows_affected() as usize)
    }

    /// Remove specific assets from an album in a single batch statement
    pub async fn remove_assets(
        pool: &PgPool,
        album_id: Uuid,
        user_id: Uuid,
        asset_ids: &[Uuid],
    ) -> Result<u64, sqlx::Error> {
        if asset_ids.is_empty() {
            return Ok(0);
        }

        let mut tx = pool.begin().await?;

        let res = sqlx::query(
            r#"
            DELETE FROM album_assets 
            WHERE album_id = $1 
              AND user_id = $2
              AND asset_id = ANY($3::uuid[])
            "#,
        )
        .bind(album_id)
        .bind(user_id)
        .bind(asset_ids)
        .execute(&mut *tx)
        .await?;

        // If the explicit cover was among the removed assets, nullify it
        sqlx::query(
            r#"
            UPDATE albums
            SET cover_asset_id = NULL
            WHERE id = $1 AND cover_asset_id = ANY($2::uuid[])
            "#,
        )
        .bind(album_id)
        .bind(asset_ids)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(res.rows_affected())
    }

    /// Set an explicit cover photo asset for an album
    pub async fn set_cover_asset(
        pool: &PgPool,
        user_id: Uuid,
        album_id: Uuid,
        asset_id: Option<Uuid>,
    ) -> Result<bool, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            UPDATE albums
            SET cover_asset_id = $1, updated_at = CURRENT_TIMESTAMP
            WHERE id = $2 AND user_id = $3
            "#,
        )
        .bind(asset_id)
        .bind(album_id)
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(rows.rows_affected() > 0)
    }

    pub async fn update_album(
        pool: &PgPool,
        user_id: Uuid,
        album_id: Uuid,
        title: &str,
        description: Option<&str>,
    ) -> Result<bool, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            UPDATE albums
            SET title = $1, description = $2, updated_at = CURRENT_TIMESTAMP
            WHERE id = $3 AND user_id = $4
            "#,
        )
        .bind(title.trim())
        .bind(description)
        .bind(album_id)
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(rows.rows_affected() > 0)
    }

    /// Reorder assets in an album by bulk writing index positions
    pub async fn reorder_album_assets(
        pool: &PgPool,
        user_id: Uuid,
        album_id: Uuid,
        ordered_asset_ids: &[Uuid],
    ) -> Result<(), sqlx::Error> {
        let is_owner: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM albums WHERE id = $1 AND user_id = $2)",
        )
        .bind(album_id)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        if !is_owner {
            return Err(sqlx::Error::RowNotFound);
        }

        // Single statement batch update using ORDINALITY
        sqlx::query(
            r#"
            UPDATE album_assets aa
            SET position = (u.ordinality - 1)::int
            FROM UNNEST($2::uuid[]) WITH ORDINALITY AS u(id, ordinality)
            WHERE aa.album_id = $1 AND aa.asset_id = u.id
            "#,
        )
        .bind(album_id)
        .bind(ordered_asset_ids)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Delete an album:
    /// - If `delete_media` is true: soft-deletes both assets and album container.
    /// - If `delete_media` is false: unlinks assets and permanently deletes the album container only.
    pub async fn delete_album(
        pool: &PgPool,
        user_id: Uuid,
        album_id: Uuid,
        delete_media: bool,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let owner_check: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM albums WHERE id = $1 AND user_id = $2",
        )
        .bind(album_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;

        if owner_check.is_none() {
            return Ok(false);
        }

        if delete_media {
            // Soft-delete all active media belonging to this album
            sqlx::query(
                r#"
                UPDATE assets
                SET deleted_at = CURRENT_TIMESTAMP
                WHERE user_id = $1 
                  AND deleted_at IS NULL
                  AND id IN (SELECT asset_id FROM album_assets WHERE album_id = $2)
                "#,
            )
            .bind(user_id)
            .bind(album_id)
            .execute(&mut *tx)
            .await?;

            // Soft-delete the album container
            let res = sqlx::query(
                r#"
                UPDATE albums 
                SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP 
                WHERE id = $1 AND user_id = $2
                "#,
            )
            .bind(album_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            Ok(res.rows_affected() > 0)
        } else {
            // Hard-delete album container (CASCADE cleans up album_assets automatically)
            let res = sqlx::query("DELETE FROM albums WHERE id = $1 AND user_id = $2")
                .bind(album_id)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            Ok(res.rows_affected() > 0)
        }
    }

    /// Auto-purge trashed albums that no longer contain any linked assets
    pub async fn cleanup_empty_trashed_albums(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<u64, sqlx::Error> {
        let res = sqlx::query(
            r#"
            DELETE FROM albums a
            WHERE a.user_id = $1
              AND a.deleted_at IS NOT NULL
              AND NOT EXISTS (
                  SELECT 1 FROM album_assets aa WHERE aa.album_id = a.id
              )
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(res.rows_affected())
    }

    /// Auto-purge trashed albums that have either no remaining assets or are older than 30 days
    pub async fn cleanup_all_expired_and_empty_trashed_albums(
        pool: &PgPool,
    ) -> Result<u64, sqlx::Error> {
        let res = sqlx::query(
            r#"
            DELETE FROM albums a
            WHERE a.deleted_at IS NOT NULL 
              AND (
                  NOT EXISTS (SELECT 1 FROM album_assets aa WHERE aa.album_id = a.id)
                  OR a.deleted_at <= (CURRENT_TIMESTAMP - INTERVAL '30 days')
              )
            "#,
        )
        .execute(pool)
        .await?;

        Ok(res.rows_affected())
    }
}