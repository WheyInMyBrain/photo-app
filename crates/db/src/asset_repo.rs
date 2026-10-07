use chrono::{DateTime, Datelike, Utc};
use sqlx::{PgPool, Postgres, PgConnection, QueryBuilder, Row};
use uuid::Uuid;
use std::path::Path;

use crate::domain::{
    AssetCacheMetadata, AssetObjectDetail, AssetPoseDetail, AssetStorageInfo,
    BreadcrumbSegment, CheckUploadResponse, DynamicFiltersResponse, ExistingAssetRow,
    FilterOption, MapLocationPoint, MapLocationsQuery, MediaItemSummary, MediaPageResponse,
    MediaQuery, MediaSection, NewAssetRecord, RawMediaRow, SubAlbum, TimelineBucket,
};

pub struct AssetRepo;

impl AssetRepo {
    /// O(1) duplicate check powered by UNIQUE(user_id, sha256).
    pub async fn check_duplicate_by_sha256(
        pool: &PgPool,
        user_id: Uuid,
        sha256: &str,
    ) -> Result<CheckUploadResponse, sqlx::Error> {
        let row = sqlx::query_as::<_, ExistingAssetRow>(
            r#"
            SELECT id, deleted_at
            FROM assets
            WHERE user_id = $1 AND sha256 = $2
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
    fn format_daily_section_meta(
        dt: DateTime<Utc>,
    ) -> (String, Option<String>, Option<String>, Option<String>, Option<String>) {
        let naive = dt.date_naive();
        let y = naive.year();
        let m = naive.month();
        let d = naive.day();

        let weekday = naive.format("%A");
        let month_full = naive.format("%B");
        let month_short = naive.format("%b").to_string();
        let year_str = y.to_string();
        let suffix = Self::ordinal_suffix(d);

        let title = format!("{weekday}, {d}{suffix} {month_full} {year_str}");
        let date_iso = format!("{:04}-{:02}-{:02}", y, m, d);
        let section_id = format!("section-{date_iso}");

        (
            section_id,
            Some(title),
            Some(month_short),
            Some(year_str),
            Some(date_iso),
        )
    }

    /// Single-pass sequential grouper (O(N) with zero heap re-allocations)
    fn build_grouped_sections(rows: Vec<RawMediaRow>) -> Vec<MediaSection> {
        let mut sections: Vec<MediaSection> = Vec::new();

        for r in rows {
            let (sec_id, title, month, year, date_iso) = match r.captured_at {
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

    /// Builds a breadcrumb hierarchy from a relative folder path
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
        pool: &PgPool,
        user_id: Uuid,
        q: &MediaQuery,
    ) -> Result<MediaPageResponse, sqlx::Error> {
        let limit = q.limit.unwrap_or(50).clamp(1, 200) as usize;
        let fetch_limit = (limit + 1) as i64;
        let show_trash = q.show_trash.unwrap_or(false);

        let search_term = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let has_search = search_term.is_some();

        let sort_mode = if has_search {
            "rank"
        } else {
            q.sort.as_deref().unwrap_or("timeline")
        };
        let is_random = sort_mode == "random";
        let random_seed = q.seed.unwrap_or(1337).abs();

        let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT \
                a.id, a.file_name, a.thumb_path, a.preview_path, \
                a.aspect_ratio, a.duration_seconds, a.mime_type, a.captured_at, \
                a.is_favorite, a.deleted_at, a.latitude, a.longitude \
            FROM assets a ",
        );

        if let Some(ref album_id) = q.album_id {
            builder.push(" JOIN album_assets aa ON aa.asset_id = a.id AND aa.album_id = ");
            builder.push_bind(album_id);
            builder.push(" AND aa.user_id = ");
            builder.push_bind(user_id);
        }

        builder.push(" WHERE a.user_id = ");
        builder.push_bind(user_id);

        if show_trash {
            builder.push(" AND a.deleted_at IS NOT NULL ");
        } else {
            builder.push(" AND a.deleted_at IS NULL ");
        }

        // Native Full-Text Search
        if let Some(st) = search_term {
            builder.push(" AND a.search_vector @@ websearch_to_tsquery('simple', ");
            builder.push_bind(st);
            builder.push(") ");
        }

        // Photos vs Videos
        if let Some(ref m_type) = q.media_type {
            match m_type.as_str() {
                "photos" => {
                    builder.push(" AND a.duration_seconds IS NULL ");
                }
                "videos" => {
                    builder.push(" AND a.duration_seconds IS NOT NULL ");
                }
                _ => {}
            }
        }

        if let Some(fav) = q.is_favorite {
            builder.push(" AND a.is_favorite = ");
            builder.push_bind(fav);
        }

        if let Some(ref cids) = q.candidate_ids {
            if cids.is_empty() {
                builder.push(" AND FALSE ");
            } else {
                builder.push(" AND a.id = ANY(");
                builder.push_bind(cids);
                builder.push(") ");
            }
        }

        // Multi-Person Filter
        if let Some(ref pids_str) = q.person_id {
            let pids: Vec<Uuid> = pids_str
                .split(',')
                .filter_map(|p| Uuid::parse_str(p.trim()).ok())
                .collect();

            if !pids.is_empty() {
                let count = pids.len() as i64;
                builder.push(
                    " AND a.id IN ( \
                        SELECT af.asset_id \
                        FROM asset_faces af \
                        WHERE af.user_id = ",
                );
                builder.push_bind(user_id);
                builder.push(" AND af.person_id = ANY(");
                builder.push_bind(pids);
                builder.push(") GROUP BY af.asset_id HAVING COUNT(DISTINCT af.person_id) = ");
                builder.push_bind(count);
                builder.push(") ");
            }
        }

        // Multi-Tag Filter
        let tags: Vec<String> = q
            .tag
            .as_deref()
            .map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_lowercase())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        if !tags.is_empty() {
            let count = tags.len() as i64;
            builder.push(
                " AND a.id IN ( \
                    SELECT at.asset_id \
                    FROM asset_tags at \
                    JOIN tags t ON at.tag_id = t.id \
                    WHERE at.user_id = ",
            );
            builder.push_bind(user_id);
            builder.push(" AND LOWER(t.name) = ANY(");
            builder.push_bind(tags);
            builder.push(") GROUP BY at.asset_id HAVING COUNT(DISTINCT LOWER(t.name)) = ");
            builder.push_bind(count);
            builder.push(") ");
        }

        // Directory & EXIF Filters
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

        // Pagination & Ordering
        if has_search {
            // Rank pagination fallback using captured_at and id
            if let (Some(cat), Some(cid)) = (q.cursor_captured_at, q.cursor_id) {
                builder.push(" AND (a.captured_at < ");
                builder.push_bind(cat);
                builder.push(" OR (a.captured_at = ");
                builder.push_bind(cat);
                builder.push(" AND a.id < ");
                builder.push_bind(cid);
                builder.push(")) ");
            }

            builder.push(" ORDER BY ts_rank_cd(a.search_vector, websearch_to_tsquery('simple', ");
            builder.push_bind(search_term.unwrap());
            builder.push(")) DESC, a.captured_at DESC NULLS LAST, a.id DESC LIMIT ");
            builder.push_bind(fetch_limit);
        } else if is_random {
            // Deterministic pseudo-random cursor pagination
            if let Some(cid) = q.cursor_id {
                builder.push(" AND ( \
                    ('x' || substr(md5(a.id::text || ");
                builder.push_bind(random_seed);
                builder.push("::text), 1, 8))::bit(32)::int, a.id \
                ) > ( \
                    (SELECT ('x' || substr(md5(c.id::text || ");
                builder.push_bind(random_seed);
                builder.push("::text), 1, 8))::bit(32)::int FROM assets c WHERE c.id = ");
                builder.push_bind(cid);
                builder.push("), ");
                builder.push_bind(cid);
                builder.push(") ");
            }

            builder.push(" ORDER BY ('x' || substr(md5(a.id::text || ");
            builder.push_bind(random_seed);
            builder.push("::text), 1, 8))::bit(32)::int ASC, a.id ASC LIMIT ");
            builder.push_bind(fetch_limit);
        } else {
            // Safe Keyset Cursor Pagination handling NULL captured_at
            if let (Some(cat), Some(cid)) = (q.cursor_captured_at, q.cursor_id) {
                builder.push(" AND (a.captured_at < ");
                builder.push_bind(cat);
                builder.push(" OR (a.captured_at = ");
                builder.push_bind(cat);
                builder.push(" AND a.id < ");
                builder.push_bind(cid);
                builder.push(")) ");
            } else if let Some(cid) = q.cursor_id {
                // For undated section scrolling
                builder.push(" AND a.captured_at IS NULL AND a.id < ");
                builder.push_bind(cid);
            }

            builder.push(" ORDER BY a.captured_at DESC NULLS LAST, a.id DESC LIMIT ");
            builder.push_bind(fetch_limit);
        }

        let albums = if q.cursor_id.is_none() && !show_trash && q.album_id.is_none() && !is_random && !has_search {
            let curr = q.folder_path.as_deref().unwrap_or("");
            crate::album_repo::AlbumRepo::get_sub_albums(pool, user_id, curr)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|sub| SubAlbum {
                    id: None,
                    name: sub.name,
                    path: sub.full_path,
                    count: sub.media_count,
                    cover_thumb: sub.cover_thumb,
                })
                .collect()
        } else {
            Vec::new()
        };

        let breadcrumbs = Self::build_path_breadcrumbs(q.folder_path.as_deref());

        let mut rows = builder.build_query_as::<RawMediaRow>().fetch_all(pool).await?;
        let has_more = rows.len() > limit;
        if has_more {
            rows.truncate(limit);
        }

        let next_cursor_captured_at = rows.last().and_then(|i| i.captured_at);
        let next_cursor_id = rows.last().map(|i| i.id);

        let sections = if is_random || has_search {
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
        pool: &PgPool,
        user_id: Uuid,
        q: &MediaQuery,
    ) -> Result<DynamicFiltersResponse, sqlx::Error> {
        let show_trash = q.show_trash.unwrap_or(false);
        let search_term = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let pids: Vec<Uuid> = q
            .person_id
            .as_deref()
            .map(|s| {
                s.split(',')
                    .filter_map(|p| Uuid::parse_str(p.trim()).ok())
                    .collect()
            })
            .unwrap_or_default();

        let tags: Vec<String> = q
            .tag
            .as_deref()
            .map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_lowercase())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let build_base_cte = || {
            let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
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
                    FROM assets a ",
            );

            if let Some(ref album_id) = q.album_id {
                builder.push(" JOIN album_assets aa ON aa.asset_id = a.id AND aa.album_id = ");
                builder.push_bind(album_id);
                builder.push(" AND aa.user_id = ");
                builder.push_bind(user_id);
            }

            builder.push(" WHERE a.user_id = ");
            builder.push_bind(user_id);

            if show_trash {
                builder.push(" AND a.deleted_at IS NOT NULL ");
            } else {
                builder.push(" AND a.deleted_at IS NULL ");
            }

            if let Some(st) = search_term {
                builder.push(" AND a.search_vector @@ websearch_to_tsquery('simple', ");
                builder.push_bind(st);
                builder.push(") ");
            }

            if let Some(ref m_type) = q.media_type {
                match m_type.as_str() {
                    "photos" => {
                        builder.push(" AND a.duration_seconds IS NULL ");
                    }
                    "videos" => {
                        builder.push(" AND a.duration_seconds IS NOT NULL ");
                    }
                    _ => {}
                }
            }

            if let Some(fav) = q.is_favorite {
                builder.push(" AND a.is_favorite = ");
                builder.push_bind(fav);
            }

            if let Some(ref cids) = q.candidate_ids {
                if cids.is_empty() {
                    builder.push(" AND FALSE ");
                } else {
                    builder.push(" AND a.id = ANY(");
                    builder.push_bind(cids);
                    builder.push(") ");
                }
            }

            if !pids.is_empty() {
                let count = pids.len() as i64;
                builder.push(
                    " AND a.id IN ( \
                        SELECT af.asset_id \
                        FROM asset_faces af \
                        WHERE af.user_id = ",
                );
                builder.push_bind(user_id);
                builder.push(" AND af.person_id = ANY(");
                builder.push_bind(&pids);
                builder.push(") GROUP BY af.asset_id HAVING COUNT(DISTINCT af.person_id) = ");
                builder.push_bind(count);
                builder.push(") ");
            }

            if !tags.is_empty() {
                let count = tags.len() as i64;
                builder.push(
                    " AND a.id IN ( \
                        SELECT at.asset_id \
                        FROM asset_tags at \
                        JOIN tags t ON at.tag_id = t.id \
                        WHERE at.user_id = ",
                );
                builder.push_bind(user_id);
                builder.push(" AND LOWER(t.name) = ANY(");
                builder.push_bind(&tags);
                builder.push(") GROUP BY at.asset_id HAVING COUNT(DISTINCT LOWER(t.name)) = ");
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

        // ROUNDTRIP 1: Compute counts, date bounds, timeline, times-of-day, cities, cameras, and folders
        let mut main_builder = build_base_cte();
        main_builder.push(
            r#"
            SELECT 
                -- Summary stats
                COUNT(*)::bigint AS total_count,
                COUNT(CASE WHEN duration_seconds IS NULL THEN 1 END)::bigint AS p_count,
                COUNT(CASE WHEN duration_seconds IS NOT NULL THEN 1 END)::bigint AS v_count,
                MIN(captured_at) AS min_d, 
                MAX(captured_at) AS max_d,

                -- Timeline
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'year', t.year::text,
                        'month', LPAD(t.month::text, 2, '0'),
                        'month_name', TO_CHAR(TO_DATE(t.month::text, 'MM'), 'Mon'),
                        'count', t.c,
                        'latest_captured_at', t.latest_d
                    ) ORDER BY t.year DESC, t.month DESC)
                    FROM (
                        SELECT year, month, COUNT(*)::bigint AS c, MAX(captured_at) AS latest_d
                        FROM filtered
                        WHERE year IS NOT NULL AND month IS NOT NULL
                        GROUP BY year, month
                    ) t
                ), '[]'::jsonb) AS timeline_json,

