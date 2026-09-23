use serde::{Deserialize, Serialize};
use sqlx::{FromRow};

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

/// Ready-to-render DOM section mapping 1:1 to frontend template loops
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaSection {
    pub title: String, // e.g. "September 2026", "August 2026", "Undated"
    pub items: Vec<MediaItemSummary>,
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
    pub name: String,
    pub path: String,
    pub count: i64,
    pub cover_thumb: Option<String>,
}

/// The response sent over the wire to the frontend
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaPageResponse {
    pub albums: Vec<SubAlbum>,
    pub sections: Vec<MediaSection>, // Cleanly pre-grouped sections
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

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DynamicFiltersResponse {
    pub total_media: i64,
    pub photos_count: i64,
    pub videos_count: i64,
    pub min_date: Option<String>,
    pub max_date: Option<String>,
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