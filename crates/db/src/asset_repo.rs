use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::path::Path;
use std::collections::BTreeMap;
use chrono::{NaiveDate};

use crate::domain::{
    AssetStorageInfo, MediaPageResponse, MediaQuery, SubAlbum, MediaSection,
    NewAssetRecord, DynamicFiltersResponse, FilterOption, AssetCacheMetadata,
    RawMediaRow, MediaItemSummary, MapLocationPoint, MapLocationsQuery,
    AssetObjectDetail, AssetPoseDetail, CheckUploadResponse,
    ExistingAssetRow, TimelineBucket, BreadcrumbSegment
};

pub struct AssetRepo;

impl AssetRepo {

    /// O(1) duplicate check powered by UNIQUE(user_id, sha256).
    pub async fn check_duplicate_by_sha256(
        pool: &SqlitePool,
        user_id: &str,
        sha256: &str,
    ) -> Result<CheckUploadResponse, sqlx::Error> {
        let row = sqlx::query_as::<_, ExistingAssetRow>(
            r#"
            SELECT id, deleted_at
            FROM assets
            WHERE user_id = ?1 AND sha256 = ?2
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(sha256)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(record) => {
                let is_deleted = record.deleted_at.is_some();
                let message = if is_deleted {
                    Some("File exists in Trash".to_string())
                } else {
                    Some("Duplicate file exists in your library".to_string())
                };

                Ok(CheckUploadResponse {
                    exists: true,
                    asset_id: Some(record.id),
                    is_deleted,
                    message,
                })
            }
            None => Ok(CheckUploadResponse {
                exists: false,
                asset_id: None,
                is_deleted: false,
                message: None,
            }),
        }
    }