                -- Times of day
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'value', LOWER(tod.period),
                        'label', tod.period,
                        'count', tod.c
                    ) ORDER BY tod.c DESC)
                    FROM (
                        SELECT 
                            CASE 
                                WHEN hour BETWEEN 5 AND 11 THEN 'Morning'
                                WHEN hour BETWEEN 12 AND 16 THEN 'Afternoon'
                                WHEN hour BETWEEN 17 AND 20 THEN 'Evening'
                                ELSE 'Night'
                            END AS period,
                            COUNT(*)::bigint AS c
                        FROM filtered
                        WHERE hour IS NOT NULL
                        GROUP BY period
                    ) tod
                ), '[]'::jsonb) AS tod_json,

                -- Locations
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'value', loc.city,
                        'label', loc.city,
                        'count', loc.c
                    ) ORDER BY loc.c DESC)
                    FROM (
                        SELECT city, COUNT(*)::bigint AS c
                        FROM filtered
                        WHERE city IS NOT NULL
                        GROUP BY city
                        LIMIT 20
                    ) loc
                ), '[]'::jsonb) AS locations_json,

                -- Cameras
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'value', cam.camera_model,
                        'label', cam.camera_model,
                        'count', cam.c
                    ) ORDER BY cam.c DESC)
                    FROM (
                        SELECT camera_model, COUNT(*)::bigint AS c
                        FROM filtered
                        WHERE camera_model IS NOT NULL
                        GROUP BY camera_model
                        LIMIT 15
                    ) cam
                ), '[]'::jsonb) AS cameras_json,

                -- Folders
                COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'value', fld.folder_path,
                        'label', fld.folder_path,
                        'count', fld.c
                    ) ORDER BY fld.c DESC)
                    FROM (
                        SELECT folder_path, COUNT(*)::bigint AS c
                        FROM filtered
                        WHERE folder_path IS NOT NULL AND folder_path <> 'root'
                        GROUP BY folder_path
                        LIMIT 20
                    ) fld
                ), '[]'::jsonb) AS albums_json
            FROM filtered;
            "#,
        );

        let row = main_builder.build().fetch_one(pool).await?;
        let total_media: i64 = row.try_get("total_count").unwrap_or(0);

        if total_media == 0 {
            return Ok(DynamicFiltersResponse::default());
        }

        let photos_count: i64 = row.try_get("p_count").unwrap_or(0);
        let videos_count: i64 = row.try_get("v_count").unwrap_or(0);
        let min_date: Option<DateTime<Utc>> = row.try_get("min_d").ok();
        let max_date: Option<DateTime<Utc>> = row.try_get("max_d").ok();

        let timeline: Vec<TimelineBucket> = serde_json::from_value(row.get("timeline_json")).unwrap_or_default();
        let times_of_day: Vec<FilterOption> = serde_json::from_value(row.get("tod_json")).unwrap_or_default();
        let locations: Vec<FilterOption> = serde_json::from_value(row.get("locations_json")).unwrap_or_default();
        let cameras: Vec<FilterOption> = serde_json::from_value(row.get("cameras_json")).unwrap_or_default();
        let albums: Vec<FilterOption> = serde_json::from_value(row.get("albums_json")).unwrap_or_default();

        // ROUNDTRIP 2: Tags & People (Relational Hash Joins)
        let mut tags_builder = build_base_cte();
        tags_builder.push(
            r#"
            SELECT 
                t.name, 
                COUNT(DISTINCT at.asset_id)::bigint AS count
            FROM filtered f
            JOIN asset_tags at ON f.id = at.asset_id AND at.user_id = 
            "#,
        );
        tags_builder.push_bind(user_id);
        tags_builder.push(
            r#"
            JOIN tags t ON at.tag_id = t.id
            GROUP BY t.id, t.name
            ORDER BY count DESC 
            LIMIT 40
            "#,
        );

        let tags = tags_builder
            .build()
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("name"),
                label: r.get("name"),
                count: r.get("count"),
            })
            .collect();

        let mut people_builder = build_base_cte();
        people_builder.push(
            r#"
            SELECT 
                p.id::text AS id, 
                p.name, 
                COUNT(DISTINCT af.asset_id)::bigint AS count
            FROM filtered f
            JOIN asset_faces af ON f.id = af.asset_id AND af.user_id = 
            "#,
        );
        people_builder.push_bind(user_id);
        people_builder.push(
            r#"
            JOIN persons p ON af.person_id = p.id
            WHERE p.name IS NOT NULL
            GROUP BY p.id, p.name
            ORDER BY count DESC 
            LIMIT 30
            "#,
        );

        let people = people_builder
            .build()
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|r| FilterOption {
                value: r.get("id"),
                label: r
                    .get::<Option<String>, _>("name")
                    .unwrap_or_else(|| "Unnamed".into()),
                count: r.get("count"),
            })
            .collect();

        Ok(DynamicFiltersResponse {
            total_media,
            photos_count,
            videos_count,
            min_date: min_date.map(|d| d.to_rfc3339()),
            max_date: max_date.map(|d| d.to_rfc3339()),
            timeline,
            times_of_day,
            people,
            tags,
            locations,
            cameras,
            albums,
        })
    }

    pub async fn insert_asset_tx(
        tx: &mut PgConnection,
        record: &NewAssetRecord,
    ) -> Result<(), sqlx::Error> {
        let clip_vec: Option<pgvector::Vector> = record.clip_embedding.as_ref().map(|bytes| {
            // bytemuck safely casts raw slice &[u8] to &[f32] for pgvector
            let float_slice: &[f32] = bytemuck::cast_slice(bytes);
            pgvector::Vector::from(float_slice.to_vec())
        });

        sqlx::query(
            r#"
            INSERT INTO assets (
                id, user_id, sha256, file_name, rel_path, folder_path, thumb_path, preview_path,
                file_size_bytes, mime_type, width, height, aspect_ratio, duration_seconds,
                captured_at, year, month, day, hour, latitude, longitude,
                altitude_meters, city, subdivision, country, country_code, camera_make, camera_model,
                caption, author, source_platform, source_url, source_post_id,
                clip_embedding
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30,
                $31, $32, $33, $34
            )
            "#,
        )
        .bind(record.id)
        .bind(record.user_id)
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
        .bind(record.captured_at)
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
        .bind(&record.caption)
        .bind(&record.author)
        .bind(&record.source_platform)
        .bind(&record.source_url)
        .bind(&record.source_post_id)
        .bind(clip_vec)
        .execute(tx)
        .await?;

        Ok(())
    }

    /// Toggles the favorite flag on an asset owned by the user.
    pub async fn toggle_favorite(
        pool: &PgPool,
        user_id: Uuid,
        asset_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query_scalar::<_, bool>(
            r#"
            UPDATE assets
            SET is_favorite = NOT is_favorite,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
            RETURNING is_favorite
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Retrieves asset file metadata strictly scoped to the authenticated user.
    pub async fn get_storage_info(
        pool: &PgPool,
        user_id: Uuid,
        asset_id: Uuid,
    ) -> Result<Option<AssetStorageInfo>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT rel_path, preview_path, (duration_seconds IS NOT NULL) AS is_video 
            FROM assets 
            WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        let info = row.map(|r| AssetStorageInfo {
            rel_path: r.get("rel_path"),
            preview_path: r.get("preview_path"),
            is_video: r.get("is_video"),
        });

        Ok(info)
    }

    /// Discovers immediate child folders under a given path with counts and latest cover thumbnail in ONE query.
    pub async fn get_sub_albums(
        pool: &PgPool,
        user_id: Uuid,
        current_folder: &str,
        media_type: Option<&str>,
    ) -> Result<Vec<SubAlbum>, sqlx::Error> {
        let clean_current = current_folder.trim().trim_matches('/');

        let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            WITH scoped_assets AS (
                SELECT 
                    folder_path,
                    thumb_path,
                    captured_at,
                    created_at
                FROM assets
                WHERE user_id = 
            "#,
        );
        builder.push_bind(user_id);
        builder.push(" AND deleted_at IS NULL ");

        if clean_current.is_empty() || clean_current == "root" {
            builder.push(" AND folder_path <> '' AND folder_path <> 'root' ");
        } else {
            builder.push(" AND folder_path LIKE ");
            builder.push_bind(format!("{clean_current}/%"));
        }

        if let Some(m_type) = media_type {
            match m_type {
                "photos" => {
                    builder.push(" AND duration_seconds IS NULL ");
                }
                "videos" => {
                    builder.push(" AND duration_seconds IS NOT NULL ");
                }
                _ => {}
            }
        }

        let prefix_depth = if clean_current.is_empty() || clean_current == "root" {
            1
        } else {
            clean_current.split('/').count() + 1
        };

        builder.push(
            r#"
            ),
            folder_partitions AS (
                SELECT 
                    SPLIT_PART(folder_path, '/', 
            "#,
        );
        builder.push_bind(prefix_depth as i32);
        builder.push(
            r#"
                    ) AS direct_name,
                    thumb_path,
                    captured_at,
                    created_at
                FROM scoped_assets
            )
            SELECT 
                direct_name,
                COUNT(*)::bigint AS item_count,
                (
                    ARRAY_AGG(thumb_path ORDER BY captured_at DESC NULLS LAST, created_at DESC)
                )[1] AS latest_thumb
            FROM folder_partitions
            WHERE direct_name <> '' AND direct_name <> 'root'
            GROUP BY direct_name
            ORDER BY direct_name ASC;
            "#,
        );

        let rows = builder.build().fetch_all(pool).await?;

        let albums = rows
            .into_iter()
            .map(|r| {
                let name: String = r.get("direct_name");
                let path = if clean_current.is_empty() || clean_current == "root" {
                    name.clone()
                } else {
                    format!("{clean_current}/{name}")
                };

                SubAlbum {
                    id: None,
                    name,
                    path,
                    count: r.get("item_count"),
                    cover_thumb: r.get("latest_thumb"),
                }
            })
            .collect();

        Ok(albums)
    }

    /// Fetches all assets soft-deleted more than 30 days ago, unlinks them from SSD, and deletes DB rows.
    pub async fn fetch_and_purge_expired_trash(
        pool: &PgPool,
        storage_root: &Path,
    ) -> Result<usize, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, user_id, rel_path, thumb_path, preview_path 
            FROM assets 
            WHERE deleted_at IS NOT NULL 
              AND deleted_at <= (CURRENT_TIMESTAMP - INTERVAL '30 days')
            "#,
        )
        .fetch_all(pool)
        .await?;

        let count = rows.len();

        for r in rows {
            let id: Uuid = r.get("id");
            let user_id: Uuid = r.get("user_id");
            let rel_path: String = r.get("rel_path");
            let thumb_path: String = r.get("thumb_path");
            let preview_path: String = r.get("preview_path");

            let orig_file = storage_root
                .join("users")
                .join(user_id.to_string())
                .join("originals")
                .join(&rel_path);

            let _ = tokio::fs::remove_file(orig_file).await;
            let _ = tokio::fs::remove_file(storage_root.join(&thumb_path)).await;
            let _ = tokio::fs::remove_file(storage_root.join(&preview_path)).await;

            let _ = sqlx::query("DELETE FROM assets WHERE id = $1 AND user_id = $2")
                .bind(id)
                .bind(user_id)
                .execute(pool)
                .await;
        }

        Ok(count)
    }

    /// Toggles deleted_at timestamp strictly for the authenticated user's asset.
    pub async fn toggle_soft_delete(
        pool: &PgPool,
        user_id: Uuid,
        id: Uuid,
    ) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
        let row = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            r#"
            UPDATE assets
            SET deleted_at = CASE 
                WHEN deleted_at IS NULL THEN CURRENT_TIMESTAMP 
                ELSE NULL 
            END,
            updated_at = CURRENT_TIMESTAMP
            WHERE id = $1 AND user_id = $2
            RETURNING deleted_at
            "#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Deletes asset rows and unlinks files from SSD strictly scoped to the user.
    pub async fn purge_asset(
        pool: &PgPool,
        user_id: Uuid,
        id: Uuid,
        storage_root: &Path,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query(
            "SELECT rel_path, thumb_path, preview_path FROM assets WHERE id = $1 AND user_id = $2",
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

        let orig_file = storage_root
            .join("users")
            .join(user_id.to_string())
            .join("originals")
            .join(&rel_path);

        let _ = tokio::fs::remove_file(orig_file).await;
        let _ = tokio::fs::remove_file(storage_root.join(&thumb_path)).await;
        let _ = tokio::fs::remove_file(storage_root.join(&preview_path)).await;

        let res = sqlx::query("DELETE FROM assets WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id)
            .execute(pool)
            .await?;

        Ok(res.rows_affected() > 0)
    }

    /// Batch soft-deletes/restores multiple assets belonging to the user in a single atomic update.
    pub async fn batch_toggle_soft_delete(
        pool: &PgPool,
        user_id: Uuid,
        ids: &[Uuid],
    ) -> Result<usize, sqlx::Error> {
        if ids.is_empty() {
            return Ok(0);
        }

        let res = sqlx::query(
            r#"
            UPDATE assets
            SET deleted_at = CASE 
                WHEN deleted_at IS NULL THEN CURRENT_TIMESTAMP 
                ELSE NULL 
            END,
            updated_at = CURRENT_TIMESTAMP
            WHERE user_id = $1 AND id = ANY($2::uuid[])
            "#,
        )
        .bind(user_id)
        .bind(ids)
        .execute(pool)
        .await?;

        Ok(res.rows_affected() as usize)
    }

    /// Batch hard-purges multiple assets belonging to the user.
    pub async fn batch_purge(
        pool: &PgPool,
        user_id: Uuid,
        ids: &[Uuid],
        storage_root: &Path,
    ) -> Result<usize, sqlx::Error> {
        if ids.is_empty() {
            return Ok(0);
        }

        let mut purged_count = 0;
        for id in ids {
            if let Ok(true) = Self::purge_asset(pool, user_id, *id, storage_root).await {
                purged_count += 1;
            }
        }

        Ok(purged_count)
    }

    pub async fn find_user_asset_by_sha256(
        pool: &PgPool,
        user_id: Uuid,
        sha256: &str,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        let row = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM assets WHERE user_id = $1 AND sha256 = $2 AND deleted_at IS NULL LIMIT 1"
        )
        .bind(user_id)
        .bind(sha256)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn get_cache_metadata(
        pool: &PgPool,
        asset_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<AssetCacheMetadata>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT thumb_path, mime_type FROM assets WHERE id = $1 AND user_id = $2"
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
        pool: &PgPool,
        user_id: Uuid,
        q: &MapLocationsQuery,
    ) -> Result<Vec<MapLocationPoint>, sqlx::Error> {
        let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT \
                a.id, \
                a.latitude AS lat, \
                a.longitude AS lng, \
                a.thumb_path \
            FROM assets a \
            WHERE a.user_id = ",
        );
        builder.push_bind(user_id);

        builder.push(" AND a.deleted_at IS NULL ");
        builder.push(" AND a.latitude IS NOT NULL ");
        builder.push(" AND a.longitude IS NOT NULL ");

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

        builder.push(" ORDER BY a.captured_at DESC NULLS LAST, a.created_at DESC");

        let points = builder
            .build_query_as::<MapLocationPoint>()
            .fetch_all(pool)
            .await?;

        Ok(points)
    }

    /// Query detected YOLO objects for a specific asset scoped to the user.
    pub async fn get_asset_objects(
        pool: &PgPool,
        user_id: Uuid,
        asset_id: Uuid,
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
            WHERE ao.asset_id = $1
              AND a.user_id = $2
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

    /// Query detected human poses with keypoints for a specific asset scoped to the user.
    pub async fn get_asset_poses(
        pool: &PgPool,
        user_id: Uuid,
        asset_id: Uuid,
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
            WHERE ap.asset_id = $1
              AND a.user_id = $2
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
            .map(|r| AssetPoseDetail {
                id: r.get("id"),
                asset_id: r.get("asset_id"),
                score: r.get("score"),
                bbox_x: r.get("bbox_x"),
                bbox_y: r.get("bbox_y"),
                bbox_w: r.get("bbox_w"),
                bbox_h: r.get("bbox_h"),
                keypoints: r.get("keypoints"),
            })
            .collect();

        Ok(poses)
    }
}