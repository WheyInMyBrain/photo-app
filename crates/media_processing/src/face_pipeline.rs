use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use super::face_engine::FaceEngine;

pub struct FacePipeline;

pub struct ExtractedFace {
    pub face_id: String,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    pub detection_score: f32,
    pub avatar_rel_path: String,
    pub embedding: Vec<f32>,
}

struct ClusterCandidate {
    person_id: String,
    centroid: Vec<f32>,
    face_count: i32,
    cover_face_id: Option<String>,
}

impl FacePipeline {
    pub const MATCH_THRESHOLD: f32 = 0.40;

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

    /// 1. Synchronous CPU/ONNX & File I/O — executed inside `spawn_blocking`
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
            let chip = Self::crop_face_chip(img, det.x, det.y, det.w, det.h);

            // Thumbnail encoding
            let avatar = chip.resize_to_fill(128, 128, FilterType::Triangle);
            
            // Standard relative path for DB & static router
            let avatar_rel_path = format!("users/{}/thumbs/faces/{}.webp", user_id, face_id);
            let avatar_abs_path = user_faces_dir.join(format!("{}.webp", face_id));

            let mut out = std::fs::File::create(&avatar_abs_path)?;
            avatar.write_to(&mut out, ImageFormat::WebP)?;

            // ONNX Feature Extraction
            let embedding = engine.extract_embedding(&chip)?;

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

