use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct AssetStorageInfo {
    pub rel_path: String,
    pub preview_path: String,
    pub is_video: bool,
}

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone, Debug)]
pub struct MediaSummary {
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
    pub clip_embedding: Option<Vec<u8>>,
}

/// The unified filter parameters struct for querying media across the entire app.
/// Every UI view (timeline, favorites, people, places, cameras, tags) uses this.
#[allow(dead_code)]
#[derive(Deserialize, Debug, Default, Clone)]
pub struct MediaQuery {
    // Full-text search term
    pub q: Option<String>,
    // Media & curation flags
    pub media_type: Option<String>, // "all" | "photos" | "videos"
    pub is_favorite: Option<bool>,

    // Entity Associations
    pub person_id: Option<String>,
    pub tag: Option<String>,
    pub folder_path: Option<String>,

    // Geographic & Hardware Filters
    pub city: Option<String>,
    pub country: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,

    // Temporal Filters
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
    pub from: Option<String>, // "YYYY-MM-DD"
    pub to: Option<String>,   // "YYYY-MM-DD"

    // Keyset Cursor Pagination & Access Control
    pub cursor_captured_at: Option<String>,
    pub cursor_id: Option<String>,
    pub limit: Option<i64>,
    pub show_trash: Option<bool>,

    // Internal vector search candidate IDs (not sent by frontend, populated by backend)
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

/// Standardized paginated response returned by the unified media query
#[derive(Serialize, Clone, Debug)]
pub struct MediaPageResponse {
    pub albums: Vec<SubAlbum>,
    pub items: Vec<MediaSummary>,
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