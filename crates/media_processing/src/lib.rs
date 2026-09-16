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
pub mod clip_cache;
pub mod simd;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub use clip_engine::ClipEngine;
pub use face_engine::FaceEngine;
pub use metadata::{ExtractedMetadata, MetadataService};
pub use models::*;
pub use storage::StorageService;
pub use tag_engine::TagEngine;
pub use simd::{dot_product_512, normalize_l2, EMBEDDING_DIM};
pub use clip_cache::{ClipCacheManager, CachedEmbedding, VectorSearchResult};
pub use video_processor::{VideoProcessor, VideoDerivatives, VideoMetadata};

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

    // =========================================================================
    // PHASE 1: Fast Derivatives & Metadata Extraction (Runs in milliseconds)
    // =========================================================================

    pub fn process_derivatives_sync(
        disk_path: &Path,
        asset_id: &str,
        thumbs_root: &Path,
    ) -> Result<DerivativeResult, Box<dyn std::error::Error + Send + Sync>> {
        let ext = disk_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let shard = if asset_id.len() >= 2 { &asset_id[0..2] } else { "misc" };
        let shard_dir = thumbs_root.join(shard);
        std::fs::create_dir_all(&shard_dir)?;

        if video_processor::VideoProcessor::is_video_or_anim(&ext) {
            Self::generate_video_derivatives(disk_path, asset_id, &shard_dir, &ext)
        } else {
            Self::generate_image_derivatives(disk_path, asset_id, &shard_dir, &ext)
        }
    }

    fn generate_image_derivatives(
        disk_path: &Path,
        asset_id: &str,
        shard_dir: &Path,
        ext: &str,
    ) -> Result<DerivativeResult, Box<dyn std::error::Error + Send + Sync>> {
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

        Ok(DerivativeResult {
            mime_type,
            width,
            height,
            aspect_ratio,
            duration_seconds: None,
            thumb_path,
            preview_path,
            meta,
        })
    }

    fn generate_video_derivatives(
        disk_path: &Path,
        asset_id: &str,
        shard_dir: &Path,
        ext: &str,
    ) -> Result<DerivativeResult, Box<dyn std::error::Error + Send + Sync>> {
        let v_meta = video_processor::VideoProcessor::extract_metadata(disk_path)?;

        let derivatives = video_processor::VideoProcessor::generate_all_derivatives(
            disk_path,
            asset_id,
            shard_dir,
            v_meta.duration_seconds,
        )?;

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

        let mime_type = match ext {
            "gif" => "image/gif",
            "mp4" => "video/mp4",
            "mov" => "video/quicktime",
            "webm" => "video/webm",
            _ => "video/mp4",
        }.to_string();

        Ok(DerivativeResult {
            mime_type,
            width: v_meta.width,
            height: v_meta.height,
            aspect_ratio,
            duration_seconds: Some(v_meta.duration_seconds),
            thumb_path: derivatives.thumb_rel,
            preview_path: derivatives.preview_rel,
            meta,
        })
    }

    // =========================================================================
    // PHASE 2: Background AI Pipeline (Runs completely independently)
    // =========================================================================

    pub fn process_ai_sync(
        &self,
        disk_path: &Path,
        asset_id: &str,
        thumbs_root: &Path,
        existing_clusters: Vec<KnownPersonCluster>,
    ) -> Result<AiEnrichmentResult, Box<dyn std::error::Error + Send + Sync>> {
        let ext = disk_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let shard = if asset_id.len() >= 2 { &asset_id[0..2] } else { "misc" };
        let shard_dir = thumbs_root.join(shard);

        if video_processor::VideoProcessor::is_video_or_anim(&ext) {
            self.process_video_ai(disk_path, asset_id, &shard_dir, thumbs_root, existing_clusters)
        } else {
            self.process_image_ai(disk_path, asset_id, &shard_dir, thumbs_root, existing_clusters)
        }
    }

    fn process_image_ai(
        &self,
        disk_path: &Path,
        asset_id: &str,
        shard_dir: &Path,
        thumbs_root: &Path,
        existing_clusters: Vec<KnownPersonCluster>,
    ) -> Result<AiEnrichmentResult, Box<dyn std::error::Error + Send + Sync>> {
        // Look for the preview generated in Phase 1 (e.g. {shard}/{asset_id}_preview.webp or .jpg)
        let candidate_preview = shard_dir.join(format!("{}_preview.webp", asset_id));
        let candidate_preview_jpg = shard_dir.join(format!("{}_preview.jpg", asset_id));

        // Use the lightweight preview if present; otherwise fall back to the original file
        let source_path = if candidate_preview.exists() {
            &candidate_preview
        } else if candidate_preview_jpg.exists() {
            &candidate_preview_jpg
        } else {
            disk_path
        };

        // Decodes a small ~1080p preview in ~5ms instead of a 48MP raw image
        let img = image_processor::ImageProcessor::load_image(source_path)?;

        // 1. Tags
        let tags = self.tag_engine.tag_image(&img, 0.35)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, confidence)| TagPrediction { name, confidence })
            .collect();

        // 2. Face Detection & Clustering
        let faces_res = FaceClusterer::cluster_single_image(
            &img,
            &self.face_engine,
            thumbs_root,
            existing_clusters,
        )?;

        // 3. Visual CLIP Embedding
        let clip_embedding = self.clip_engine.extract_image_embedding(&img).ok();

        Ok(AiEnrichmentResult {
            detected_faces: faces_res.detected_faces,
            updated_clusters: faces_res.updated_clusters,
            new_persons: faces_res.new_persons,
            tags,
            clip_embedding,
        })
    }

    fn process_video_ai(
        &self,
        disk_path: &Path,
        asset_id: &str,
        shard_dir: &Path,
        thumbs_root: &Path,
        existing_clusters: Vec<KnownPersonCluster>,
    ) -> Result<AiEnrichmentResult, Box<dyn std::error::Error + Send + Sync>> {
        // Determine whether to sample from the lightweight 720p H.264 preview or the original
        let candidate_preview_mp4 = shard_dir.join(format!("{}_preview.mp4", asset_id));
        let video_source = if candidate_preview_mp4.exists() {
            &candidate_preview_mp4
        } else {
            disk_path
        };

        // Extract metadata for duration
        let v_meta = video_processor::VideoProcessor::extract_metadata(video_source)
            .or_else(|_| video_processor::VideoProcessor::extract_metadata(disk_path))?;

        let sample_count = if v_meta.duration_seconds > 60.0 {
            4
        } else if v_meta.duration_seconds > 1.0 {
            3
        } else {
            1
        };

        // Samples 720p frames rapidly from the preview mp4
        let sampled_frames = video_processor::VideoProcessor::sample_frames(
            video_source,
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

        // 3. Pooled CLIP embedding
        let clip_embedding = self.clip_engine.extract_video_embedding(&sampled_frames).ok();

        Ok(AiEnrichmentResult {
            detected_faces: faces_res.detected_faces,
            updated_clusters: faces_res.updated_clusters,
            new_persons: faces_res.new_persons,
            tags,
            clip_embedding,
        })
    }
}