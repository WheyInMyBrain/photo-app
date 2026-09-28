// photo-app/crates/media_processing/src/models.rs
use serde::{Deserialize, Serialize};

use crate::metadata::ExtractedMetadata;
use crate::simd::EMBEDDING_DIM;

#[derive(Clone, Debug)]
pub struct KnownPersonCluster {
    pub person_id: String,
    pub face_count: i32,
    pub cover_face_id: Option<String>,
    pub exemplars: Vec<[f32; EMBEDDING_DIM]>,
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

#[derive(Default, Clone, Debug)]
pub struct ClusteredFacesResult {
    pub detected_faces: Vec<NewFaceRecord>,
    pub updated_clusters: Vec<ClusterUpdate>,
    pub new_persons: Vec<NewPersonRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagPrediction {
    pub name: String,
    pub confidence: f32,
}

// ---------------------------------------------------------------------------
// YOLO Domain Models (Serialized to DB JSON columns)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectedObject {
    pub class_id: usize,
    pub label: String,
    pub score: f32,
    /// Normalized coordinates [0.0..1.0] relative to original image
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keypoint {
    /// Normalized coordinates [0.0..1.0] relative to original image
    pub x: f32,
    pub y: f32,
    pub score: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectedPose {
    pub score: f32,
    /// Normalized coordinates [0.0..1.0] of human bounding box
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// 17 COCO keypoints:
    /// [Nose, L_Eye, R_Eye, L_Ear, R_Ear, L_Shoulder, R_Shoulder, L_Elbow, R_Elbow,
    ///  L_Wrist, R_Wrist, L_Hip, R_Hip, L_Knee, R_Knee, L_Ankle, R_Ankle]
    pub keypoints: Vec<Keypoint>,
}

// ---------------------------------------------------------------------------
// Pipeline Results (Internal structs, no serde required)
// ---------------------------------------------------------------------------

/// Phase 1 Output: derivatives and file metadata (fast, non-AI)
#[derive(Debug, Clone)]
pub struct DerivativeResult {
    pub mime_type: String,
    pub width: i64,
    pub height: i64,
    pub aspect_ratio: f64,
    pub duration_seconds: Option<f64>,
    pub thumb_path: String,
    pub preview_path: String,
    pub meta: ExtractedMetadata,
}

/// Phase 2 Output: enriched data from ML inference (tags, faces, vector embedding, objects, poses)
#[derive(Debug, Clone, Default)]
pub struct AiEnrichmentResult {
    pub detected_faces: Vec<NewFaceRecord>,
    pub updated_clusters: Vec<ClusterUpdate>,
    pub new_persons: Vec<NewPersonRecord>,
    pub tags: Vec<TagPrediction>,
    pub clip_embedding: Option<Vec<f32>>,
    pub objects: Vec<DetectedObject>,
    pub poses: Vec<DetectedPose>,
}

/// Kept for backwards compatibility if needed elsewhere in the codebase
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
    pub clip_embedding: Option<Vec<f32>>,
    pub objects: Vec<DetectedObject>,
    pub poses: Vec<DetectedPose>,
}