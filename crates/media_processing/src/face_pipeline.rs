use image::{imageops::FilterType, DynamicImage, ImageFormat};
use std::path::Path;
use uuid::Uuid;

use super::face_engine::FaceEngine;
use media_processing::{normalize_l2, EMBEDDING_DIM};

pub struct FacePipeline;

pub struct ExtractedFace {
    pub face_id: String,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    pub detection_score: f32,
    pub avatar_rel_path: String,
    pub embedding: [f32; EMBEDDING_DIM],
}

impl FacePipeline {
    // With 5-point alignment, 0.40–0.45 is the standard threshold for ArcFace w600k_r50
    pub const MATCH_THRESHOLD: f32 = 0.42;

    pub fn detect_and_extract_faces_sync(
        engine: &FaceEngine,
        user_id: &str,
        user_faces_dir: &Path,
        img: &DynamicImage,
    ) -> Result<Vec<ExtractedFace>, Box<dyn std::error::Error + Send + Sync>> {
        let detections = engine.detect_faces(img, 0.50, 0.35)?;
        if detections.is_empty() {
            return Ok(Vec::new());
        }

        std::fs::create_dir_all(user_faces_dir)?;
        let mut results = Vec::with_capacity(detections.len());

        for det in detections {
            let face_id = Uuid::new_v4().to_string();

            // 1. Create the visual preview avatar for the UI (square crop)
            let chip_view = media_processing::math::view_face_chip(img, det.x, det.y, det.w, det.h);
            let avatar = image::imageops::resize(&chip_view, 128, 128, FilterType::Triangle);
            let avatar_rel_path = format!("users/{}/thumbs/faces/{}.webp", user_id, face_id);
            let avatar_abs_path = user_faces_dir.join(format!("{}.webp", face_id));

            let mut out = std::fs::File::create(&avatar_abs_path)?;
            DynamicImage::ImageRgba8(avatar).write_to(&mut out, ImageFormat::WebP)?;

            // 2. Generate canonical 5-point aligned 112x112 chip for ArcFace
            let aligned_chip = FaceEngine::align_face_112(img, &det.landmarks);

            // 3. Extract high-precision embedding
            let raw_embedding = engine.extract_embedding(&aligned_chip)?;
            if raw_embedding.len() != EMBEDDING_DIM {
                continue;
            }

            let mut embedding = [0.0f32; EMBEDDING_DIM];
            embedding.copy_from_slice(&raw_embedding);
            normalize_l2(&mut embedding);

            results.push(ExtractedFace {
                face_id,
                bbox_x: det.x,
                bbox_y: det.y,
                bbox_w: det.w,
                bbox_h: det.h,
                detection_score: det.score,
                avatar_rel_path,
                embedding,
            });
        }

        Ok(results)
    }
}