    /// Formats an ordinal day suffix: 1 -> "1st", 2 -> "2nd", 3 -> "3rd", 4 -> "4th"
    #[inline]
    fn ordinal_suffix(day: u32) -> &'static str {
        match day {
            11..=13 => "th",
            _ => match day % 10 {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            },
        }
    }

    /// Pre-formats ready-to-render section metadata:
    /// Returns: (section_id, title, month_short, year_str, date_iso)
    /// Example title: "Saturday, 12th September 2026"
    fn format_daily_section_meta(dt_str: &str) -> (String, Option<String>, Option<String>, Option<String>, Option<String>) {
        if dt_str.len() >= 10 {
            let y_slice = &dt_str[0..4];
            let m_slice = &dt_str[5..7];
            let d_slice = &dt_str[8..10];

            if let (Ok(y), Ok(m), Ok(d)) = (y_slice.parse::<i32>(), m_slice.parse::<u32>(), d_slice.parse::<u32>()) {
                if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
                    let weekday = date.format("%A");
                    let month_full = date.format("%B");
                    let month_short = date.format("%b").to_string();
                    let year_str = y.to_string();
                    let suffix = Self::ordinal_suffix(d);

                    let title = format!("{weekday}, {d}{suffix} {month_full} {year_str}");
                    let date_iso = format!("{y_slice}-{m_slice}-{d_slice}");
                    let section_id = format!("section-{date_iso}");

                    return (section_id, Some(title), Some(month_short), Some(year_str), Some(date_iso));
                }
            }
        }

        (
            "section-undated".to_string(),
            Some("Undated".to_string()),
            None,
            None,
            None,
        )
    }

    /// Single-pass sequential grouper (O(N) with zero heap re-allocations)
    /// Produces clean, daily-scoped MediaSection partitions for Timeline mode.
    fn build_grouped_sections(rows: Vec<RawMediaRow>) -> Vec<MediaSection> {
        let mut sections: Vec<MediaSection> = Vec::new();

        for r in rows {
            let (sec_id, title, month, year, date_iso) = match &r.captured_at {
                Some(dt) => Self::format_daily_section_meta(dt),
                None => (
                    "section-undated".to_string(),
                    Some("Undated".to_string()),
                    None,
                    None,
                    None,
                ),
            };

            let item = MediaItemSummary::from(r);

            if let Some(last_sec) = sections.last_mut() {
                if last_sec.id == sec_id {
                    last_sec.items.push(item);
                    continue;
                }
            }

            sections.push(MediaSection {
                id: sec_id,
                title,
                month,
                year,
                date_iso,
                items: vec![item],
            });
        }

        sections
    }

    /// Builds a breadcrumb hierarchy from a relative folder path (e.g. "Trips/2026/Goa")
    fn build_path_breadcrumbs(folder_path: Option<&str>) -> Vec<BreadcrumbSegment> {
        let path = match folder_path {
            Some(p) => p.trim_matches('/'),
            None => return Vec::new(),
        };

        if path.is_empty() {
            return Vec::new();
        }

        let parts: Vec<&str> = path.split('/').collect();
        let mut crumbs = Vec::with_capacity(parts.len());
        let mut cumulative = String::new();

        for part in parts {
            if !cumulative.is_empty() {
                cumulative.push('/');
            }
            cumulative.push_str(part);

            crumbs.push(BreadcrumbSegment {
                name: part.to_string(),
                path: cumulative.clone(),
                album_id: None,
            });
        }

        crumbs
    }

    pub async fn query_media(
        pool: &SqlitePool,
        user_id: &str,
        q: &MediaQuery,
    ) -> Result<MediaPageResponse, sqlx::Error> {
        let limit = q.limit.unwrap_or(50).clamp(1, 200) as usize;
        let fetch_limit = (limit + 1) as i64;
        let show_trash = q.show_trash.unwrap_or(false);

        // Sort mode: defaults to "random" when not explicitly set
        let sort_mode = q.sort.as_deref().unwrap_or("random");
        let is_random = sort_mode == "random";

        // Seed used for stable pagination in random mode
        let random_seed = q.seed.unwrap_or(1337).abs();

        // 1. Base SELECT query
        let mut builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT \
                a.id, a.file_name, a.thumb_path, a.preview_path, \
                a.aspect_ratio, a.duration_seconds, a.mime_type, a.captured_at, \
                a.is_favorite, a.deleted_at, a.latitude, a.longitude \
            FROM assets a "
        );

        let fts_query = q.q.as_deref().and_then(Self::sanitize_query);
        if fts_query.is_some() {
            builder.push(" JOIN asset_search_index fts ON fts.asset_id = a.id ");
        }

        // --- HARD USER ISOLATION ---
        builder.push(" WHERE a.user_id = ");
        builder.push_bind(user_id);

        // --- TRASH SEPARATION ---
        if show_trash {
            builder.push(" AND a.deleted_at IS NOT NULL ");
        } else {
            builder.push(" AND a.deleted_at IS NULL ");
        }

        // --- FULL TEXT SEARCH ---
        if let Some(ref expr) = fts_query {
            builder.push(" AND fts.user_id = ");
            builder.push_bind(user_id);
            builder.push(" AND asset_search_index MATCH ");
            builder.push_bind(expr);
        }

        // --- MEDIA TYPE FILTER ---
        if let Some(ref m_type) = q.media_type {
            match m_type.as_str() {
                "photos" => {
                    builder.push(" AND (a.mime_type NOT LIKE 'video/%' OR a.mime_type IS NULL) ");
                }
                "videos" => {
                    builder.push(" AND a.mime_type LIKE 'video/%' ");
                }
                _ => {}
            }
        }

        // --- FAVORITES ---
        if let Some(fav) = q.is_favorite {
            builder.push(" AND a.is_favorite = ");
            builder.push_bind(if fav { 1 } else { 0 });
        }

        // --- VECTOR SEARCH CANDIDATES ---
        if let Some(ref cids) = q.candidate_ids {
            if cids.is_empty() {
                builder.push(" AND 0 = 1 ");
            } else {
                builder.push(" AND a.id IN (");
                let mut sep = builder.separated(", ");
                for cid in cids {
                    sep.push_bind(cid);
                }
                sep.push_unseparated(") ");
            }
        }

        // --- MULTI-PERSON FILTER ---
        if let Some(ref pids_str) = q.person_id {
            let pids: Vec<&str> = pids_str
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();

            if !pids.is_empty() {
                let count = pids.len() as i64;
                builder.push(
                    " AND a.id IN ( \
                        SELECT af.asset_id \
                        FROM asset_faces af \
                        JOIN persons p ON af.person_id = p.id \
                        WHERE p.user_id = "
                );
                builder.push_bind(user_id);
                builder.push(" AND af.person_id IN (");
                let mut sep = builder.separated(", ");
                for pid in &pids {
                    sep.push_bind(pid);
                }
                sep.push_unseparated(") GROUP BY af.asset_id HAVING COUNT(DISTINCT af.person_id) = ");
                builder.push_bind(count);
                builder.push(") ");
            }
        }

        // --- MULTI-TAG FILTER ---
        if let Some(ref tags_str) = q.tag {
            let tags: Vec<String> = tags_str
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();

            if !tags.is_empty() {
                let count = tags.len() as i64;
                builder.push(
                    " AND a.id IN ( \
                        SELECT at.asset_id \
                        FROM asset_tags at \
                        JOIN tags t ON at.tag_id = t.id \
                        WHERE t.user_id = "
                );
                builder.push_bind(user_id);
                builder.push(" AND LOWER(t.name) IN (");
                let mut sep = builder.separated(", ");
                for t in &tags {
                    sep.push_bind(t);
                }
                sep.push_unseparated(") GROUP BY at.asset_id HAVING COUNT(DISTINCT LOWER(t.name)) = ");
                builder.push_bind(count);
                builder.push(") ");
            }
        }

        // --- ALBUM FILTER ---
        if let Some(ref album_id) = q.album_id {
            builder.push(
                " AND a.id IN ( \
                    SELECT aa.asset_id \
                    FROM album_assets aa \
                    JOIN albums alb ON aa.album_id = alb.id \
                    WHERE alb.id = "
                );
            builder.push_bind(album_id);
            builder.push(" AND alb.user_id = ");
            builder.push_bind(user_id);
            builder.push(") ");
        }

        // --- DIRECTORY / EXIF FILTERS ---
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
            builder.push(" AND COALESCE(a.captured_at, a.created_at) >= ");
            builder.push_bind(from_date);
        }
        if let Some(ref to_date) = q.to {
            builder.push(" AND COALESCE(a.captured_at, a.created_at) <= ");
            builder.push_bind(to_date);
        }

        // --- PAGINATION & ORDERING ---
        if is_random {
            let seed_scalar: i64 = (random_seed % 100_000) + 1;

            let hash_expr = format!(
                "abs(((unicode(substr(a.id, 1, 1)) * 31 + unicode(substr(a.id, 2, 1))) * 31 + unicode(substr(a.id, 3, 1))) * {seed_scalar}) % 1000003"
            );

            if let Some(cid) = q.cursor_id.as_deref().filter(|s| !s.trim().is_empty()) {
                // Compute the cursor's hash in Rust directly.
                // Unicode codepoints of ASCII characters in Rust match SQLite's unicode() 1:1.
                let c0 = cid.chars().nth(0).map(|c| c as u32 as i64).unwrap_or(0);
                let c1 = cid.chars().nth(1).map(|c| c as u32 as i64).unwrap_or(0);
                let c2 = cid.chars().nth(2).map(|c| c as u32 as i64).unwrap_or(0);
                let cursor_hash = (((c0 * 31 + c1) * 31 + c2) * seed_scalar).abs() % 1_000_003;

                builder.push(" AND (");
                builder.push(&hash_expr);
                builder.push(format!(" > {cursor_hash} OR ("));
                builder.push(&hash_expr);
                builder.push(format!(" = {cursor_hash} AND a.id > "));
                builder.push_bind(cid);
                builder.push(")) ");
            }

            builder.push(" ORDER BY ");
            builder.push(&hash_expr);
            builder.push(" ASC, a.id ASC LIMIT ");
            builder.push_bind(fetch_limit);
        } else {
            // Deterministic Keyset Pagination (Timeline)
            if let (Some(cat), Some(cid)) = (&q.cursor_captured_at, &q.cursor_id) {
                let cat_clean = cat.trim();
                let cid_clean = cid.trim();

                if !cat_clean.is_empty() && !cid_clean.is_empty() {
                    builder.push(" AND (COALESCE(a.captured_at, '') < ");
                    builder.push_bind(cat_clean);
                    builder.push(" OR (COALESCE(a.captured_at, '') = ");
                    builder.push_bind(cat_clean);
                    builder.push(" AND a.id < ");
                    builder.push_bind(cid_clean);
                    builder.push(")) ");
                }
            }

            if fts_query.is_some() {
                builder.push(" ORDER BY fts.rank ASC, COALESCE(a.captured_at, '') DESC, a.id DESC LIMIT ");
            } else {
                builder.push(" ORDER BY COALESCE(a.captured_at, '') DESC, a.id DESC LIMIT ");
            }
            builder.push_bind(fetch_limit);
        }

        // Sub-album retrieval (Only in timeline root folder view, not in random feed)
        let albums = if q.cursor_id.is_none() && !show_trash && q.album_id.is_none() && !is_random {
            let curr = q.folder_path.as_deref().unwrap_or("");
            Self::get_sub_albums(pool, user_id, curr, q.media_type.as_deref()).await.unwrap_or_default()
        } else {
            Vec::new()
        };

        // Breadcrumbs: generated server-side from folder_path
        let breadcrumbs = Self::build_path_breadcrumbs(q.folder_path.as_deref());

        // 2. Fetch rows
        let mut rows = builder.build_query_as::<RawMediaRow>().fetch_all(pool).await?;
        let has_more = rows.len() > limit;
        if has_more {
            rows.truncate(limit);
        }

        // 3. Extract cursors from the last item
        let next_cursor_captured_at = rows.last().map(|i| i.captured_at.clone().unwrap_or_default());
        let next_cursor_id = rows.last().map(|i| i.id.clone());

        // 4. Group into sections
        let sections = if is_random {
            if rows.is_empty() {
                Vec::new()
            } else {
                vec![MediaSection {
                    id: "explore-feed".to_string(),
                    title: None,
                    month: None,
                    year: None,
                    date_iso: None,
                    items: rows.into_iter().map(MediaItemSummary::from).collect(),
                }]
            }
        } else {
            Self::build_grouped_sections(rows)
        };

        Ok(MediaPageResponse {
            albums,
            breadcrumbs,
            sections,
            next_cursor_captured_at,
            next_cursor_id,
            has_more,
        })
    }

    pub async fn get_dynamic_filters(
        pool: &SqlitePool,
        user_id: &str,
        q: &MediaQuery,
    ) -> Result<DynamicFiltersResponse, sqlx::Error> {
        let show_trash = q.show_trash.unwrap_or(false);
        let fts_query = q.q.as_deref().and_then(Self::sanitize_query);

        let pids: Vec<&str> = q
            .person_id
            .as_deref()
            .map(|s| s.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()).collect())
            .unwrap_or_default();

        let tags: Vec<String> = q
            .tag
            .as_deref()
            .map(|s| s.split(',').map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect())
            .unwrap_or_default();

        // Project ONLY the lightweight columns required for aggregation.
        let build_cte = || {
            let mut builder: QueryBuilder<Sqlite> = QueryBuilder::new(
                "WITH filtered AS ( \
                    SELECT \
                        a.id, \
                        a.duration_seconds, \
                        a.captured_at, \
                        a.year, \
                        a.month, \
                        a.hour, \
                        a.city, \
                        a.camera_model, \
                        a.folder_path \
                    FROM assets a "
            );

            // Directly join album_assets if album filter is specified
            if let Some(ref album_id) = q.album_id {
                builder.push(" JOIN album_assets aa ON aa.asset_id = a.id AND aa.album_id = ");
                builder.push_bind(album_id);
            }

            if fts_query.is_some() {
                builder.push(" JOIN asset_search_index fts ON fts.asset_id = a.id ");
            }

            builder.push(" WHERE a.user_id = ");
            builder.push_bind(user_id);

            if show_trash {
                builder.push(" AND a.deleted_at IS NOT NULL ");
            } else {
                builder.push(" AND a.deleted_at IS NULL ");
            }

            if let Some(ref expr) = fts_query {
                builder.push(" AND fts.user_id = ");
                builder.push_bind(user_id);
                builder.push(" AND asset_search_index MATCH ");
                builder.push_bind(expr);
            }

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

            if let Some(ref cids) = q.candidate_ids {
                if cids.is_empty() {
                    builder.push(" AND 1 = 0 ");
                } else {
                    builder.push(" AND a.id IN (");
                    let mut sep = builder.separated(", ");
                    for cid in cids {
                        sep.push_bind(cid);
                    }
                    sep.push_unseparated(") ");
                }
            }

            if !pids.is_empty() {
                let count = pids.len() as i64;
                builder.push(" AND a.id IN (SELECT af.asset_id FROM asset_faces af JOIN persons p ON af.person_id = p.id WHERE p.user_id = ");
                builder.push_bind(user_id);
                builder.push(" AND af.person_id IN (");
                let mut sep = builder.separated(", ");
                for pid in &pids { sep.push_bind(pid); }
                sep.push_unseparated(") GROUP BY af.asset_id HAVING COUNT(DISTINCT af.person_id) = ");
                builder.push_bind(count);
                builder.push(") ");
            }

            if !tags.is_empty() {
                let count = tags.len() as i64;
                builder.push(" AND a.id IN (SELECT at.asset_id FROM asset_tags at JOIN tags t ON at.tag_id = t.id WHERE t.user_id = ");
                builder.push_bind(user_id);
                builder.push(" AND LOWER(t.name) IN (");
                let mut sep = builder.separated(", ");
                for t in &tags { sep.push_bind(t); }
                sep.push_unseparated(") GROUP BY at.asset_id HAVING COUNT(DISTINCT LOWER(t.name)) = ");
                builder.push_bind(count);
                builder.push(") ");
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

            builder.push(") ");
            builder
        };

        // 1. Primary counts and date bounds
        let mut stats_builder = build_cte();
        stats_builder.push(
            r#"
            SELECT 
                COUNT(*) as total_count,
                COUNT(CASE WHEN duration_seconds IS NULL THEN 1 END) as p_count,
                COUNT(CASE WHEN duration_seconds IS NOT NULL THEN 1 END) as v_count,
                MIN(captured_at) as min_d, 
                MAX(captured_at) as max_d 
            FROM filtered
            "#
        );

        let stats_row = stats_builder.build().fetch_one(pool).await?;
        let total_media: i64 = stats_row.try_get("total_count").unwrap_or(0);

        if total_media == 0 {
            return Ok(DynamicFiltersResponse::default());
        }

        let photos_count: i64 = stats_row.try_get("p_count").unwrap_or(0);
        let videos_count: i64 = stats_row.try_get("v_count").unwrap_or(0);
        let min_date: Option<String> = stats_row.try_get("min_d").ok();
        let max_date: Option<String> = stats_row.try_get("max_d").ok();

        // 2. Full Timeline Milestones
        let mut timeline_builder = build_cte();
        timeline_builder.push(
            r#"
            SELECT 
                CAST(year AS TEXT) as yr,
                PRINTF('%02d', month) as mo,
                CASE month
                    WHEN 1 THEN 'Jan' WHEN 2 THEN 'Feb' WHEN 3 THEN 'Mar'
                    WHEN 4 THEN 'Apr' WHEN 5 THEN 'May' WHEN 6 THEN 'Jun'
                    WHEN 7 THEN 'Jul' WHEN 8 THEN 'Aug' WHEN 9 THEN 'Sep'
                    WHEN 10 THEN 'Oct' WHEN 11 THEN 'Nov' WHEN 12 THEN 'Dec'
                    ELSE 'Unknown'
                END as mo_name,
                COUNT(*) as count,
                MAX(captured_at) as latest_captured_at
            FROM filtered
            WHERE year IS NOT NULL AND month IS NOT NULL
            GROUP BY year, month
            ORDER BY year DESC, month DESC
            "#
        );

        let timeline: Vec<TimelineBucket> = timeline_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| TimelineBucket {
                year: r.get("yr"),
                month: r.get("mo"),
                month_name: r.get("mo_name"),
                count: r.get("count"),
                latest_captured_at: r.get("latest_captured_at"),
            })
            .collect();

        // 3. Times of day breakdown
        let mut tod_builder = build_cte();
        tod_builder.push(
            r#"
            SELECT 
                CASE 
                    WHEN hour BETWEEN 5 AND 11 THEN 'Morning'
                    WHEN hour BETWEEN 12 AND 16 THEN 'Afternoon'
                    WHEN hour BETWEEN 17 AND 20 THEN 'Evening'
                    ELSE 'Night'
                END as period,
                COUNT(*) as count
            FROM filtered
            WHERE hour IS NOT NULL 
            GROUP BY period 
            ORDER BY count DESC
            "#
        );

        let times_of_day = tod_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get::<String, _>("period").to_lowercase(),
                label: r.get("period"),
                count: r.get("count"),
            })
            .collect();

        // 4. People breakdown: Drives directly from asset_id clustered index
        let mut people_builder = build_cte();
        people_builder.push(
            r#"
            SELECT p.id, p.name, COUNT(DISTINCT af.asset_id) as count
            FROM asset_faces af
            JOIN persons p ON af.person_id = p.id
            WHERE p.user_id = 
            "#
        );
        people_builder.push_bind(user_id);
        people_builder.push(
            r#"
              AND p.name IS NOT NULL
              AND af.asset_id IN (SELECT id FROM filtered)
            GROUP BY p.id, p.name
            ORDER BY count DESC 
            LIMIT 30
            "#
        );

        let people = people_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("id"),
                label: r.get::<Option<String>, _>("name").unwrap_or_else(|| "Unnamed".into()),
                count: r.get("count"),
            })
            .collect();

        // 5. Tags breakdown: Drives directly from asset_tags primary key (asset_id, tag_id)
        let mut tags_builder = build_cte();
        tags_builder.push(
            r#"
            SELECT t.name, COUNT(DISTINCT at.asset_id) as count
            FROM asset_tags at
            JOIN tags t ON at.tag_id = t.id
            WHERE t.user_id = 
            "#
        );
        tags_builder.push_bind(user_id);
        tags_builder.push(
            r#"
              AND at.asset_id IN (SELECT id FROM filtered)
            GROUP BY t.id, t.name
            ORDER BY count DESC 
            LIMIT 40
            "#
        );

        let tags = tags_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("name"),
                label: r.get("name"),
                count: r.get("count"),
            })
            .collect();

        // 6. Locations breakdown
        let mut loc_builder = build_cte();
        loc_builder.push(
            r#"
            SELECT city, COUNT(*) as count 
            FROM filtered 
            WHERE city IS NOT NULL 
            GROUP BY city 
            ORDER BY count DESC 
            LIMIT 20
            "#
        );

        let locations = loc_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("city"),
                label: r.get("city"),
                count: r.get("count"),
            })
            .collect();

        // 7. Cameras breakdown
        let mut cam_builder = build_cte();
        cam_builder.push(
            r#"
            SELECT camera_model, COUNT(*) as count 
            FROM filtered 
            WHERE camera_model IS NOT NULL 
            GROUP BY camera_model 
            ORDER BY count DESC 
            LIMIT 15
            "#
        );

        let cameras = cam_builder.build().fetch_all(pool).await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("camera_model"),
                label: r.get("camera_model"),
                count: r.get("count"),
            })
            .collect();

        // 8. Folders breakdown
        let mut album_builder = build_cte();
        album_builder.push(
            r#"
            SELECT folder_path, COUNT(*) as count 
            FROM filtered 
            WHERE folder_path IS NOT NULL AND folder_path != 'root' 
            GROUP BY folder_path 
            ORDER BY count DESC 
            LIMIT 20
            "#
        );

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
            timeline,
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

    pub async fn insert_asset_tx(
        tx: &mut sqlx::SqliteConnection,
        record: &NewAssetRecord,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO assets (
                id, user_id, sha256, file_name, rel_path, folder_path, thumb_path, preview_path,
                file_size_bytes, mime_type, width, height, aspect_ratio, duration_seconds,
                captured_at, year, month, day, hour, latitude, longitude,
                altitude_meters, city, subdivision, country, country_code, camera_make, camera_model,
                clip_embedding
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29
            )
            "#,
        )
        .bind(&record.id)
        .bind(&record.user_id)
        .bind(&record.sha256)
        .bind(&record.file_name)
        .bind(&record.rel_path)
        .bind(&record.folder_path)
        .bind(&record.thumb_path)
        .bind(&record.preview_path)
        .bind(record.file_size_bytes)
        .bind(&record.mime_type)
        .bind(record.width)
        .bind(record.height)
        .bind(record.aspect_ratio)
        .bind(record.duration_seconds)
        .bind(&record.captured_at)
        .bind(record.year)
        .bind(record.month)
        .bind(record.day)
        .bind(record.hour)
        .bind(record.latitude)
        .bind(record.longitude)
        .bind(record.altitude)
        .bind(&record.city)
        .bind(&record.subdivision)
        .bind(&record.country)
        .bind(&record.country_code)
        .bind(&record.camera_make)
        .bind(&record.camera_model)
        .bind(&record.clip_embedding)
        .execute(tx)
        .await?;

        Ok(())
    }

    pub async fn sync_search_index(
        pool: &SqlitePool,
        user_id: &str,
        asset_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM asset_search_index WHERE asset_id = ?1 AND user_id = ?2")
            .bind(asset_id)
            .bind(user_id)
            .execute(pool)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO asset_search_index (asset_id, user_id, persons, tags, location, temporal, camera, file_name)
            SELECT 
                a.id,
                a.user_id,
                COALESCE((
                    SELECT GROUP_CONCAT(name, ' ')
                    FROM (
                        SELECT DISTINCT p.name AS name
                        FROM asset_faces af
                        JOIN persons p ON af.person_id = p.id
                        WHERE af.asset_id = a.id 
                          AND p.user_id = a.user_id
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
                          AND t.user_id = a.user_id
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
            WHERE a.id = ?1 AND a.user_id = ?2
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Toggles the favorite flag on an asset owned by the user.
    pub async fn toggle_favorite(
        pool: &SqlitePool,
        user_id: &str,
        asset_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query(
            "SELECT is_favorite FROM assets WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL",
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        let current: i64 = row.try_get("is_favorite").unwrap_or(0);
        let new_state = if current == 1 { 0 } else { 1 };

        sqlx::query(
            "UPDATE assets SET is_favorite = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2 AND user_id = ?3",
        )
        .bind(new_state)
        .bind(asset_id)
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(new_state == 1)
    }

    /// Retrieves asset file metadata strictly scoped to the authenticated user.
    pub async fn get_storage_info(
        pool: &SqlitePool,
        user_id: &str,
        asset_id: &str,
    ) -> Result<Option<AssetStorageInfo>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT rel_path, preview_path, (duration_seconds IS NOT NULL) AS is_video 
            FROM assets 
            WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        let info = row.map(|r| AssetStorageInfo {
            rel_path: r.try_get("rel_path").unwrap_or_default(),
            preview_path: r.try_get("preview_path").unwrap_or_default(),
            is_video: r.try_get::<bool, _>("is_video").unwrap_or(false),
        });

        Ok(info)
    }

    /// Discovers immediate child folders under a given path with counts and latest cover thumbnail
    pub async fn get_sub_albums(
        pool: &SqlitePool,
        user_id: &str,
        current_folder: &str,
        media_type: Option<&str>,
    ) -> Result<Vec<SubAlbum>, sqlx::Error> {
        let clean_current = current_folder.trim_matches('/');
        let prefix = if clean_current.is_empty() || clean_current == "root" {
            String::new()
        } else {
            format!("{clean_current}/")
        };

        let mut builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            r#"
            SELECT 
                folder_path, 
                thumb_path
            FROM assets
            WHERE user_id = 
            "#,
        );
        builder.push_bind(user_id);
        builder.push(" AND deleted_at IS NULL ");
        builder.push(" AND folder_path LIKE ");
        builder.push_bind(format!("{prefix}%"));
        builder.push(" AND folder_path != ");
        builder.push_bind(if clean_current.is_empty() { "root" } else { clean_current });

        // Photo / Video separation in album tree
        if let Some(m_type) = media_type {
            match m_type {
                "photos" => { builder.push(" AND duration_seconds IS NULL "); }
                "videos" => { builder.push(" AND duration_seconds IS NOT NULL "); }
                _ => {}
            }
        }

        // Newest assets first so the first row encountered per folder is the latest cover
        builder.push(" ORDER BY COALESCE(captured_at, created_at) DESC ");

        let rows = builder.build().fetch_all(pool).await?;

        // Map: direct_child_name -> (full_child_path, item_count, latest_cover_thumb)
        let mut groups: BTreeMap<String, (String, i64, Option<String>)> = BTreeMap::new();

        for r in rows {
            let full_fp: String = r.try_get("folder_path").unwrap_or_default();
            let thumb: Option<String> = r.try_get("thumb_path").ok();

            let clean_fp = full_fp.trim_matches('/');
            let remainder = if prefix.is_empty() {
                clean_fp
            } else {
                clean_fp.strip_prefix(&prefix).unwrap_or(clean_fp)
            };

            let direct_child = remainder.split('/').next().unwrap_or("").trim();
            if direct_child.is_empty() || direct_child == "root" {
                continue;
            }

            let full_child_path = if prefix.is_empty() {
                direct_child.to_string()
            } else {
                format!("{prefix}{direct_child}")
            };

            let entry = groups
                .entry(direct_child.to_string())
                .or_insert_with(|| (full_child_path, 0, thumb));
            entry.1 += 1;
        }

        let albums = groups
            .into_iter()
            .map(|(name, (path, count, cover_thumb))| SubAlbum {
                id: None, // Raw folder path directory; None signals filesystem-backed
                name,
                path,
                count,
                cover_thumb,
            })
            .collect();

        Ok(albums)
    }

    pub async fn fetch_and_purge_expired_trash(
        pool: &SqlitePool,
        storage_root: &Path,
    ) -> Result<usize, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, user_id, rel_path, thumb_path, preview_path 
            FROM assets 
            WHERE deleted_at IS NOT NULL 
              AND datetime(deleted_at, '+30 days') <= datetime('now')
            "#,
        )
        .fetch_all(pool)
        .await?;

        let count = rows.len();

        for r in rows {
            let id: String = r.get("id");
            let user_id: String = r.get("user_id");
            let rel_path: String = r.get("rel_path");
            let thumb_path: String = r.get("thumb_path");
            let preview_path: String = r.get("preview_path");

            // storage_root/users/<user_id>/originals/<rel_path>
            let orig_file = storage_root
                .join("users")
                .join(&user_id)
                .join("originals")
                .join(&rel_path);

            // thumb_path & preview_path already include "users/<user_id>/thumbs/..."
            let _ = tokio::fs::remove_file(orig_file).await;
            let _ = tokio::fs::remove_file(storage_root.join(&thumb_path)).await;
            let _ = tokio::fs::remove_file(storage_root.join(&preview_path)).await;

            let _ = sqlx::query("DELETE FROM assets WHERE id = ?1 AND user_id = ?2")
                .bind(&id)
                .bind(&user_id)
                .execute(pool)
                .await;
        }

        Ok(count)
    }

    /// Toggles deleted_at timestamp strictly for the authenticated user's asset.
    pub async fn toggle_soft_delete(
        pool: &SqlitePool,
        user_id: &str,
        id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            UPDATE assets
            SET deleted_at = CASE 
                WHEN deleted_at IS NULL THEN CURRENT_TIMESTAMP 
                ELSE NULL 
            END,
            updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1 AND user_id = ?2
            RETURNING deleted_at
            "#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        let deleted_at: Option<String> = row.get("deleted_at");
        Ok(deleted_at)
    }

    /// Deletes asset rows and unlinks files from SSD strictly scoped to the user.
    pub async fn purge_asset(
        pool: &SqlitePool,
        user_id: &str,
        id: &str,
        storage_root: &Path,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query(
            "SELECT rel_path, thumb_path, preview_path FROM assets WHERE id = ?1 AND user_id = ?2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        let row = match row {
            Some(r) => r,
            None => return Ok(false),
        };

        let rel_path: String = row.get("rel_path");
        let thumb_path: String = row.get("thumb_path");
        let preview_path: String = row.get("preview_path");

        // storage_root/users/<user_id>/originals/<rel_path>
        let orig_file = storage_root
            .join("users")
            .join(user_id)
            .join("originals")
            .join(&rel_path);

        // thumb_path & preview_path already include "users/<user_id>/thumbs/..."
        let _ = tokio::fs::remove_file(orig_file).await;
        let _ = tokio::fs::remove_file(storage_root.join(&thumb_path)).await;
        let _ = tokio::fs::remove_file(storage_root.join(&preview_path)).await;

        sqlx::query("DELETE FROM assets WHERE id = ?1 AND user_id = ?2")
            .bind(id)
            .bind(user_id)
            .execute(pool)
            .await?;

        Ok(true)
    }

    /// Batch soft-deletes/restores multiple assets belonging to the user.
    pub async fn batch_toggle_soft_delete(
        pool: &SqlitePool,
        user_id: &str,
        ids: &[String],
    ) -> Result<usize, sqlx::Error> {
        if ids.is_empty() {
            return Ok(0);
        }

        let mut tx = pool.begin().await?;
        let mut count = 0;

        for id in ids {
            let res = sqlx::query(
                r#"
                UPDATE assets
                SET deleted_at = CASE 
                    WHEN deleted_at IS NULL THEN CURRENT_TIMESTAMP 
                    ELSE NULL 
                END,
                updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1 AND user_id = ?2
                "#,
            )
            .bind(id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

            count += res.rows_affected() as usize;
        }

        tx.commit().await?;
        Ok(count)
    }

    /// Batch hard-purges multiple assets belonging to the user.
    pub async fn batch_purge(
        pool: &SqlitePool,
        user_id: &str,
        ids: &[String],
        storage_root: &Path,
    ) -> Result<usize, sqlx::Error> {
        if ids.is_empty() {
            return Ok(0);
        }

        let mut purged_count = 0;

        for id in ids {
            if let Ok(true) = Self::purge_asset(pool, user_id, id, storage_root).await {
                purged_count += 1;
            }
        }

        Ok(purged_count)
    }

    pub async fn find_user_asset_by_sha256(
        pool: &SqlitePool,
        user_id: &str,
        sha256: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query_scalar::<_, String>(
            "SELECT id FROM assets WHERE user_id = ? AND sha256 = ? AND deleted_at IS NULL LIMIT 1"
        )
        .bind(user_id)
        .bind(sha256)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn get_cache_metadata(
        pool: &SqlitePool,
        asset_id: &str,
        user_id: &str,
    ) -> Result<Option<AssetCacheMetadata>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT thumb_path, mime_type FROM assets WHERE id = ? AND user_id = ?"
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| AssetCacheMetadata {
            thumb_path: r.get("thumb_path"),
            mime_type: r.get("mime_type"),
        }))
    }

    /// Fetch all active geotagged media for a user with optional bounding-box constraints.
    pub async fn query_locations(
        pool: &SqlitePool,
        user_id: &str,
        q: &MapLocationsQuery,
    ) -> Result<Vec<MapLocationPoint>, sqlx::Error> {
        let mut builder: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT \
                a.id, \
                a.latitude AS lat, \
                a.longitude AS lng, \
                a.thumb_path \
            FROM assets a \
            WHERE a.user_id = "
        );
        builder.push_bind(user_id);

        // Strict non-trashed & non-null geo constraints (hits idx_assets_user_geo)
        builder.push(" AND a.deleted_at IS NULL ");
        builder.push(" AND a.latitude IS NOT NULL ");
        builder.push(" AND a.longitude IS NOT NULL ");

        // Optional Bounding-Box filtering for high-density viewport updates
        if let (Some(min_lat), Some(max_lat)) = (q.min_lat, q.max_lat) {
            builder.push(" AND a.latitude BETWEEN ");
            builder.push_bind(min_lat);
            builder.push(" AND ");
            builder.push_bind(max_lat);
        }

        if let (Some(min_lng), Some(max_lng)) = (q.min_lng, q.max_lng) {
            builder.push(" AND a.longitude BETWEEN ");
            builder.push_bind(min_lng);
            builder.push(" AND ");
            builder.push_bind(max_lng);
        }

        if let Some(ref folder) = q.folder_path {
            if !folder.is_empty() {
                builder.push(" AND a.folder_path = ");
                builder.push_bind(folder);
            }
        }

        // Return latest captures first
        builder.push(" ORDER BY a.captured_at DESC, a.created_at DESC");

        let points = builder
            .build_query_as::<MapLocationPoint>()
            .fetch_all(pool)
            .await?;

        Ok(points)
    }

    /// Query detected YOLO objects for a specific asset scoped to the user
    pub async fn get_asset_objects(
        pool: &sqlx::SqlitePool,
        user_id: &str,
        asset_id: &str,
    ) -> Result<Vec<AssetObjectDetail>, sqlx::Error> {
        let rows = sqlx::query_as::<_, AssetObjectDetail>(
            r#"
            SELECT 
                ao.id,
                ao.asset_id,
                ao.class_id,
                ao.label,
                ao.score,
                ao.bbox_x,
                ao.bbox_y,
                ao.bbox_w,
                ao.bbox_h
            FROM asset_objects ao
            JOIN assets a ON ao.asset_id = a.id
            WHERE ao.asset_id = ?1
              AND a.user_id = ?2
              AND a.deleted_at IS NULL
            ORDER BY ao.score DESC
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Query detected human poses with keypoints for a specific asset scoped to the user
    pub async fn get_asset_poses(
        pool: &sqlx::SqlitePool,
        user_id: &str,
        asset_id: &str,
    ) -> Result<Vec<AssetPoseDetail>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT 
                ap.id,
                ap.asset_id,
                ap.score,
                ap.bbox_x,
                ap.bbox_y,
                ap.bbox_w,
                ap.bbox_h,
                ap.keypoints
            FROM asset_poses ap
            JOIN assets a ON ap.asset_id = a.id
            WHERE ap.asset_id = ?1
              AND a.user_id = ?2
              AND a.deleted_at IS NULL
            ORDER BY ap.score DESC
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let poses = rows
            .into_iter()
            .map(|r| {
                let keypoints_raw: String = r.get("keypoints");
                let keypoints_json: serde_json::Value =
                    serde_json::from_str(&keypoints_raw).unwrap_or_else(|_| serde_json::json!([]));

                AssetPoseDetail {
                    id: r.get("id"),
                    asset_id: r.get("asset_id"),
                    score: r.get("score"),
                    bbox_x: r.get("bbox_x"),
                    bbox_y: r.get("bbox_y"),
                    bbox_w: r.get("bbox_w"),
                    bbox_h: r.get("bbox_h"),
                    keypoints: keypoints_json,
                }
            })
            .collect();

        Ok(poses)
    }
}