use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::domain::media::{
    AssetStorageInfo, MediaPageResponse, MediaQuery, MediaSummary, NewAssetRecord, DynamicFiltersResponse, FilterOption, SubAlbum,
};

pub struct AssetRepo;

impl AssetRepo {
    pub async fn query_media(
        pool: &SqlitePool,
        q: &MediaQuery,
    ) -> Result<MediaPageResponse, sqlx::Error> {
        let limit = q.limit.unwrap_or(50).clamp(1, 200);
        let fetch_limit = limit + 1;
        let privacy_level = if q.is_private.unwrap_or(false) { 1 } else { 0 };

        let mut builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT \
                a.id, a.file_name, a.thumb_path, a.preview_path, \
                a.aspect_ratio, a.duration_seconds, a.mime_type, a.captured_at, \
                a.is_favorite \
            FROM assets a "
        );

        let fts_query = q.q.as_deref().and_then(Self::sanitize_query);
        if fts_query.is_some() {
            builder.push(" JOIN asset_search_index fts ON fts.asset_id = a.id ");
        }

        builder.push(" WHERE a.is_private = ");
        builder.push_bind(privacy_level);

        if let Some(ref expr) = fts_query {
            builder.push(" AND fts.is_private = ");
            builder.push_bind(privacy_level);
            builder.push(" AND asset_search_index MATCH ");
            builder.push_bind(expr);
        }

        // --- PHOTO vs VIDEO SEPARATION ---
        if let Some(ref m_type) = q.media_type {
            match m_type.as_str() {
                "photos" => { builder.push(" AND a.duration_seconds IS NULL "); }
                "videos" => { builder.push(" AND a.duration_seconds IS NOT NULL "); }
                _ => {}
            }
        }

        if let Some(fav) = q.is_favorite {
            builder.push(" AND a.is_favorite = ");
            builder.push_bind(if fav { 1 } else { 0 });
        }

        // --- ROBUST MULTI-PERSON ---
        if let Some(ref pids_str) = q.person_id {
            let pids: Vec<&str> = pids_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            let count = pids.len() as i64;
            if count > 0 {
                builder.push(" AND a.id IN (SELECT asset_id FROM asset_faces WHERE person_id IN (");
                let mut sep = builder.separated(", ");
                for pid in &pids { sep.push_bind(pid); }
                sep.push_unseparated(") GROUP BY asset_id HAVING COUNT(DISTINCT person_id) = ");
                builder.push_bind(count);
                builder.push(") ");
            }
        }

        // --- ROBUST MULTI-TAG ---
        if let Some(ref tags_str) = q.tag {
            let tags: Vec<String> = tags_str.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
            let count = tags.len() as i64;
            if count > 0 {
                builder.push(" AND a.id IN (SELECT at.asset_id FROM asset_tags at JOIN tags t ON at.tag_id = t.id WHERE LOWER(t.name) IN (");
                let mut sep = builder.separated(", ");
                for t in &tags { sep.push_bind(t); }
                sep.push_unseparated(") GROUP BY at.asset_id HAVING COUNT(DISTINCT LOWER(t.name)) = ");
                builder.push_bind(count);
                builder.push(") ");
            }
        }

        if let Some(ref folder) = q.folder_path {
            builder.push(" AND a.folder_path = ");
            builder.push_bind(folder);
        }
        if let Some(ref city) = q.city {
            builder.push(" AND a.city = ");
            builder.push_bind(city);
        }
        if let Some(ref model) = q.camera_model {
            builder.push(" AND a.camera_model = ");
            builder.push_bind(model);
        }
        if let Some(ref from_date) = q.from {
            builder.push(" AND a.captured_at >= ");
            builder.push_bind(from_date);
        }
        if let Some(ref to_date) = q.to {
            builder.push(" AND a.captured_at <= ");
            builder.push_bind(to_date);
        }

        // Keyset pagination
        if let (Some(cat), Some(cid)) = (&q.cursor_captured_at, &q.cursor_id) {
            builder.push(" AND (a.captured_at < ");
            builder.push_bind(cat);
            builder.push(" OR (a.captured_at = ");
            builder.push_bind(cat);
            builder.push(" AND a.id < ");
            builder.push_bind(cid);
            builder.push(")) ");
        }

