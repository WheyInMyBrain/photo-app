pub mod face_detector;
pub mod face_engine;
pub mod image_processor;
pub mod metadata;
pub mod storage;
pub mod tag_engine;
pub mod video_processor;

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use uuid::Uuid;

pub use face_engine::FaceEngine;
pub use metadata::{ExtractedMetadata, MetadataService};
pub use storage::StorageService;
pub use tag_engine::TagEngine;

// -----------------------------------------------------------------------------
// Core Data Contracts
// -----------------------------------------------------------------------------

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

#[derive(Default)]
struct ClusteredFacesResult {
    detected_faces: Vec<NewFaceRecord>,
    updated_clusters: Vec<ClusterUpdate>,
    new_persons: Vec<NewPersonRecord>,
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
}

// -----------------------------------------------------------------------------
// Unified Media Engine
// -----------------------------------------------------------------------------

#[derive(Clone)]
pub struct MediaEngine {
    pub face_engine: Arc<FaceEngine>,
    pub tag_engine: Arc<TagEngine>,
}

impl MediaEngine {
    pub fn new(face_engine: Arc<FaceEngine>, tag_engine: Arc<TagEngine>) -> Self {
        Self {
            face_engine,
            tag_engine,
        }
    }

    /// Top-level coordinator: delegates cleanly to video or image pipelines
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
            self.process_video(
                disk_path,
                asset_id,
                thumbs_root,
                &shard_dir,
                &ext,
                existing_clusters,
                run_ai,
            )
        } else {
            self.process_image(
                disk_path,
                asset_id,
                thumbs_root,
                &shard_dir,
                &ext,
                existing_clusters,
                run_ai,
            )
        }
    }

    // -------------------------------------------------------------------------
    // Video Processing Pipeline
    // -------------------------------------------------------------------------
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

        let aspect = if v_meta.height > 0 {
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

        let mime = if ext == "mp4" { "video/mp4" } else { "video/quicktime" };

        if !run_ai {
            return Ok(ProcessedMediaResult {
                mime_type: mime.to_string(),
                width: v_meta.width,
                height: v_meta.height,
                aspect_ratio: aspect,
                duration_seconds: Some(v_meta.duration_seconds),
                thumb_path,
                preview_path,
                meta,
                detected_faces: Vec::new(),
                updated_clusters: Vec::new(),
                new_persons: Vec::new(),
                tags: Vec::new(),
            });
        }

        // 1. Sample 3-4 representative frames across video duration
        let sample_count = if v_meta.duration_seconds > 60.0 { 4 } else { 3 };
        let sampled_frames = video_processor::VideoProcessor::sample_frames(
            disk_path,
            v_meta.duration_seconds,
            sample_count,
        );

        // 2. Aggregate & Deduplicate Tags across all sampled frames
        let mut tag_map: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
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
        let tags: Vec<TagPrediction> = tag_map
            .into_iter()
            .map(|(name, confidence)| TagPrediction { name, confidence })
            .collect();

        // 3. Detect & Cluster Faces across all sampled frames
        let faces_res = self.process_video_faces(&sampled_frames, thumbs_root, existing_clusters)?;

        Ok(ProcessedMediaResult {
            mime_type: mime.to_string(),
            width: v_meta.width,
            height: v_meta.height,
            aspect_ratio: aspect,
            duration_seconds: Some(v_meta.duration_seconds),
            thumb_path,
            preview_path,
            meta,
            detected_faces: faces_res.detected_faces,
            updated_clusters: faces_res.updated_clusters,
            new_persons: faces_res.new_persons,
            tags,
        })
    }

    // -------------------------------------------------------------------------
    // Video Face Detection & Intra-Video Deduplication
    // -------------------------------------------------------------------------
    fn process_video_faces(
        &self,
        frames: &[DynamicImage],
        thumbs_root: &Path,
        mut existing_clusters: Vec<KnownPersonCluster>,
    ) -> Result<ClusteredFacesResult, Box<dyn std::error::Error + Send + Sync>> {
        if frames.is_empty() {
            return Ok(ClusteredFacesResult::default());
        }

        let faces_dir = thumbs_root.join("faces");
        std::fs::create_dir_all(&faces_dir)?;

        // Map: person_id -> highest scoring face record in this video
        let mut best_faces_per_person: std::collections::HashMap<String, NewFaceRecord> =
            std::collections::HashMap::new();
        let mut updated_clusters: Vec<ClusterUpdate> = Vec::new();
        let mut new_persons: Vec<NewPersonRecord> = Vec::new();

        for frame in frames {
            let detections = match self.face_engine.detect_faces(frame, 0.50, 0.35) {
                Ok(dets) if !dets.is_empty() => dets,
                _ => continue,
            };

            for det in detections {
                let chip = crop_face_chip(frame, det.x, det.y, det.w, det.h);

                let embedding = match self.face_engine.extract_embedding(&chip) {
                    Ok(emb) => emb,
                    Err(_) => continue,
                };

                // Match against existing person clusters
                let mut best_match: Option<(usize, f32)> = None;
                for (idx, cluster) in existing_clusters.iter().enumerate() {
                    let sim = cosine_similarity(&embedding, &cluster.centroid);
                    if sim > best_match.map(|(_, s)| s).unwrap_or(-1.0) {
                        best_match = Some((idx, sim));
                    }
                }

                let target_pid = if let Some((idx, sim)) = best_match {
                    if sim >= 0.40 {
                        let cluster = &mut existing_clusters[idx];
                        let pid = cluster.person_id.clone();

                        // Only update the centroid once per video per person
                        if !best_faces_per_person.contains_key(&pid) {
                            let n = cluster.face_count as f32;
                            let mut new_centroid = vec![0.0f32; 512];
                            for k in 0..512 {
                                new_centroid[k] = (cluster.centroid[k] * n) + embedding[k];
                            }
                            let norm = new_centroid.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
                            for v in new_centroid.iter_mut() {
                                *v /= norm;
                            }

                            cluster.centroid = new_centroid.clone();
                            cluster.face_count += 1;

                            updated_clusters.push(ClusterUpdate {
                                person_id: pid.clone(),
                                new_centroid,
                                new_face_count: cluster.face_count,
                                new_cover_face_id: None,
                            });
                        }

                        pid
                    } else {
                        // Under threshold -> New Person
                        let new_pid = Uuid::new_v4().to_string();
                        let temp_face_id = Uuid::new_v4().to_string();

                        existing_clusters.push(KnownPersonCluster {
                            person_id: new_pid.clone(),
                            centroid: embedding.clone(),
                            face_count: 1,
                            cover_face_id: Some(temp_face_id.clone()),
                        });

                        new_persons.push(NewPersonRecord {
                            person_id: new_pid.clone(),
                            cover_face_id: temp_face_id,
                            centroid: embedding.clone(),
                        });

                        new_pid
                    }
                } else {
                    let new_pid = Uuid::new_v4().to_string();
                    let temp_face_id = Uuid::new_v4().to_string();

                    existing_clusters.push(KnownPersonCluster {
                        person_id: new_pid.clone(),
                        centroid: embedding.clone(),
                        face_count: 1,
                        cover_face_id: Some(temp_face_id.clone()),
                    });

                    new_persons.push(NewPersonRecord {
                        person_id: new_pid.clone(),
                        cover_face_id: temp_face_id,
                        centroid: embedding.clone(),
                    });

                    new_pid
                };

                // Keep only the highest-quality face detection for each person in this video
                let is_better = match best_faces_per_person.get(&target_pid) {
                    Some(existing) => det.score > existing.score,
                    None => true,
                };

                if is_better {
                    let face_id = Uuid::new_v4().to_string();
                    let avatar = chip.resize_to_fill(128, 128, FilterType::Triangle);
                    let avatar_rel = format!("thumbs/faces/{}.webp", face_id);
                    let avatar_abs = faces_dir.join(format!("{}.webp", face_id));

                    if let Ok(mut out) = std::fs::File::create(&avatar_abs) {
                        let _ = avatar.write_to(&mut out, ImageFormat::WebP);
                    }

                    // Update cover_face_id if this person was created in this video pass
                    if let Some(np) = new_persons.iter_mut().find(|p| p.person_id == target_pid) {
                        np.cover_face_id = face_id.clone();
                    }

                    best_faces_per_person.insert(
                        target_pid.clone(),
                        NewFaceRecord {
                            face_id,
                            person_id: target_pid,
                            bbox_x: det.x,
                            bbox_y: det.y,
                            bbox_w: det.w,
                            bbox_h: det.h,
                            score: det.score,
                            face_thumb_rel_path: avatar_rel,
                            embedding,
                        },
                    );
                }
            }
        }

        Ok(ClusteredFacesResult {
            detected_faces: best_faces_per_person.into_values().collect(),
            updated_clusters,
            new_persons,
        })
    }

    // -------------------------------------------------------------------------
    // Image Processing Pipeline
    // -------------------------------------------------------------------------
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
        let aspect = if height > 0 { width as f64 / height as f64 } else { 1.0 };

        let (thumb_path, preview_path) =
            image_processor::ImageProcessor::generate_derivatives(&img, asset_id, shard_dir)?;

        let mime = match ext {
            "heic" | "heif" => "image/heic",
            "png" => "image/png",
            "webp" => "image/webp",
            _ => "image/jpeg",
        };

        if !run_ai {
            return Ok(ProcessedMediaResult {
                mime_type: mime.to_string(),
                width,
                height,
                aspect_ratio: aspect,
                duration_seconds: None,
                thumb_path,
                preview_path,
                meta,
                detected_faces: Vec::new(),
                updated_clusters: Vec::new(),
                new_persons: Vec::new(),
                tags: Vec::new(),
            });
        }

        // 1. Tag Inference
        let tags = self.tag_engine.tag_image(&img, 0.35)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, confidence)| TagPrediction { name, confidence })
            .collect();

        // 2. Face Detection & In-Memory Clustering
        let faces_res = self.process_faces(&img, thumbs_root, existing_clusters)?;

        Ok(ProcessedMediaResult {
            mime_type: mime.to_string(),
            width,
            height,
            aspect_ratio: aspect,
            duration_seconds: None,
            thumb_path,
            preview_path,
            meta,
            detected_faces: faces_res.detected_faces,
            updated_clusters: faces_res.updated_clusters,
            new_persons: faces_res.new_persons,
            tags,
        })
    }

    // -------------------------------------------------------------------------
    // Face Detection & Clustering Worker
    // -------------------------------------------------------------------------
    fn process_faces(
        &self,
        img: &DynamicImage,
        thumbs_root: &Path,
        mut existing_clusters: Vec<KnownPersonCluster>,
    ) -> Result<ClusteredFacesResult, Box<dyn std::error::Error + Send + Sync>> {
        let detections = match self.face_engine.detect_faces(img, 0.50, 0.35) {
            Ok(dets) if !dets.is_empty() => dets,
            _ => return Ok(ClusteredFacesResult::default()),
        };

        let faces_dir = thumbs_root.join("faces");
        std::fs::create_dir_all(&faces_dir)?;

        let mut claimed_persons: HashSet<String> = HashSet::new();
        let mut detected_faces = Vec::new();
        let mut updated_clusters = Vec::new();
        let mut new_persons = Vec::new();

        for det in detections {
            let face_id = Uuid::new_v4().to_string();
            let chip = crop_face_chip(img, det.x, det.y, det.w, det.h);

            // 1. Save Avatar Thumbnail
            let avatar = chip.resize_to_fill(128, 128, FilterType::Triangle);
            let avatar_rel = format!("thumbs/faces/{}.webp", face_id);
            let avatar_abs = faces_dir.join(format!("{}.webp", face_id));

            if let Ok(mut out) = std::fs::File::create(&avatar_abs) {
                let _ = avatar.write_to(&mut out, ImageFormat::WebP);
            }

            // 2. Compute ArcFace Embedding
            let embedding = match self.face_engine.extract_embedding(&chip) {
                Ok(emb) => emb,
                Err(_) => continue,
            };

            // 3. Find Best Centroid Match
            let mut best_match: Option<(usize, f32)> = None;
            for (idx, cluster) in existing_clusters.iter().enumerate() {
                if claimed_persons.contains(&cluster.person_id) {
                    continue;
                }
                let sim = cosine_similarity(&embedding, &cluster.centroid);
                if sim > best_match.map(|(_, s)| s).unwrap_or(-1.0) {
                    best_match = Some((idx, sim));
                }
            }

            // 4. Cluster Match vs. New Person Assignment
            let target_pid = if let Some((idx, sim)) = best_match {
                if sim >= 0.40 {
                    let cluster = &mut existing_clusters[idx];
                    let pid = cluster.person_id.clone();

                    // Online running average centroid update
                    let n = cluster.face_count as f32;
                    let mut new_centroid = vec![0.0f32; 512];
                    for k in 0..512 {
                        new_centroid[k] = (cluster.centroid[k] * n) + embedding[k];
                    }
                    let norm = new_centroid.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
                    for v in new_centroid.iter_mut() {
                        *v /= norm;
                    }

                    cluster.centroid = new_centroid.clone();
                    cluster.face_count += 1;

                    updated_clusters.push(ClusterUpdate {
                        person_id: pid.clone(),
                        new_centroid,
                        new_face_count: cluster.face_count,
                        new_cover_face_id: None,
                    });

                    pid
                } else {
                    let new_pid = Uuid::new_v4().to_string();
                    existing_clusters.push(KnownPersonCluster {
                        person_id: new_pid.clone(),
                        centroid: embedding.clone(),
                        face_count: 1,
                        cover_face_id: Some(face_id.clone()),
                    });
                    new_persons.push(NewPersonRecord {
                        person_id: new_pid.clone(),
                        cover_face_id: face_id.clone(),
                        centroid: embedding.clone(),
                    });
                    new_pid
                }
            } else {
                let new_pid = Uuid::new_v4().to_string();
                existing_clusters.push(KnownPersonCluster {
                    person_id: new_pid.clone(),
                    centroid: embedding.clone(),
                    face_count: 1,
                    cover_face_id: Some(face_id.clone()),
                });
                new_persons.push(NewPersonRecord {
                    person_id: new_pid.clone(),
                    cover_face_id: face_id.clone(),
                    centroid: embedding.clone(),
                });
                new_pid
            };

            claimed_persons.insert(target_pid.clone());

            detected_faces.push(NewFaceRecord {
                face_id,
                person_id: target_pid,
                bbox_x: det.x,
                bbox_y: det.y,
                bbox_w: det.w,
                bbox_h: det.h,
                score: det.score,
                face_thumb_rel_path: avatar_rel,
                embedding,
            });
        }

        Ok(ClusteredFacesResult {
            detected_faces,
            updated_clusters,
            new_persons,
        })
    }
}

// -----------------------------------------------------------------------------
// Pure Math & Geometry Helpers
// -----------------------------------------------------------------------------

#[inline]
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != 512 || b.len() != 512 {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn crop_face_chip(img: &DynamicImage, x: f32, y: f32, w: f32, h: f32) -> DynamicImage {
    let (img_w, img_h) = (img.width() as f32, img.height() as f32);
    let bw = w * img_w;
    let bh = h * img_h;

    let cx = (x * img_w) + bw * 0.5;
    let cy = (y * img_h) + bh * 0.43;

    let side = (bw.max(bh) * 1.35).round();
    let px = (cx - side * 0.5).round().max(0.0) as u32;
    let py = (cy - side * 0.5).round().max(0.0) as u32;
    let pw = (side as u32).min(img.width().saturating_sub(px)).max(1);
    let ph = (side as u32).min(img.height().saturating_sub(py)).max(1);

    img.crop_imm(px, py, pw, ph)
}