use chrono::{DateTime, Utc};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AssetStorageInfo {
    pub rel_path: String,
    pub preview_path: String,
    pub is_video: bool,
}

/// Raw row fetched directly from PostgreSQL via SQLx
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct RawMediaRow {
    pub id: Uuid,
    pub file_name: String,
    pub thumb_path: String,
    pub preview_path: String,
    pub aspect_ratio: Option<f32>,
    pub duration_seconds: Option<f32>,
    pub mime_type: String,
    pub captured_at: Option<DateTime<Utc>>,
    pub is_favorite: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// Wire-ready asset payload for the frontend
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaItemSummary {
    pub id: Uuid,
    pub file_name: String,
    pub thumb_path: String,
    pub preview_path: String,
    pub aspect_ratio: f32,
    pub duration_seconds: Option<f32>,
    pub mime_type: String,
    pub captured_at: Option<String>,
    pub is_favorite: bool,
    pub days_remaining: Option<i64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaSection {
    pub id: String,
    pub title: Option<String>,
    pub month: Option<String>,
    pub year: Option<String>,
    pub date_iso: Option<String>,
    pub items: Vec<MediaItemSummary>,
}

impl From<RawMediaRow> for MediaItemSummary {
    fn from(r: RawMediaRow) -> Self {
        let days_remaining = r.deleted_at.map(|del_time| {
            let passed = (Utc::now() - del_time).num_days();
            (30 - passed).max(0)
        });

        Self {
            id: r.id,
            file_name: r.file_name,
            thumb_path: r.thumb_path,
            preview_path: r.preview_path,
            aspect_ratio: r.aspect_ratio.unwrap_or(1.0),
            duration_seconds: r.duration_seconds,
            mime_type: r.mime_type,
            captured_at: r.captured_at.map(|d| d.to_rfc3339()),
            is_favorite: r.is_favorite,
            days_remaining,
            latitude: r.latitude,
            longitude: r.longitude,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewAssetRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub sha256: String,
    pub file_name: String,
    pub rel_path: String,
    pub folder_path: String,
    pub thumb_path: String,
    pub preview_path: String,
    pub file_size_bytes: i64,
    pub mime_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub aspect_ratio: Option<f32>,
    pub duration_seconds: Option<f32>,
    pub captured_at: Option<DateTime<Utc>>,
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
    pub hour: Option<i32>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f32>,
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
    pub clip_embedding: Option<Vec<u8>>, // Serialized f32 bytes or raw vec
}

fn empty_string_as_none<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let opt = Option::<String>::deserialize(de)?;
    match opt.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => s.parse::<T>().map(Some).map_err(de::Error::custom),
    }
}

fn empty_string_as_bool<'de, D>(de: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(de)?;
    match opt.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some("true") | Some("1") => Ok(Some(true)),
        Some("false") | Some("0") => Ok(Some(false)),
        Some(other) => Err(de::Error::custom(format!("invalid boolean: {other}"))),
    }
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct MediaQuery {
    pub album_id: Option<Uuid>,
    pub sort: Option<String>,

    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub seed: Option<i64>,

    pub q: Option<String>,
    pub media_type: Option<String>,

    #[serde(default, deserialize_with = "empty_string_as_bool")]
    pub is_favorite: Option<bool>,

    pub person_id: Option<String>,
    pub tag: Option<String>,
    pub folder_path: Option<String>,

    pub city: Option<String>,
    pub country: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,

    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub year: Option<i32>,

    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub month: Option<i32>,

    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub day: Option<i32>,

    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,

    pub cursor_captured_at: Option<DateTime<Utc>>,
    pub cursor_id: Option<Uuid>,

    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub limit: Option<i64>,

    #[serde(default, deserialize_with = "empty_string_as_bool")]
    pub show_trash: Option<bool>,

    #[serde(skip)]
    pub candidate_ids: Option<Vec<Uuid>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubAlbum {
    pub id: Option<Uuid>,
    pub name: String,
    pub path: String,
    pub count: i64,
    pub cover_thumb: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BreadcrumbSegment {
    pub name: String,
    pub path: String,
    pub album_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaPageResponse {
    pub albums: Vec<SubAlbum>,
    pub breadcrumbs: Vec<BreadcrumbSegment>,
    pub sections: Vec<MediaSection>,
    pub next_cursor_captured_at: Option<DateTime<Utc>>,
    pub next_cursor_id: Option<Uuid>,
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
    pub month: String,
    pub month_name: String,
    pub count: i64,
    pub latest_captured_at: Option<DateTime<Utc>>,
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
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct BatchActionResponse {
    pub affected_count: usize,
}

#[derive(Serialize)]
pub struct FavoriteToggleResponse {
    pub asset_id: Uuid,
    pub is_favorite: bool,
}

#[derive(Serialize)]
pub struct SoftDeleteResponse {
    pub id: Uuid,
    pub is_deleted: bool,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct SimilarMediaItem {
    pub id: Uuid,
    pub thumb_path: String,
    pub mime_type: String,
    pub similarity: f32,
}

pub struct AssetCacheMetadata {
    pub thumb_path: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MapLocationPoint {
    pub id: Uuid,
    pub lat: f64,
    pub lng: f64,
    pub thumb_path: String,
}

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
    pub id: Uuid,
    pub asset_id: Uuid,
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
    pub id: Uuid,
    pub asset_id: Uuid,
    pub score: f32,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    pub keypoints: serde_json::Value,
}