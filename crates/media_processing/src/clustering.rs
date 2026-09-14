use crate::face_engine::FaceEngine;
use crate::math::{cosine_similarity, crop_face_chip, update_centroid};
use crate::models::{ClusteredFacesResult, KnownPersonCluster, NewFaceRecord, NewPersonRecord, ClusterUpdate};
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use uuid::Uuid;

pub struct FaceClusterer;

impl FaceClusterer {
    /// Clusters faces from a single photo
    pub fn cluster_single_image(
        img: &DynamicImage,
        face_engine: &FaceEngine,
        thumbs_root: &Path,
        mut existing_clusters: Vec<KnownPersonCluster>,
    ) -> Result<ClusteredFacesResult, Box<dyn std::error::Error + Send + Sync>> {
        let detections = match face_engine.detect_faces(img, 0.50, 0.35) {
            Ok(dets) if !dets.is_empty() => dets,
            _ => return Ok(ClusteredFacesResult::default()),
        };

        // Absolute target: storage_root/users/<user_id>/thumbs/faces/
        let faces_dir = thumbs_root.join("faces");
        std::fs::create_dir_all(&faces_dir)?;

        let mut claimed_persons = HashSet::new();
        let mut detected_faces = Vec::new();
        let mut updated_clusters = Vec::new();
        let mut new_persons = Vec::new();

        for det in detections {
            let chip = crop_face_chip(img, det.x, det.y, det.w, det.h);
            let embedding = match face_engine.extract_embedding(&chip) {
                Ok(emb) => emb,
                Err(_) => continue,
            };

            let (target_pid, is_new) = Self::find_or_create_match(
                &embedding,
                &mut existing_clusters,
                &claimed_persons,
                &mut updated_clusters,
                &mut new_persons,
            );

            let face_id = Uuid::new_v4().to_string();
            let avatar_rel = Self::save_face_thumb(&chip, &faces_dir, &face_id)?;

            if is_new {
                if let Some(np) = new_persons.iter_mut().find(|p| p.person_id == target_pid) {
                    np.cover_face_id = face_id.clone();
                }
            }

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

    /// Clusters faces sampled across video frames with intra-video deduplication
    pub fn cluster_video_frames(
        frames: &[DynamicImage],
        face_engine: &FaceEngine,
        thumbs_root: &Path,
        mut existing_clusters: Vec<KnownPersonCluster>,
    ) -> Result<ClusteredFacesResult, Box<dyn std::error::Error + Send + Sync>> {
        if frames.is_empty() {
            return Ok(ClusteredFacesResult::default());
        }

        let faces_dir = thumbs_root.join("faces");
        std::fs::create_dir_all(&faces_dir)?;

        let mut best_faces: HashMap<String, NewFaceRecord> = HashMap::new();
        let mut updated_clusters = Vec::new();
        let mut new_persons = Vec::new();

        for frame in frames {
            let detections = match face_engine.detect_faces(frame, 0.50, 0.35) {
                Ok(dets) if !dets.is_empty() => dets,
                _ => continue,
            };

            for det in detections {
                let chip = crop_face_chip(frame, det.x, det.y, det.w, det.h);
                let embedding = match face_engine.extract_embedding(&chip) {
                    Ok(emb) => emb,
                    Err(_) => continue,
                };

                let empty_claimed = HashSet::new();
                let (target_pid, _) = Self::find_or_create_match(
                    &embedding,
                    &mut existing_clusters,
                    &empty_claimed,
                    &mut updated_clusters,
                    &mut new_persons,
                );

                let is_better = match best_faces.get(&target_pid) {
                    Some(prev) => det.score > prev.score,
                    None => true,
                };

                if is_better {
                    let face_id = Uuid::new_v4().to_string();
                    let avatar_rel = Self::save_face_thumb(&chip, &faces_dir, &face_id)?;

                    if let Some(np) = new_persons.iter_mut().find(|p| p.person_id == target_pid) {
                        np.cover_face_id = face_id.clone();
                    }

                    best_faces.insert(
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
            detected_faces: best_faces.into_values().collect(),
            updated_clusters,
            new_persons,
        })
    }

    fn find_or_create_match(
        embedding: &[f32],
        existing_clusters: &mut Vec<KnownPersonCluster>,
        claimed_persons: &HashSet<String>,
        updated_clusters: &mut Vec<ClusterUpdate>,
        new_persons: &mut Vec<NewPersonRecord>,
    ) -> (String, bool) {
        let mut best_match: Option<(usize, f32)> = None;
        for (idx, cluster) in existing_clusters.iter().enumerate() {
            if claimed_persons.contains(&cluster.person_id) {
                continue;
            }
            let sim = cosine_similarity(embedding, &cluster.centroid);
            if sim > best_match.map(|(_, s)| s).unwrap_or(-1.0) {
                best_match = Some((idx, sim));
            }
        }

        if let Some((idx, sim)) = best_match {
            if sim >= 0.40 {
                let cluster = &mut existing_clusters[idx];
                let pid = cluster.person_id.clone();

                let new_centroid = update_centroid(&cluster.centroid, cluster.face_count, embedding);
                cluster.centroid = new_centroid.clone();
                cluster.face_count += 1;

                updated_clusters.push(ClusterUpdate {
                    person_id: pid.clone(),
                    new_centroid,
                    new_face_count: cluster.face_count,
                    new_cover_face_id: None,
                });

                return (pid, false);
            }
        }

        // Under threshold -> New Person
        let new_pid = Uuid::new_v4().to_string();
        existing_clusters.push(KnownPersonCluster {
            person_id: new_pid.clone(),
            centroid: embedding.to_vec(),
            face_count: 1,
            cover_face_id: None,
        });

        new_persons.push(NewPersonRecord {
            person_id: new_pid.clone(),
            cover_face_id: String::new(),
            centroid: embedding.to_vec(),
        });

        (new_pid, true)
    }

    fn save_face_thumb(
        chip: &DynamicImage,
        faces_dir: &Path,
        face_id: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let avatar = chip.resize_to_fill(128, 128, FilterType::Triangle);
        let avatar_filename = format!("{}.webp", face_id);
        let avatar_abs = faces_dir.join(&avatar_filename);

        if let Ok(mut out) = std::fs::File::create(&avatar_abs) {
            let _ = avatar.write_to(&mut out, ImageFormat::WebP);
        }

        // Returns "faces/<face_id>.webp" (QueueService prefixes users/<user_id>/thumbs/)
        Ok(format!("faces/{}", avatar_filename))
    }
}