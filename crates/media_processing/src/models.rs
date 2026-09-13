use crate::metadata::ExtractedMetadata;

#[derive(Clone, Debug)]
pub struct KnownPersonCluster {
    pub person_id: String,
    pub centroid: Vec<f32>,
    pub face_count: i32,
    pub cover_face_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NewFaceRecord {
    pub face_id: String,
    pub person_id: String,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    pub score: f32,
    pub face_thumb_rel_path: String,
    pub embedding: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct ClusterUpdate {
    pub person_id: String,
    pub new_centroid: Vec<f32>,
    pub new_face_count: i32,
    pub new_cover_face_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NewPersonRecord {
    pub person_id: String,
    pub cover_face_id: String,
    pub centroid: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct TagPrediction {
    pub name: String,
    pub confidence: f32,
}

#[derive(Default, Clone, Debug)]
pub struct ClusteredFacesResult {
    pub detected_faces: Vec<NewFaceRecord>,
    pub updated_clusters: Vec<ClusterUpdate>,
    pub new_persons: Vec<NewPersonRecord>,
}

/// Consolidated output returned to the caller
#[derive(Clone, Debug)]
pub struct ProcessedMediaResult {
    pub mime_type: String,
    pub width: i64,
    pub height: i64,
    pub aspect_ratio: f64,
    pub duration_seconds: Option<f64>,
    pub thumb_path: String,
    pub preview_path: String,
    pub meta: ExtractedMetadata,

    // AI Outputs
    pub detected_faces: Vec<NewFaceRecord>,
    pub updated_clusters: Vec<ClusterUpdate>,
    pub new_persons: Vec<NewPersonRecord>,
    pub tags: Vec<TagPrediction>,
    pub clip_embedding: Option<Vec<f32>>, // <-- Visual embedding slot for CLIP
}