use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone)]
pub struct AssetStorageInfo {
    pub rel_path: String,
    pub preview_path: String,
    pub is_video: bool,
}

/// Raw row fetched directly from SQLite via SQLx
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct RawMediaRow {
    pub id: String,
    pub file_name: String,
    pub thumb_path: String,
    pub preview_path: String,
    pub aspect_ratio: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub mime_type: String,
    pub captured_at: Option<String>,
    pub is_favorite: i64,
    pub deleted_at: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// Dumb, wire-ready asset payload for the frontend
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaItemSummary {
    pub id: String,
    pub file_name: String,
    pub thumb_path: String,
    pub preview_path: String,
    pub aspect_ratio: f64,
    pub duration_seconds: Option<f64>,
    pub mime_type: String,
    pub captured_at: Option<String>,
    pub is_favorite: bool,
    pub days_remaining: Option<i64>, // Pre-calculated (e.g. 30 - days_passed)
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// Server-driven section contract:
/// - In Timeline mode: title is "Saturday, 12th September 2026", with month & year populated.
/// - In Random/Explore mode: title is None (or empty), so frontend renders a continuous grid with no dividers.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaSection {
    pub id: String,                    // Unique section key, e.g. "2026-09-12" or "explore-feed"
    pub title: Option<String>,         // Pre-formatted day title or None for random
    pub month: Option<String>,         // e.g. "Sep" (pre-calculated for timeline scrubber)
    pub year: Option<String>,          // e.g. "2026" (pre-calculated for timeline scrubber)
    pub date_iso: Option<String>,      // e.g. "2026-09-12"
    pub items: Vec<MediaItemSummary>,
}

impl From<RawMediaRow> for MediaItemSummary {
    fn from(r: RawMediaRow) -> Self {
        let days_remaining = r.deleted_at.as_deref().and_then(|d| {
            chrono::DateTime::parse_from_rfc3339(d)
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M:%S")
                        .map(|ndt| ndt.and_utc().fixed_offset())
                })
                .ok()
                .map(|del_time| {
                    let passed = (chrono::Utc::now() - del_time.with_timezone(&chrono::Utc)).num_days();
                    (30 - passed).max(0)
                })
        });

        Self {
            id: r.id,
            file_name: r.file_name,
            thumb_path: r.thumb_path,
            preview_path: r.preview_path,
            aspect_ratio: r.aspect_ratio.unwrap_or(1.0),
            duration_seconds: r.duration_seconds,
            mime_type: r.mime_type,
            captured_at: r.captured_at,
            is_favorite: r.is_favorite == 1,
            days_remaining,
            latitude: r.latitude,
            longitude: r.longitude,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewAssetRecord {
    pub id: String,
    pub user_id: String,
    pub sha256: String,
    pub file_name: String,
    pub rel_path: String,
    pub folder_path: String,
    pub thumb_path: String,
    pub preview_path: String,
    pub file_size_bytes: i64,
    pub mime_type: String,
    pub width: i64,
    pub height: i64,
    pub aspect_ratio: f64,
    pub duration_seconds: Option<f64>,
    pub captured_at: Option<String>,
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
    pub hour: Option<i32>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f64>,
    pub city: Option<String>,
    pub subdivision: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub author: Option<String>,
    pub source_platform: Option<String>,
    pub source_url: Option<String>,
    pub source_post_id: Option<String>,
    pub caption: Option<String>,
    pub clip_embedding: Option<Vec<u8>>,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct MediaQuery {
    pub album_id: Option<String>,
    pub sort: Option<String>,
    pub seed: Option<i64>,
    pub q: Option<String>,
    pub media_type: Option<String>,
    pub is_favorite: Option<bool>,

    pub person_id: Option<String>,
    pub tag: Option<String>,
    pub folder_path: Option<String>,

    pub city: Option<String>,
    pub country: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,

    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
    pub from: Option<String>,
    pub to: Option<String>,

    pub cursor_captured_at: Option<String>,
    pub cursor_id: Option<String>,
    pub limit: Option<i64>,
    pub show_trash: Option<bool>,

    #[serde(skip)]
    pub candidate_ids: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubAlbum {
    pub id: Option<String>,
    pub name: String,
    pub path: String,
    pub count: i64,
    pub cover_thumb: Option<String>,
}

/// Server-generated breadcrumb node eliminating frontend string-splitting
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BreadcrumbSegment {
    pub name: String,
    pub path: String,
    pub album_id: Option<String>,
}

/// The response sent over the wire to the frontend
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaPageResponse {
    pub albums: Vec<SubAlbum>,
    pub breadcrumbs: Vec<BreadcrumbSegment>, // Server-computed path breadcrumbs
    pub sections: Vec<MediaSection>,          // Ready-to-render pre-grouped sections
    pub next_cursor_captured_at: Option<String>,
    pub next_cursor_id: Option<String>,
    pub has_more: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct FilterOption {
    pub value: String,
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineBucket {
    pub year: String,
    pub month: String,      // e.g. "09"
    pub month_name: String, // e.g. "Sep"
    pub count: i64,
    pub latest_captured_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DynamicFiltersResponse {
    pub total_media: i64,
    pub photos_count: i64,
    pub videos_count: i64,
    pub min_date: Option<String>,
    pub max_date: Option<String>,
    pub timeline: Vec<TimelineBucket>,
    pub times_of_day: Vec<FilterOption>,
    pub people: Vec<FilterOption>,
    pub tags: Vec<FilterOption>,
    pub locations: Vec<FilterOption>,
    pub cameras: Vec<FilterOption>,
    pub albums: Vec<FilterOption>,
}

#[derive(Debug, Deserialize)]
pub struct BatchActionRequest {
    pub ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchActionResponse {
    pub affected_count: usize,
}

#[derive(Serialize)]
pub struct FavoriteToggleResponse {
    pub asset_id: String,
    pub is_favorite: bool,
}

#[derive(Serialize)]
pub struct SoftDeleteResponse {
    pub id: String,
    pub is_deleted: bool,
    pub deleted_at: Option<String>,
}

#[derive(Serialize)]
pub struct SimilarMediaItem {
    pub id: String,
    pub thumb_path: String,
    pub mime_type: String,
    pub similarity: f32,
}

pub struct AssetCacheMetadata {
    pub thumb_path: String,
    pub mime_type: String,
}

/// Lightweight point returned strictly for map markers and clustering.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MapLocationPoint {
    pub id: String,
    pub lat: f64,
    pub lng: f64,
    pub thumb_path: String,
}

/// Optional viewport bounding-box filter parameters
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MapLocationsQuery {
    pub min_lat: Option<f64>,
    pub max_lat: Option<f64>,
    pub min_lng: Option<f64>,
    pub max_lng: Option<f64>,
    pub folder_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AssetObjectDetail {
    pub id: String,
    pub asset_id: String,
    pub class_id: i32,
    pub label: String,
    pub score: f32,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AssetPoseDetail {
    pub id: String,
    pub asset_id: String,
    pub score: f32,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    pub keypoints: serde_json::Value,
}