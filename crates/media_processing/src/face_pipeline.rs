use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use super::face_engine::FaceEngine;
use media_processing::{dot_product_512, normalize_l2, EMBEDDING_DIM};

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

struct ClusterCandidate {
    person_id: String,
    centroid: [f32; EMBEDDING_DIM],
    face_count: i32,
    cover_face_id: Option<String>,
}

impl FacePipeline {
    pub const MATCH_THRESHOLD: f32 = 0.40;

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
            
            let chip_view = media_processing::math::view_face_chip(img, det.x, det.y, det.w, det.h);
            let chip_image = DynamicImage::ImageRgba8(image::imageops::crop_imm(
                img,
                chip_view.offsets().0,
                chip_view.offsets().1,
                chip_view.width(),
                chip_view.height(),
            ).to_image());

            let avatar = image::imageops::resize(&chip_view, 128, 128, FilterType::Triangle);
            let avatar_rel_path = format!("users/{}/thumbs/faces/{}.webp", user_id, face_id);
            let avatar_abs_path = user_faces_dir.join(format!("{}.webp", face_id));

            let mut out = std::fs::File::create(&avatar_abs_path)?;
            DynamicImage::ImageRgba8(avatar).write_to(&mut out, ImageFormat::WebP)?;

            let raw_embedding = engine.extract_embedding(&chip_image)?;
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

        let rows = sqlx::query(
            "SELECT id, centroid_embedding, face_count, cover_face_id 
             FROM persons 
             WHERE user_id = ?1",
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        let expected_bytes = EMBEDDING_DIM * std::mem::size_of::<f32>();
        let mut known_clusters: Vec<ClusterCandidate> = Vec::new();

        for row in rows {
            let blob: Vec<u8> = row.try_get("centroid_embedding")?;
            if blob.len() == expected_bytes {
                let slice: &[f32] = bytemuck::cast_slice(&blob);
                let mut centroid = [0.0f32; EMBEDDING_DIM];
                centroid.copy_from_slice(slice);
                normalize_l2(&mut centroid);

                known_clusters.push(ClusterCandidate {
                    person_id: row.try_get("id")?,
                    centroid,
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
                let sim = dot_product_512(&face.embedding, &cluster.centroid);
                if sim > highest_sim {
                    highest_sim = sim;
                    best_match_idx = Some(idx);
                }
            }

            let target_person_id = if highest_sim >= Self::MATCH_THRESHOLD {
                let idx = best_match_idx.unwrap();
                let cluster = &mut known_clusters[idx];
                let pid = cluster.person_id.clone();

                let n = cluster.face_count as f32;
                let mut new_centroid = [0.0f32; EMBEDDING_DIM];
                for k in 0..EMBEDDING_DIM {
                    new_centroid[k] = (cluster.centroid[k] * n) + face.embedding[k];
                }
                normalize_l2(&mut new_centroid);

                cluster.centroid = new_centroid;
                cluster.face_count += 1;

                let mut should_update_cover = false;
                if let Some(ref current_cover_id) = cluster.cover_face_id {
                    let cover_row = sqlx::query("SELECT embedding FROM asset_faces WHERE id = ?1")
                        .bind(current_cover_id)
                        .fetch_optional(&mut *tx)
                        .await?;

                    if let Some(cr) = cover_row {
                        let cur_blob: Vec<u8> = cr.try_get("embedding")?;
                        if cur_blob.len() == expected_bytes {
                            let cur_slice: &[f32] = bytemuck::cast_slice(&cur_blob);
                            let mut cur_arr = [0.0f32; EMBEDDING_DIM];
                            cur_arr.copy_from_slice(cur_slice);
                            normalize_l2(&mut cur_arr);

                            let cur_sim = dot_product_512(&cur_arr, &cluster.centroid);
                            let new_sim = dot_product_512(&face.embedding, &cluster.centroid);

                            if new_sim > cur_sim {
                                should_update_cover = true;
                                cluster.cover_face_id = Some(face.face_id.clone());
                            }
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
                        "#,
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
                        "#,
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
                    "#,
                )
                .bind(&new_pid)
                .bind(user_id)
                .bind(&face.face_id)
                .bind(embedding_bytes)
                .execute(&mut *tx)
                .await?;

                known_clusters.push(ClusterCandidate {
                    person_id: new_pid.clone(),
                    centroid: face.embedding,
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
                "#,
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