    /// 2. Fast Async Database Clustering & Persistence (Strictly Scoped to `user_id`)
    pub async fn persist_faces(
        pool: &SqlitePool,
        user_id: &str,
        asset_id: &str,
        extracted: Vec<ExtractedFace>,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let mut tx = pool.begin().await?;

        if extracted.is_empty() {
            sqlx::query("UPDATE assets SET face_processed = 1 WHERE id = ?1 AND user_id = ?2")
                .bind(asset_id)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(0);
        }

        // Fetch existing clusters ONLY for this user
        let rows = sqlx::query(
            "SELECT id, centroid_embedding, face_count, cover_face_id 
             FROM persons 
             WHERE user_id = ?1"
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        let mut known_clusters: Vec<ClusterCandidate> = Vec::new();
        for row in rows {
            let blob: Vec<u8> = row.try_get("centroid_embedding")?;
            if blob.len() == 2048 {
                let centroid: &[f32] = bytemuck::cast_slice(&blob);
                known_clusters.push(ClusterCandidate {
                    person_id: row.try_get("id")?,
                    centroid: centroid.to_vec(),
                    face_count: row.try_get("face_count")?,
                    cover_face_id: row.try_get("cover_face_id")?,
                });
            }
        }

        let mut claimed_in_asset: HashSet<String> = HashSet::new();
        let total_faces = extracted.len();

        for face in extracted {
            let embedding_bytes: &[u8] = bytemuck::cast_slice(&face.embedding);

            let mut best_match_idx: Option<usize> = None;
            let mut highest_sim = -1.0f32;

            for (idx, cluster) in known_clusters.iter().enumerate() {
                if claimed_in_asset.contains(&cluster.person_id) {
                    continue;
                }
                let sim = Self::cosine_similarity(&face.embedding, &cluster.centroid);
                if sim > highest_sim {
                    highest_sim = sim;
                    best_match_idx = Some(idx);
                }
            }

            let target_person_id = if highest_sim >= Self::MATCH_THRESHOLD {
                let idx = best_match_idx.unwrap();
                let cluster = &mut known_clusters[idx];
                let pid = cluster.person_id.clone();

                // Online Centroid Update
                let n = cluster.face_count as f32;
                let mut new_centroid = vec![0.0f32; 512];
                for k in 0..512 {
                    new_centroid[k] = (cluster.centroid[k] * n) + face.embedding[k];
                }
                let norm: f32 = new_centroid.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
                for v in new_centroid.iter_mut() {
                    *v /= norm;
                }

                cluster.centroid = new_centroid.clone();
                cluster.face_count += 1;

                let mut should_update_cover = false;
                if let Some(ref current_cover_id) = cluster.cover_face_id {
                    let cover_row = sqlx::query(
                        "SELECT embedding FROM asset_faces WHERE id = ?1"
                    )
                    .bind(current_cover_id)
                    .fetch_optional(&mut *tx)
                    .await?;

                    if let Some(cr) = cover_row {
                        let cur_blob: Vec<u8> = cr.try_get("embedding")?;
                        let cur_slice: &[f32] = bytemuck::cast_slice(&cur_blob);
                        let cur_sim = Self::cosine_similarity(cur_slice, &cluster.centroid);
                        let new_sim = Self::cosine_similarity(&face.embedding, &cluster.centroid);

                        if new_sim > cur_sim {
                            should_update_cover = true;
                            cluster.cover_face_id = Some(face.face_id.clone());
                        }
                    }
                } else {
                    should_update_cover = true;
                    cluster.cover_face_id = Some(face.face_id.clone());
                }

                let new_centroid_bytes: &[u8] = bytemuck::cast_slice(&cluster.centroid);

                if should_update_cover {
                    sqlx::query(
                        r#"
                        UPDATE persons 
                        SET centroid_embedding = ?1, face_count = ?2, cover_face_id = ?3, updated_at = CURRENT_TIMESTAMP
                        WHERE id = ?4 AND user_id = ?5
                        "#
                    )
                    .bind(new_centroid_bytes)
                    .bind(cluster.face_count)
                    .bind(&face.face_id)
                    .bind(&pid)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                } else {
                    sqlx::query(
                        r#"
                        UPDATE persons 
                        SET centroid_embedding = ?1, face_count = ?2, updated_at = CURRENT_TIMESTAMP
                        WHERE id = ?3 AND user_id = ?4
                        "#
                    )
                    .bind(new_centroid_bytes)
                    .bind(cluster.face_count)
                    .bind(&pid)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                }

                pid
            } else {
                let new_pid = Uuid::new_v4().to_string();

                sqlx::query(
                    r#"
                    INSERT INTO persons (id, user_id, name, cover_face_id, face_count, centroid_embedding)
                    VALUES (?1, ?2, NULL, ?3, 1, ?4)
                    "#
                )
                .bind(&new_pid)
                .bind(user_id)
                .bind(&face.face_id)
                .bind(embedding_bytes)
                .execute(&mut *tx)
                .await?;

                known_clusters.push(ClusterCandidate {
                    person_id: new_pid.clone(),
                    centroid: face.embedding.clone(),
                    face_count: 1,
                    cover_face_id: Some(face.face_id.clone()),
                });

                new_pid
            };

            claimed_in_asset.insert(target_person_id.clone());

            sqlx::query(
                r#"
                INSERT INTO asset_faces (
                    id, asset_id, person_id, bbox_x, bbox_y, bbox_w, bbox_h,
                    detection_score, face_thumb_path, embedding, is_verified
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)
                "#
            )
            .bind(&face.face_id)
            .bind(asset_id)
            .bind(&target_person_id)
            .bind(face.bbox_x)
            .bind(face.bbox_y)
            .bind(face.bbox_w)
            .bind(face.bbox_h)
            .bind(face.detection_score)
            .bind(&face.avatar_rel_path)
            .bind(embedding_bytes)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query("UPDATE assets SET face_processed = 1 WHERE id = ?1 AND user_id = ?2")
            .bind(asset_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(total_faces)
    }

    pub async fn process_asset_faces(
        engine: Arc<FaceEngine>,
        pool: &SqlitePool,
        storage_root: &Path,
        user_id: &str,
        asset_id: &str,
        img: Arc<DynamicImage>,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let faces_dir = storage_root
            .join("users")
            .join(user_id)
            .join("thumbs")
            .join("faces");

        let user_id_owned = user_id.to_string();

        let extracted = tokio::task::spawn_blocking(move || {
            Self::detect_and_extract_faces_sync(&engine, &user_id_owned, &faces_dir, &img)
        })
        .await??;

        let count = Self::persist_faces(pool, user_id, asset_id, extracted).await?;
        Ok(count)
    }
}