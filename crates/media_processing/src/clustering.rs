use crate::face_engine::FaceEngine;
use crate::math::compute_face_chip_bounds;
use crate::models::{
    ClusterUpdate, ClusteredFacesResult, KnownPersonCluster, NewFaceRecord, NewPersonRecord,
};
use crate::simd::{dot_product_512, normalize_l2, EMBEDDING_DIM};
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use uuid::Uuid;

pub struct FaceClusterer;

impl FaceClusterer {
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

        let faces_dir = thumbs_root.join("faces");
        std::fs::create_dir_all(&faces_dir)?;

        let mut claimed_persons = HashSet::new();
        let mut detected_faces = Vec::new();
        let mut updated_clusters = Vec::new();
        let mut new_persons = Vec::new();

        for det in detections {
            let bounds = compute_face_chip_bounds(img.width(), img.height(), det.x, det.y, det.w, det.h);

            let chip_image = DynamicImage::ImageRgba8(
                image::imageops::crop_imm(img, bounds.px, bounds.py, bounds.pw, bounds.ph).to_image(),
            );

            let embedding_vec = match face_engine.extract_embedding(&chip_image) {
                Ok(emb) if emb.len() == EMBEDDING_DIM => emb,
                _ => continue,
            };

            let mut normalized_emb = [0.0f32; EMBEDDING_DIM];
            normalized_emb.copy_from_slice(&embedding_vec);
            normalize_l2(&mut normalized_emb);

            let (target_pid, is_new) = Self::find_or_create_match(
                &normalized_emb,
                &mut existing_clusters,
                &claimed_persons,
                &mut updated_clusters,
                &mut new_persons,
            );

            let face_id = Uuid::new_v4().to_string();
            let avatar_rel = Self::save_face_thumb(&chip_image, &faces_dir, &face_id)?;

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
                embedding: embedding_vec,
            });
        }

        Ok(ClusteredFacesResult {
            detected_faces,
            updated_clusters,
            new_persons,
        })
    }

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

        // Internal struct to hold best candidate before writing to disk
        struct BestCandidate {
            score: f32,
            chip_image: DynamicImage,
            embedding_vec: Vec<f32>,
            normalized_emb: [f32; EMBEDDING_DIM],
            bbox: (f32, f32, f32, f32),
        }

        // Map: person_id -> BestCandidate
        let mut best_candidates: HashMap<String, BestCandidate> = HashMap::new();
        let mut updated_clusters = Vec::new();
        let mut new_persons = Vec::new();

        for frame in frames {
            let detections = match face_engine.detect_faces(frame, 0.50, 0.35) {
                Ok(dets) if !dets.is_empty() => dets,
                _ => continue,
            };

            // Enforce that two faces in the SAME frame cannot claim the same identity
            let mut frame_claimed = HashSet::new();

            for det in detections {
                let bounds = compute_face_chip_bounds(frame.width(), frame.height(), det.x, det.y, det.w, det.h);

                let chip_image = DynamicImage::ImageRgba8(
                    image::imageops::crop_imm(frame, bounds.px, bounds.py, bounds.pw, bounds.ph).to_image(),
                );

                let embedding_vec = match face_engine.extract_embedding(&chip_image) {
                    Ok(emb) if emb.len() == EMBEDDING_DIM => emb,
                    _ => continue,
                };

                let mut normalized_emb = [0.0f32; EMBEDDING_DIM];
                normalized_emb.copy_from_slice(&embedding_vec);
                normalize_l2(&mut normalized_emb);

                // Check if this detection matches an already-identified person in this video
                let mut matched_existing_candidate = None;
                for (pid, cand) in &best_candidates {
                    if frame_claimed.contains(pid) {
                        continue;
                    }
                    if dot_product_512(&normalized_emb, &cand.normalized_emb) >= 0.40 {
                        matched_existing_candidate = Some(pid.clone());
                        break;
                    }
                }

                let target_pid = if let Some(pid) = matched_existing_candidate {
                    pid
                } else {
                    // Match against global clusters (or create a new person)
                    let (pid, _) = Self::find_or_create_match(
                        &normalized_emb,
                        &mut existing_clusters,
                        &frame_claimed,
                        &mut updated_clusters,
                        &mut new_persons,
                    );
                    pid
                };

                frame_claimed.insert(target_pid.clone());

                let is_better = match best_candidates.get(&target_pid) {
                    Some(prev) => det.score > prev.score,
                    None => true,
                };

                if is_better {
                    best_candidates.insert(
                        target_pid,
                        BestCandidate {
                            score: det.score,
                            chip_image,
                            embedding_vec,
                            normalized_emb,
                            bbox: (det.x, det.y, det.w, det.h),
                        },
                    );
                }
            }
        }

        // Write ONLY the winning face thumbnail per person to disk
        let mut detected_faces = Vec::with_capacity(best_candidates.len());
        for (person_id, cand) in best_candidates {
            let face_id = Uuid::new_v4().to_string();
            let avatar_rel = Self::save_face_thumb(&cand.chip_image, &faces_dir, &face_id)?;

            if let Some(np) = new_persons.iter_mut().find(|p| p.person_id == person_id) {
                np.cover_face_id = face_id.clone();
            }

            detected_faces.push(NewFaceRecord {
                face_id,
                person_id,
                bbox_x: cand.bbox.0,
                bbox_y: cand.bbox.1,
                bbox_w: cand.bbox.2,
                bbox_h: cand.bbox.3,
                score: cand.score,
                face_thumb_rel_path: avatar_rel,
                embedding: cand.embedding_vec,
            });
        }

        Ok(ClusteredFacesResult {
            detected_faces,
            updated_clusters,
            new_persons,
        })
    }

    fn find_or_create_match(
        normalized_embedding: &[f32; EMBEDDING_DIM],
        existing_clusters: &mut Vec<KnownPersonCluster>,
        claimed_persons: &HashSet<String>,
        updated_clusters: &mut Vec<ClusterUpdate>,
        new_persons: &mut Vec<NewPersonRecord>,
    ) -> (String, bool) {
        let mut best_person_idx: Option<usize> = None;
        let mut highest_sim: f32 = -1.0;

        for (idx, cluster) in existing_clusters.iter().enumerate() {
            if claimed_persons.contains(&cluster.person_id) {
                continue;
            }

            // Find the best matching exemplar for this person
            for exemplar in &cluster.exemplars {
                let sim = dot_product_512(normalized_embedding, exemplar);
                if sim > highest_sim {
                    highest_sim = sim;
                    best_person_idx = Some(idx);
                }
            }
        }

        // Match threshold (0.42 - 0.45 works best with InsightFace / ArcFace embeddings)
        if let Some(idx) = best_person_idx {
            if highest_sim >= 0.42 {
                let cluster = &mut existing_clusters[idx];
                let pid = cluster.person_id.clone();
                cluster.face_count += 1;

                // If this face captures a noticeably different look (e.g., sim between 0.42 and 0.65)
                // and we have fewer than 5 exemplars, store it as an additional representative angle
                if highest_sim < 0.65 && cluster.exemplars.len() < 5 {
                    cluster.exemplars.push(*normalized_embedding);
                }

                if let Some(existing_update) = updated_clusters.iter_mut().find(|u| u.person_id == pid) {
                    existing_update.new_face_count = cluster.face_count;
                } else if !new_persons.iter().any(|np| np.person_id == pid) {
                    updated_clusters.push(ClusterUpdate {
                        person_id: pid.clone(),
                        new_centroid: normalized_embedding.to_vec(),
                        new_face_count: cluster.face_count,
                        new_cover_face_id: None,
                    });
                }

                return (pid, false);
            }
        }

        // No match found -> create a new identity with this face as its first exemplar
        let new_pid = Uuid::new_v4().to_string();
        existing_clusters.push(KnownPersonCluster {
            person_id: new_pid.clone(),
            face_count: 1,
            cover_face_id: None,
            exemplars: vec![*normalized_embedding],
        });

        new_persons.push(NewPersonRecord {
            person_id: new_pid.clone(),
            cover_face_id: String::new(),
            centroid: normalized_embedding.to_vec(),
        });

        (new_pid, true)
    }

    fn save_face_thumb(
        chip_image: &DynamicImage,
        faces_dir: &Path,
        face_id: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let avatar_filename = format!("{}.webp", face_id);
        let avatar_abs = faces_dir.join(&avatar_filename);

        let avatar = chip_image.resize_to_fill(128, 128, FilterType::Triangle);

        let mut out = std::fs::File::create(&avatar_abs)?;
        avatar.write_to(&mut out, ImageFormat::WebP)?;

        Ok(format!("faces/{}", avatar_filename))
    }
}