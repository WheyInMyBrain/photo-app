pub mod clip_engine;
pub mod clustering;
pub mod face_detector;
pub mod face_engine;
pub mod image_processor;
pub mod math;
pub mod metadata;
pub mod models;
pub mod storage;
pub mod tag_engine;
pub mod video_processor;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub use clip_engine::ClipEngine;
pub use face_engine::FaceEngine;
pub use metadata::{ExtractedMetadata, MetadataService};
pub use models::*;
pub use storage::StorageService;
pub use tag_engine::TagEngine;

use clustering::FaceClusterer;

#[derive(Clone)]
pub struct MediaEngine {
    pub face_engine: Arc<FaceEngine>,
    pub tag_engine: Arc<TagEngine>,
    pub clip_engine: Arc<ClipEngine>,
}

impl MediaEngine {
    pub fn new(
        face_engine: Arc<FaceEngine>,
        tag_engine: Arc<TagEngine>,
        clip_engine: Arc<ClipEngine>,
    ) -> Self {
        Self {
            face_engine,
            tag_engine,
            clip_engine,
        }
    }

    pub fn process_asset_sync(
        &self,
        disk_path: &Path,
        asset_id: &str,
        thumbs_root: &Path,
        existing_clusters: Vec<KnownPersonCluster>,
        run_ai: bool,
    ) -> Result<ProcessedMediaResult, Box<dyn std::error::Error + Send + Sync>> {
        let ext = disk_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let shard = if asset_id.len() >= 2 { &asset_id[0..2] } else { "misc" };
        let shard_dir = thumbs_root.join(shard);
        std::fs::create_dir_all(&shard_dir)?;

        if video_processor::VideoProcessor::is_video(&ext) {
            self.process_video(disk_path, asset_id, thumbs_root, &shard_dir, &ext, existing_clusters, run_ai)
        } else {
            self.process_image(disk_path, asset_id, thumbs_root, &shard_dir, &ext, existing_clusters, run_ai)
        }
    }

    fn process_image(
        &self,
        disk_path: &Path,
        asset_id: &str,
        thumbs_root: &Path,
        shard_dir: &Path,
        ext: &str,
        existing_clusters: Vec<KnownPersonCluster>,
        run_ai: bool,
    ) -> Result<ProcessedMediaResult, Box<dyn std::error::Error + Send + Sync>> {
        let meta = metadata::MetadataService::extract(disk_path);
        let img = image_processor::ImageProcessor::load_image(disk_path)?;
        let width = img.width() as i64;
        let height = img.height() as i64;
        let aspect_ratio = if height > 0 { width as f64 / height as f64 } else { 1.0 };

        let (thumb_path, preview_path) =
            image_processor::ImageProcessor::generate_derivatives(&img, asset_id, shard_dir)?;

        let mime_type = match ext {
            "heic" | "heif" => "image/heic",
            "png" => "image/png",
            "webp" => "image/webp",
            _ => "image/jpeg",
        }.to_string();

        if !run_ai {
            return Ok(ProcessedMediaResult {
                mime_type,
                width,
                height,
                aspect_ratio,
                duration_seconds: None,
                thumb_path,
                preview_path,
                meta,
                detected_faces: Vec::new(),
                updated_clusters: Vec::new(),
                new_persons: Vec::new(),
                tags: Vec::new(),
                clip_embedding: None,
            });
        }

        // 1. Tags
        let tags = self.tag_engine.tag_image(&img, 0.35)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, confidence)| TagPrediction { name, confidence })
            .collect();

        // 2. Faces
        let faces_res = FaceClusterer::cluster_single_image(
            &img,
            &self.face_engine,
            thumbs_root,
            existing_clusters,
        )?;

        // 3. Visual CLIP Embedding
        let clip_embedding = self.clip_engine.extract_image_embedding(&img).ok();

        Ok(ProcessedMediaResult {
            mime_type,
            width,
            height,
            aspect_ratio,
            duration_seconds: None,
            thumb_path,
            preview_path,
            meta,
            detected_faces: faces_res.detected_faces,
            updated_clusters: faces_res.updated_clusters,
            new_persons: faces_res.new_persons,
            tags,
            clip_embedding,
        })
    }

    fn process_video(
        &self,
        disk_path: &Path,
        asset_id: &str,
        thumbs_root: &Path,
        shard_dir: &Path,
        ext: &str,
        existing_clusters: Vec<KnownPersonCluster>,
        run_ai: bool,
    ) -> Result<ProcessedMediaResult, Box<dyn std::error::Error + Send + Sync>> {
        let v_meta = video_processor::VideoProcessor::extract_metadata(disk_path)?;
        let (thumb_path, preview_path) =
            video_processor::VideoProcessor::generate_poster(disk_path, asset_id, shard_dir)?;

        let aspect_ratio = if v_meta.height > 0 {
            v_meta.width as f64 / v_meta.height as f64
        } else {
            1.777
        };

        let mut meta = ExtractedMetadata::default();
        meta.captured_at = v_meta.captured_at;
        meta.camera_make = v_meta.camera_make;
        meta.camera_model = v_meta.camera_model;

        if let Some(ref dt) = meta.captured_at {
            let p: Vec<&str> = dt.split(|c| c == '-' || c == 'T' || c == ' ' || c == ':').collect();
            if p.len() >= 4 {
                meta.year = p[0].parse().ok();
                meta.month = p[1].parse().ok();
                meta.day = p[2].parse().ok();
                meta.hour = p[3].parse().ok();
            }
        }

        let mime_type = if ext == "mp4" { "video/mp4" } else { "video/quicktime" }.to_string();

        if !run_ai {
            return Ok(ProcessedMediaResult {
                mime_type,
                width: v_meta.width,
                height: v_meta.height,
                aspect_ratio,
                duration_seconds: Some(v_meta.duration_seconds),
                thumb_path,
                preview_path,
                meta,
                detected_faces: Vec::new(),
                updated_clusters: Vec::new(),
                new_persons: Vec::new(),
                tags: Vec::new(),
                clip_embedding: None,
            });
        }

        let sample_count = if v_meta.duration_seconds > 60.0 { 4 } else { 3 };
        let sampled_frames = video_processor::VideoProcessor::sample_frames(
            disk_path,
            v_meta.duration_seconds,
            sample_count,
        );

        // 1. Tags across sampled frames
        let mut tag_map: HashMap<String, f32> = HashMap::new();
        for frame in &sampled_frames {
            if let Ok(predictions) = self.tag_engine.tag_image(frame, 0.35) {
                for (name, conf) in predictions {
                    let entry = tag_map.entry(name).or_insert(conf);
                    if conf > *entry {
                        *entry = conf;
                    }
                }
            }
        }
        let tags = tag_map
            .into_iter()
            .map(|(name, confidence)| TagPrediction { name, confidence })
            .collect();

        // 2. Faces across sampled frames
        let faces_res = FaceClusterer::cluster_video_frames(
            &sampled_frames,
            &self.face_engine,
            thumbs_root,
            existing_clusters,
        )?;

        // 3. Pool CLIP embeddings across all sampled video frames
        let clip_embedding = self.clip_engine.extract_video_embedding(&sampled_frames).ok();

        Ok(ProcessedMediaResult {
            mime_type,
            width: v_meta.width,
            height: v_meta.height,
            aspect_ratio,
            duration_seconds: Some(v_meta.duration_seconds),
            thumb_path,
            preview_path,
            meta,
            detected_faces: faces_res.detected_faces,
            updated_clusters: faces_res.updated_clusters,
            new_persons: faces_res.new_persons,
            tags,
            clip_embedding,
        })
    }
}