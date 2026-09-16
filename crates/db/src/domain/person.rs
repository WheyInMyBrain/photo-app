use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct PersonCard {
    pub id: String,
    pub name: Option<String>,
    pub face_count: i64,
    pub avatar_thumb: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct AssetFaceDetail {
    pub face_id: String,
    pub person_id: Option<String>,
    pub person_name: Option<String>,
    pub face_thumb_path: String,
    pub bbox_x: f64,
    pub bbox_y: f64,
    pub bbox_w: f64,
    pub bbox_h: f64,
    pub score: f64,
    pub is_verified: bool,
}