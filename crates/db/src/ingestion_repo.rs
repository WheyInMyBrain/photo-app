// photo-app/crates/db/src/ingestion_repo.rs

use crate::domain::media::NewAssetRecord;
use crate::tag_repo::{IngestionTagInput, TagRepo};
use pgvector::Vector;
use sqlx::{PgPool, Postgres, QueryBuilder};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct IngestionNewPerson {
    pub person_id: Uuid,
    pub cover_face_id: Option<Uuid>,
    pub centroid: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct IngestionUpdatedCluster {
    pub person_id: Uuid,
    pub new_centroid: Vec<f32>,
    pub new_face_count: i32,
}

#[derive(Debug, Clone)]
pub struct IngestionDetectedFace {
    pub face_id: Uuid,
    pub person_id: Option<Uuid>,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    pub score: f32,
    pub face_thumb_db_path: String,
    pub embedding: Vec<f32>,
}

// ---------------------------------------------------------------------------
// YOLO DTOs for Database Ingestion
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct IngestionDetectedObject {
    pub id: Uuid,
    pub class_id: i32,
    pub label: String,
    pub score: f32,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
}

#[derive(Debug, Clone)]
pub struct IngestionDetectedPose {
    pub id: Uuid,
    pub score: f32,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    pub keypoints: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct IngestionPayload {
    pub asset: NewAssetRecord,
    pub new_persons: Vec<IngestionNewPerson>,
    pub updated_clusters: Vec<IngestionUpdatedCluster>,
    pub detected_faces: Vec<IngestionDetectedFace>,
    pub objects: Vec<IngestionDetectedObject>,
    pub poses: Vec<IngestionDetectedPose>,
    pub tags: Vec<IngestionTagInput>,
}

/// Payload containing exclusively AI inferences produced in the background.
#[derive(Debug, Clone, Default)]
pub struct AiEnrichmentPayload {
    pub asset_id: Uuid,
    pub user_id: Uuid,
    pub clip_embedding: Option<Vec<u8>>, // Raw float bytes or vec
    pub new_persons: Vec<IngestionNewPerson>,
    pub updated_clusters: Vec<IngestionUpdatedCluster>,
    pub detected_faces: Vec<IngestionDetectedFace>,
    pub poses: Vec<IngestionDetectedPose>,
    pub tags: Vec<IngestionTagInput>,
}

pub struct IngestionRepo;

impl IngestionRepo {
    /// Commits the entire post-processing payload atomically (used for monolithic runs).
    pub async fn commit_processed_asset(
        pool: &PgPool,
        payload: IngestionPayload,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;

        // 1. Insert core asset record
        crate::AssetRepo::insert_asset_tx(&mut tx, &payload.asset).await?;

        // 2. Batch insert new persons
        let new_covers: Vec<(Uuid, Uuid)> = payload
            .new_persons
            .iter()
            .filter_map(|np| np.cover_face_id.map(|cov| (np.person_id, cov)))
            .collect();

        if !payload.new_persons.is_empty() {
            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO persons (id, user_id, name, cover_face_id, face_count, centroid_embedding) ",
            );

            qb.push_values(payload.new_persons, |mut b, np| {
                let centroid_vec = Vector::from(np.centroid);
                b.push_bind(np.person_id)
                    .push_bind(payload.asset.user_id)
                    .push_bind(None::<String>)
                    .push_bind(None::<Uuid>)
                    .push_bind(1i32)
                    .push_bind(centroid_vec);
            });

            qb.push(" ON CONFLICT (id) DO NOTHING");
            qb.build().execute(&mut *tx).await?;
        }

        // 3. Update existing cluster centroids
        for uc in payload.updated_clusters {
            let centroid_vec = Vector::from(uc.new_centroid);
            sqlx::query(
                r#"
                UPDATE persons 
                SET centroid_embedding = $1, 
                    face_count = $2, 
                    updated_at = CURRENT_TIMESTAMP 
                WHERE id = $3 AND user_id = $4
                "#,
            )
            .bind(centroid_vec)
            .bind(uc.new_face_count)
            .bind(uc.person_id)
            .bind(payload.asset.user_id)
            .execute(&mut *tx)
            .await?;
        }

        // 4. Batch insert detected faces
        if !payload.detected_faces.is_empty() {
            let referenced_person_ids: Vec<Uuid> = payload
                .detected_faces
                .iter()
                .filter_map(|f| f.person_id)
                .collect();

            let mut valid_person_set: HashSet<Uuid> = HashSet::new();

            if !referenced_person_ids.is_empty() {
                let rows: Vec<Uuid> = sqlx::query_scalar(
                    r#"
                    SELECT id 
                    FROM persons 
                    WHERE user_id = $1 AND id = ANY($2::uuid[])
                    "#,
                )
                .bind(payload.asset.user_id)
                .bind(&referenced_person_ids)
                .fetch_all(&mut *tx)
                .await?;

                valid_person_set.extend(rows);
            }

            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO asset_faces (
                    id, user_id, asset_id, person_id, bbox_x, bbox_y, bbox_w, bbox_h,
                    detection_score, face_thumb_path, embedding, is_verified
                ) ",
            );

            qb.push_values(payload.detected_faces, |mut b, face| {
                let embedding_vec = Vector::from(face.embedding);

                let person_id_opt = face
                    .person_id
                    .filter(|pid| valid_person_set.contains(pid));

                b.push_bind(face.face_id)
                    .push_bind(payload.asset.user_id)
                    .push_bind(payload.asset.id)
                    .push_bind(person_id_opt)
                    .push_bind(face.bbox_x)
                    .push_bind(face.bbox_y)
                    .push_bind(face.bbox_w)
                    .push_bind(face.bbox_h)
                    .push_bind(face.score)
                    .push_bind(face.face_thumb_db_path)
                    .push_bind(embedding_vec)
                    .push_bind(false);
            });

            qb.build().execute(&mut *tx).await?;
        }

        // 4b. Backfill cover_face_id on persons
        for (person_id, cover_face_id) in new_covers {
            sqlx::query(
                r#"
                UPDATE persons 
                SET cover_face_id = $1 
                WHERE id = $2 
                  AND user_id = $3 
                  AND EXISTS (SELECT 1 FROM asset_faces WHERE id = $1)
                "#,
            )
            .bind(cover_face_id)
            .bind(person_id)
            .bind(payload.asset.user_id)
            .execute(&mut *tx)
            .await?;
        }

        // 5. Batch insert detected objects (YOLO seg boxes)
        if !payload.objects.is_empty() {
            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO asset_objects (id, user_id, asset_id, class_id, label, score, bbox_x, bbox_y, bbox_w, bbox_h) ",
            );

            qb.push_values(payload.objects, |mut b, obj| {
                b.push_bind(obj.id)
                    .push_bind(payload.asset.user_id)
                    .push_bind(payload.asset.id)
                    .push_bind(obj.class_id)
                    .push_bind(obj.label)
                    .push_bind(obj.score)
                    .push_bind(obj.bbox_x)
                    .push_bind(obj.bbox_y)
                    .push_bind(obj.bbox_w)
                    .push_bind(obj.bbox_h);
            });

            qb.build().execute(&mut *tx).await?;
        }

        // 6. Batch insert detected poses (YOLO pose keypoints)
        if !payload.poses.is_empty() {
            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO asset_poses (id, user_id, asset_id, score, bbox_x, bbox_y, bbox_w, bbox_h, keypoints) ",
            );

            qb.push_values(payload.poses, |mut b, pose| {
                b.push_bind(pose.id)
                    .push_bind(payload.asset.user_id)
                    .push_bind(payload.asset.id)
                    .push_bind(pose.score)
                    .push_bind(pose.bbox_x)
                    .push_bind(pose.bbox_y)
                    .push_bind(pose.bbox_w)
                    .push_bind(pose.bbox_h)
                    .push_bind(pose.keypoints);
            });

            qb.build().execute(&mut *tx).await?;
        }

        // 7. Save Asset Tags using TagRepo
        if !payload.tags.is_empty() {
            TagRepo::save_asset_tags_tx(
                &mut tx,
                payload.asset.user_id,
                payload.asset.id,
                &payload.tags,
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Commits purely the asynchronous AI inferences to an existing asset row.
    pub async fn commit_ai_metadata(
        pool: &PgPool,
        payload: AiEnrichmentPayload,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;

        // 0. Safety Check: Verify the base asset exists
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM assets WHERE id = $1 AND user_id = $2)",
        )
        .bind(payload.asset_id)
        .bind(payload.user_id)
        .fetch_one(&mut *tx)
        .await?;

        if !exists {
            tx.rollback().await?;
            return Err(sqlx::Error::RowNotFound);
        }

        // 1. Update asset with CLIP Embedding and flag
        if let Some(ref bytes) = payload.clip_embedding {
            let float_slice: &[f32] = bytemuck::cast_slice(bytes);
            let clip_vector = Vector::from(float_slice.to_vec());

            sqlx::query(
                r#"
                UPDATE assets 
                SET clip_embedding = $1, clip_processed = TRUE 
                WHERE id = $2 AND user_id = $3
                "#,
            )
            .bind(clip_vector)
            .bind(payload.asset_id)
            .bind(payload.user_id)
            .execute(&mut *tx)
            .await?;
        }

        // 2. Batch insert new persons
        let new_covers: Vec<(Uuid, Uuid)> = payload
            .new_persons
            .iter()
            .filter_map(|np| np.cover_face_id.map(|cov| (np.person_id, cov)))
            .collect();

        if !payload.new_persons.is_empty() {
            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO persons (id, user_id, name, cover_face_id, face_count, centroid_embedding) ",
            );

            qb.push_values(payload.new_persons, |mut b, np| {
                let centroid_vec = Vector::from(np.centroid);
                b.push_bind(np.person_id)
                    .push_bind(payload.user_id)
                    .push_bind(None::<String>)
                    .push_bind(None::<Uuid>)
                    .push_bind(1i32)
                    .push_bind(centroid_vec);
            });

            qb.push(" ON CONFLICT (id) DO NOTHING");
            qb.build().execute(&mut *tx).await?;
        }

        // 3. Update existing cluster centroids
        for uc in payload.updated_clusters {
            let centroid_vec = Vector::from(uc.new_centroid);
            sqlx::query(
                r#"
                UPDATE persons 
                SET centroid_embedding = $1, 
                    face_count = $2, 
                    updated_at = CURRENT_TIMESTAMP 
                WHERE id = $3 AND user_id = $4
                "#,
            )
            .bind(centroid_vec)
            .bind(uc.new_face_count)
            .bind(uc.person_id)
            .bind(payload.user_id)
            .execute(&mut *tx)
            .await?;
        }

        // 4. Batch insert detected faces
        if !payload.detected_faces.is_empty() {
            let referenced_person_ids: Vec<Uuid> = payload
                .detected_faces
                .iter()
                .filter_map(|f| f.person_id)
                .collect();

            let mut valid_person_set: HashSet<Uuid> = HashSet::new();

            if !referenced_person_ids.is_empty() {
                let rows: Vec<Uuid> = sqlx::query_scalar(
                    r#"
                    SELECT id 
                    FROM persons 
                    WHERE user_id = $1 AND id = ANY($2::uuid[])
                    "#,
                )
                .bind(payload.user_id)
                .bind(&referenced_person_ids)
                .fetch_all(&mut *tx)
                .await?;

                valid_person_set.extend(rows);
            }

            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO asset_faces (
                    id, user_id, asset_id, person_id, bbox_x, bbox_y, bbox_w, bbox_h,
                    detection_score, face_thumb_path, embedding, is_verified
                ) ",
            );

            qb.push_values(payload.detected_faces, |mut b, face| {
                let embedding_vec = Vector::from(face.embedding);

                let person_id_opt = face
                    .person_id
                    .filter(|pid| valid_person_set.contains(pid));

                b.push_bind(face.face_id)
                    .push_bind(payload.user_id)
                    .push_bind(payload.asset_id)
                    .push_bind(person_id_opt)
                    .push_bind(face.bbox_x)
                    .push_bind(face.bbox_y)
                    .push_bind(face.bbox_w)
                    .push_bind(face.bbox_h)
                    .push_bind(face.score)
                    .push_bind(face.face_thumb_db_path)
                    .push_bind(embedding_vec)
                    .push_bind(false);
            });

            qb.build().execute(&mut *tx).await?;

            sqlx::query("UPDATE assets SET face_processed = TRUE WHERE id = $1 AND user_id = $2")
                .bind(payload.asset_id)
                .bind(payload.user_id)
                .execute(&mut *tx)
                .await?;
        }

        // 4b. Backfill cover_face_id on persons
        for (person_id, cover_face_id) in new_covers {
            sqlx::query(
                r#"
                UPDATE persons 
                SET cover_face_id = $1 
                WHERE id = $2 
                  AND user_id = $3 
                  AND EXISTS (SELECT 1 FROM asset_faces WHERE id = $1)
                "#,
            )
            .bind(cover_face_id)
            .bind(person_id)
            .bind(payload.user_id)
            .execute(&mut *tx)
            .await?;
        }

        // 5. Batch insert detected poses (YOLO pose keypoints)
        if !payload.poses.is_empty() {
            // Delete prior poses on this asset to maintain idempotency
            sqlx::query("DELETE FROM asset_poses WHERE asset_id = $1")
                .bind(payload.asset_id)
                .execute(&mut *tx)
                .await?;

            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO asset_poses (id, user_id, asset_id, score, bbox_x, bbox_y, bbox_w, bbox_h, keypoints) ",
            );

            qb.push_values(payload.poses, |mut b, pose| {
                b.push_bind(pose.id)
                    .push_bind(payload.user_id)
                    .push_bind(payload.asset_id)
                    .push_bind(pose.score)
                    .push_bind(pose.bbox_x)
                    .push_bind(pose.bbox_y)
                    .push_bind(pose.bbox_w)
                    .push_bind(pose.bbox_h)
                    .push_bind(pose.keypoints);
            });

            qb.build().execute(&mut *tx).await?;
        }

        // 6. Delegate AI Tags to TagRepo
        if !payload.tags.is_empty() {
            TagRepo::save_asset_tags_tx(
                &mut tx,
                payload.user_id,
                payload.asset_id,
                &payload.tags,
            )
            .await?;

            sqlx::query("UPDATE assets SET tags_processed = TRUE WHERE id = $1 AND user_id = $2")
                .bind(payload.asset_id)
                .bind(payload.user_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}