        if fts_query.is_some() {
            builder.push(" ORDER BY fts.rank, a.captured_at DESC, a.created_at DESC, a.id DESC LIMIT ");
        } else {
            builder.push(" ORDER BY a.captured_at DESC, a.created_at DESC, a.id DESC LIMIT ");
        }
        builder.push_bind(fetch_limit);

        // Sub-album retrieval now passes media_type for strict separation
        let albums = if q.cursor_id.is_none() {
            let curr = q.folder_path.as_deref().unwrap_or("");
            Self::get_sub_albums(pool, curr, q.media_type.as_deref(), privacy_level).await.unwrap_or_default()
        } else {
            Vec::new()
        };

        let mut items = builder.build_query_as::<MediaSummary>().fetch_all(pool).await?;
        let has_more = items.len() as i64 > limit;
        if has_more { items.pop(); }

        let next_cursor_captured_at = items.last().and_then(|i| i.captured_at.clone());
        let next_cursor_id = items.last().map(|i| i.id.clone());

        Ok(MediaPageResponse {
            albums,
            items,
            next_cursor_captured_at,
            next_cursor_id,
            has_more,
        })
    }

    pub async fn get_dynamic_filters(
        pool: &SqlitePool,
        q: &MediaQuery,
    ) -> Result<DynamicFiltersResponse, sqlx::Error> {
        let privacy_level = if q.is_private.unwrap_or(false) { 1 } else { 0 };

        let mut builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT a.id FROM assets a "
        );

        let fts_query = q.q.as_deref().and_then(Self::sanitize_query);
        if fts_query.is_some() {
            builder.push(" JOIN asset_search_index fts ON fts.asset_id = a.id ");
        }

        builder.push(" WHERE a.is_private = ");
        builder.push_bind(privacy_level);

        if let Some(ref expr) = fts_query {
            builder.push(" AND fts.is_private = ");
            builder.push_bind(privacy_level);
            builder.push(" AND asset_search_index MATCH ");
            builder.push_bind(expr);
        }

        // Photo vs Video filter
        if let Some(ref m_type) = q.media_type {
            match m_type.as_str() {
                "photos" => { builder.push(" AND a.duration_seconds IS NULL "); }
                "videos" => { builder.push(" AND a.duration_seconds IS NOT NULL "); }
                _ => {}
            }
        }
        if let Some(fav) = q.is_favorite {
            builder.push(" AND a.is_favorite = ");
            builder.push_bind(if fav { 1 } else { 0 });
        }

        // Multi-Person
        if let Some(ref pids_str) = q.person_id {
            let pids: Vec<&str> = pids_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            let count = pids.len() as i64;
            if count > 0 {
                builder.push(" AND a.id IN (SELECT asset_id FROM asset_faces WHERE person_id IN (");
                let mut sep = builder.separated(", ");
                for pid in &pids { sep.push_bind(pid); }
                sep.push_unseparated(") GROUP BY asset_id HAVING COUNT(DISTINCT person_id) = ");
                builder.push_bind(count);
                builder.push(") ");
            }
        }

        // Multi-Tag
        if let Some(ref tags_str) = q.tag {
            let tags: Vec<String> = tags_str.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
            let count = tags.len() as i64;
            if count > 0 {
                builder.push(" AND a.id IN (SELECT at.asset_id FROM asset_tags at JOIN tags t ON at.tag_id = t.id WHERE LOWER(t.name) IN (");
                let mut sep = builder.separated(", ");
                for t in &tags { sep.push_bind(t); }
                sep.push_unseparated(") GROUP BY at.asset_id HAVING COUNT(DISTINCT LOWER(t.name)) = ");
                builder.push_bind(count);
                builder.push(") ");
            }
        }

        // Folder Path
        if let Some(ref folder) = q.folder_path {
            builder.push(" AND a.folder_path = ");
            builder.push_bind(folder);
        }
        if let Some(ref city) = q.city {
            builder.push(" AND a.city = ");
            builder.push_bind(city);
        }
        if let Some(ref model) = q.camera_model {
            builder.push(" AND a.camera_model = ");
            builder.push_bind(model);
        }
        if let Some(ref from_date) = q.from {
            builder.push(" AND a.captured_at >= ");
            builder.push_bind(from_date);
        }
        if let Some(ref to_date) = q.to {
            builder.push(" AND a.captured_at <= ");
            builder.push_bind(to_date);
        }

        let matching_ids: Vec<String> = builder.build_query_scalar::<String>().fetch_all(pool).await?;

        if matching_ids.is_empty() {
            return Ok(DynamicFiltersResponse::default());
        }

        let total_media = matching_ids.len() as i64;

        // Photo / Video Breakdown & Date range
        let mut stats_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            r#"
            SELECT 
                COUNT(CASE WHEN duration_seconds IS NULL THEN 1 END) as p_count,
                COUNT(CASE WHEN duration_seconds IS NOT NULL THEN 1 END) as v_count,
                MIN(captured_at) as min_d, 
                MAX(captured_at) as max_d 
            FROM assets 
            WHERE id IN (
            "#
        );
        let mut sep = stats_builder.separated(", ");
        for id in &matching_ids { sep.push_bind(id); }
        sep.push_unseparated(")");

        let stats_row = stats_builder.build().fetch_one(pool).await?;
        let photos_count: i64 = stats_row.try_get("p_count").unwrap_or(0);
        let videos_count: i64 = stats_row.try_get("v_count").unwrap_or(0);
        let min_date: Option<String> = stats_row.try_get("min_d").ok();
        let max_date: Option<String> = stats_row.try_get("max_d").ok();

        // Time of day
        let mut tod_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            r#"
            SELECT 
                CASE 
                    WHEN hour BETWEEN 5 AND 11 THEN 'Morning'
                    WHEN hour BETWEEN 12 AND 16 THEN 'Afternoon'
                    WHEN hour BETWEEN 17 AND 20 THEN 'Evening'
                    ELSE 'Night'
                END as period,
                COUNT(*) as count
            FROM assets
            WHERE hour IS NOT NULL AND id IN (
            "#
        );
        let mut sep = tod_builder.separated(", ");
        for id in &matching_ids { sep.push_bind(id); }
        sep.push_unseparated(") GROUP BY period ORDER BY count DESC");

        let times_of_day = tod_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get::<String, _>("period").to_lowercase(),
                label: r.get("period"),
                count: r.get("count"),
            })
            .collect();

        // People
        let mut people_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            r#"
            SELECT p.id, p.name, COUNT(DISTINCT af.asset_id) as count
            FROM asset_faces af
            JOIN persons p ON af.person_id = p.id
            WHERE p.name IS NOT NULL AND af.asset_id IN (
            "#
        );
        let mut sep = people_builder.separated(", ");
        for id in &matching_ids { sep.push_bind(id); }
        sep.push_unseparated(") GROUP BY p.id ORDER BY count DESC LIMIT 30");

        let people = people_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("id"),
                label: r.get::<Option<String>, _>("name").unwrap_or_else(|| "Unnamed".into()),
                count: r.get("count"),
            })
            .collect();

        // Tags
        let mut tags_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            r#"
            SELECT t.name, COUNT(DISTINCT at.asset_id) as count
            FROM asset_tags at
            JOIN tags t ON at.tag_id = t.id
            WHERE at.asset_id IN (
            "#
        );
        let mut sep = tags_builder.separated(", ");
        for id in &matching_ids { sep.push_bind(id); }
        sep.push_unseparated(") GROUP BY t.id ORDER BY count DESC LIMIT 40");

        let tags = tags_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("name"),
                label: r.get("name"),
                count: r.get("count"),
            })
            .collect();

        // Locations
        let mut loc_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT city, COUNT(*) as count FROM assets WHERE city IS NOT NULL AND id IN ("
        );
        let mut sep = loc_builder.separated(", ");
        for id in &matching_ids { sep.push_bind(id); }
        sep.push_unseparated(") GROUP BY city ORDER BY count DESC LIMIT 20");

        let locations = loc_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("city"),
                label: r.get("city"),
                count: r.get("count"),
            })
            .collect();

        // Cameras
        let mut cam_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT camera_model, COUNT(*) as count FROM assets WHERE camera_model IS NOT NULL AND id IN ("
        );
        let mut sep = cam_builder.separated(", ");
        for id in &matching_ids { sep.push_bind(id); }
        sep.push_unseparated(") GROUP BY camera_model ORDER BY count DESC LIMIT 15");

        let cameras = cam_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("camera_model"),
                label: r.get("camera_model"),
                count: r.get("count"),
            })
            .collect();

        // Albums / Folders
        let mut album_builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT folder_path, COUNT(*) as count FROM assets WHERE folder_path IS NOT NULL AND folder_path != 'root' AND id IN ("
        );
        let mut sep = album_builder.separated(", ");
        for id in &matching_ids { sep.push_bind(id); }
        sep.push_unseparated(") GROUP BY folder_path ORDER BY count DESC LIMIT 20");

        let albums = album_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("folder_path"),
                label: r.get("folder_path"),
                count: r.get("count"),
            })
            .collect();

        Ok(DynamicFiltersResponse {
            total_media,
            photos_count,
            videos_count,
            min_date,
            max_date,
            times_of_day,
            people,
            tags,
            locations,
            cameras,
            albums,
        })
    }

    /// Tokenizes queries into FTS5 prefix expressions
    pub fn sanitize_query(raw: &str) -> Option<String> {
        let tokens: Vec<String> = raw
            .split_whitespace()
            .map(|s| {
                s.chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .collect::<String>()
                    .to_lowercase()
            })
            .filter(|s| !s.is_empty())
            .map(|s| format!("{}*", s))
            .collect();

        if tokens.is_empty() {
            None
        } else {
            Some(tokens.join(" AND "))
        }
    }

    /// Indexes or updates an asset's entry in the FTS5 search index
    /// Indexes or updates an asset's entry in the FTS5 search index
    pub async fn sync_search_index(pool: &SqlitePool, asset_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM asset_search_index WHERE asset_id = ?1")
            .bind(asset_id)
            .execute(pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO asset_search_index (asset_id, is_private, persons, tags, location, temporal, camera, file_name)
            SELECT 
                a.id,
                a.is_private,
                COALESCE((
                    SELECT GROUP_CONCAT(name, ' ')
                    FROM (
                        SELECT DISTINCT p.name AS name
                        FROM asset_faces af
                        JOIN persons p ON af.person_id = p.id
                        WHERE af.asset_id = a.id 
                          AND p.name IS NOT NULL 
                          AND TRIM(p.name) != ''
                    )
                ), '') AS persons,
                COALESCE((
                    SELECT GROUP_CONCAT(name, ' ')
                    FROM (
                        SELECT DISTINCT t.name AS name
                        FROM asset_tags at
                        JOIN tags t ON at.tag_id = t.id
                        WHERE at.asset_id = a.id
                    )
                ), '') AS tags,
                TRIM(COALESCE(a.city, '') || ' ' || COALESCE(a.subdivision, '') || ' ' || COALESCE(a.country, '')) AS location,
                TRIM(
                    COALESCE(CAST(a.year AS TEXT), '') || ' ' ||
                    COALESCE(SUBSTR(a.captured_at, 1, 10), '')
                ) AS temporal,
                TRIM(COALESCE(a.camera_make, '') || ' ' || COALESCE(a.camera_model, '') || ' ' || COALESCE(a.lens_model, '')) AS camera,
                a.file_name
            FROM assets a
            WHERE a.id = ?1
            "#,
        )
        .bind(asset_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn insert_asset(pool: &SqlitePool, a: &NewAssetRecord) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO assets (
                id, sha256, file_name, rel_path, folder_path,
                thumb_path, preview_path, file_size_bytes, mime_type,
                width, height, aspect_ratio, duration_seconds, is_private,
                captured_at, year, month, day, hour,
                latitude, longitude, altitude_meters,
                city, subdivision, country, country_code,
                camera_make, camera_model,
                face_processed, tags_processed
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, ?19,
                ?20, ?21, ?22,
                ?23, ?24, ?25, ?26,
                ?27, ?28,
                0, 0
            )
            "#,
        )
        .bind(&a.id)
        .bind(&a.sha256)
        .bind(&a.file_name)
        .bind(&a.rel_path)
        .bind(&a.folder_path)
        .bind(&a.thumb_path)
        .bind(&a.preview_path)
        .bind(a.file_size_bytes)
        .bind(&a.mime_type)
        .bind(a.width)
        .bind(a.height)
        .bind(a.aspect_ratio)
        .bind(a.duration_seconds)
        .bind(if a.is_private { 1 } else { 0 })
        .bind(&a.captured_at)
        .bind(a.year)
        .bind(a.month)
        .bind(a.day)
        .bind(a.hour)
        .bind(a.latitude)
        .bind(a.longitude)
        .bind(a.altitude)
        .bind(&a.city)
        .bind(&a.subdivision)
        .bind(&a.country)
        .bind(&a.country_code)
        .bind(&a.camera_make)
        .bind(&a.camera_model)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn toggle_favorite(pool: &SqlitePool, asset_id: &str) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT is_favorite FROM assets WHERE id = ?1")
            .bind(asset_id)
            .fetch_one(pool)
            .await?;

        let current: i64 = row.try_get("is_favorite").unwrap_or(0);
        let new_state = if current == 1 { 0 } else { 1 };

        sqlx::query("UPDATE assets SET is_favorite = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2")
            .bind(new_state)
            .bind(asset_id)
            .execute(pool)
            .await?;

        Ok(new_state == 1)
    }

    pub async fn find_id_by_sha256(
        pool: &SqlitePool,
        sha256: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT id FROM assets WHERE sha256 = ?1")
            .bind(sha256)
            .fetch_optional(pool)
            .await
    }

    pub async fn get_storage_info(
        pool: &SqlitePool,
        asset_id: &str,
    ) -> Result<Option<AssetStorageInfo>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT rel_path, preview_path, is_private, (duration_seconds IS NOT NULL) AS is_video FROM assets WHERE id = ?1"
        )
        .bind(asset_id)
        .fetch_optional(pool)
        .await?;

        let info = row.map(|r| AssetStorageInfo {
            rel_path: r.try_get("rel_path").unwrap_or_default(),
            preview_path: r.try_get("preview_path").unwrap_or_default(),
            is_private: r.try_get::<i64, _>("is_private").unwrap_or(0) == 1,
            is_video: r.try_get::<bool, _>("is_video").unwrap_or(false),
        });

        Ok(info)
    }

    /// Discovers immediate child folders under a given path
    pub async fn get_sub_albums(
        pool: &SqlitePool,
        current_folder: &str,
        media_type: Option<&str>,
        privacy_level: i64,
    ) -> Result<Vec<SubAlbum>, sqlx::Error> {
        let prefix = if current_folder.is_empty() || current_folder == "root" {
            String::new()
        } else {
            format!("{}/", current_folder.trim_matches('/'))
        };

        let mut builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            r#"
            SELECT folder_path, thumb_path
            FROM assets
            WHERE folder_path LIKE 
            "#
        );
        builder.push_bind(format!("{}%", prefix));
        builder.push(" AND folder_path != ");
        builder.push_bind(if current_folder.is_empty() { "root" } else { current_folder });
        builder.push(" AND is_private = ");
        builder.push_bind(privacy_level);

        // Photo / Video separation in album tree
        if let Some(m_type) = media_type {
            match m_type {
                "photos" => { builder.push(" AND duration_seconds IS NULL "); }
                "videos" => { builder.push(" AND duration_seconds IS NOT NULL "); }
                _ => {}
            }
        }

        builder.push(" ORDER BY created_at DESC");

        let rows = builder.build().fetch_all(pool).await?;

        use std::collections::BTreeMap;
        let mut groups: BTreeMap<String, (String, i64, Option<String>)> = BTreeMap::new();

        for r in rows {
            let full_fp: String = r.get("folder_path");
            let thumb: Option<String> = r.get("thumb_path");

            let remainder = if prefix.is_empty() {
                full_fp.as_str()
            } else {
                full_fp.strip_prefix(&prefix).unwrap_or(&full_fp)
            };

            let direct_child = remainder.split('/').next().unwrap_or("").trim();
            if direct_child.is_empty() || direct_child == "root" {
                continue;
            }

            let full_child_path = if prefix.is_empty() {
                direct_child.to_string()
            } else {
                format!("{}{}", prefix, direct_child)
            };

            let entry = groups.entry(direct_child.to_string()).or_insert((full_child_path, 0, thumb));
            entry.1 += 1;
        }

        let albums = groups
            .into_iter()
            .map(|(name, (path, count, cover_thumb))| SubAlbum {
                name,
                path,
                count,
                cover_thumb,
            })
            .collect();

        Ok(albums)
    }
}