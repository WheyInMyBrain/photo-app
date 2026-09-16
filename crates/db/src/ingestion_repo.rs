use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use crate::domain::media::NewAssetRecord;

#[derive(Debug, Clone)]
pub struct IngestionNewPerson {
    pub person_id: String,
    pub cover_face_id: String,
    pub centroid: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct IngestionUpdatedCluster {
    pub person_id: String,
    pub new_centroid: Vec<f32>,
    pub new_face_count: i32,
}

#[derive(Debug, Clone)]
pub struct IngestionDetectedFace {
    pub face_id: String,
    pub person_id: String,
    pub bbox_x: f32,
    pub bbox_y: f32,
    pub bbox_w: f32,
    pub bbox_h: f32,
    pub score: f32,
    pub face_thumb_db_path: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct IngestionPayload {
    pub asset: NewAssetRecord,
    pub new_persons: Vec<IngestionNewPerson>,
    pub updated_clusters: Vec<IngestionUpdatedCluster>,
    pub detected_faces: Vec<IngestionDetectedFace>,
    pub tags: Vec<(String, f32)>, // (name, confidence)
}

/// Payload containing exclusively AI inferences produced in the background.
#[derive(Debug, Clone)]
pub struct AiEnrichmentPayload {
    pub asset_id: String,
    pub user_id: String,
    pub clip_embedding: Option<Vec<u8>>,
    pub new_persons: Vec<IngestionNewPerson>,
    pub updated_clusters: Vec<IngestionUpdatedCluster>,
    pub detected_faces: Vec<IngestionDetectedFace>,
    pub tags: Vec<(String, f32)>, // (name, confidence)
}

pub struct IngestionRepo;

impl IngestionRepo {
    /// Commits the entire post-processing payload atomically (used for monolithic runs).
    pub async fn commit_processed_asset(
        pool: &SqlitePool,
        payload: IngestionPayload,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;

        // 1. Insert core asset record
        crate::AssetRepo::insert_asset_tx(&mut *tx, &payload.asset).await?;

        // 2. Batch insert new persons (ON CONFLICT DO NOTHING in case of batch duplicates)
        let new_covers: Vec<(String, String)> = payload
            .new_persons
            .iter()
            .filter(|np| !np.cover_face_id.is_empty())
            .map(|np| (np.person_id.clone(), np.cover_face_id.clone()))
            .collect();

        if !payload.new_persons.is_empty() {
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO persons (id, user_id, name, cover_face_id, face_count, centroid_embedding) ",
            );

            qb.push_values(payload.new_persons, |mut b, np| {
                let embedding_bytes: &[u8] = bytemuck::cast_slice(&np.centroid);
                b.push_bind(np.person_id)
                    .push_bind(&payload.asset.user_id)
                    .push_bind(None::<String>)
                    .push_bind(None::<String>)
                    .push_bind(1i32)
                    .push_bind(embedding_bytes);
            });

            qb.push(" ON CONFLICT(id) DO NOTHING");
            qb.build().execute(&mut *tx).await?;
        }

        // 3. Update existing cluster centroids
        for uc in payload.updated_clusters {
            let embedding_bytes: &[u8] = bytemuck::cast_slice(&uc.new_centroid);
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "UPDATE persons SET centroid_embedding = ",
            );
            qb.push_bind(embedding_bytes);
            qb.push(", face_count = ");
            qb.push_bind(uc.new_face_count);
            qb.push(", updated_at = CURRENT_TIMESTAMP WHERE id = ");
            qb.push_bind(&uc.person_id);
            qb.push(" AND user_id = ");
            qb.push_bind(&payload.asset.user_id);

            qb.build().execute(&mut *tx).await?;
        }

        // 4. Batch insert detected faces (Convert empty person_id to NULL)
        if !payload.detected_faces.is_empty() {
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO asset_faces (
                    id, asset_id, person_id, bbox_x, bbox_y, bbox_w, bbox_h,
                    detection_score, face_thumb_path, embedding, is_verified
                ) ",
            );

            qb.push_values(payload.detected_faces, |mut b, face| {
                let embedding_bytes: &[u8] = bytemuck::cast_slice(&face.embedding);
                
                let person_id_opt = if face.person_id.trim().is_empty() {
                    None
                } else {
                    Some(face.person_id)
                };

                b.push_bind(face.face_id)
                    .push_bind(&payload.asset.id)
                    .push_bind(person_id_opt)
                    .push_bind(face.bbox_x)
                    .push_bind(face.bbox_y)
                    .push_bind(face.bbox_w)
                    .push_bind(face.bbox_h)
                    .push_bind(face.score)
                    .push_bind(face.face_thumb_db_path)
                    .push_bind(embedding_bytes)
                    .push_bind(0i32);
            });

            qb.build().execute(&mut *tx).await?;
        }

        // 4b. Backfill cover_face_id on persons now that asset_faces exist
        for (person_id, cover_face_id) in new_covers {
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "UPDATE persons SET cover_face_id = ",
            );
            qb.push_bind(cover_face_id);
            qb.push(" WHERE id = ");
            qb.push_bind(person_id);
            qb.push(" AND user_id = ");
            qb.push_bind(&payload.asset.user_id);

            qb.build().execute(&mut *tx).await?;
        }

        // 5. Batch insert & associate tags
        if !payload.tags.is_empty() {
            let mut tag_qb: QueryBuilder<Sqlite> =
                QueryBuilder::new("INSERT INTO tags (user_id, name, source) ");

            tag_qb.push_values(&payload.tags, |mut b, (name, _)| {
                b.push_bind(&payload.asset.user_id)
                    .push_bind(name)
                    .push_bind("model");
            });
            tag_qb.push(" ON CONFLICT(user_id, name) DO NOTHING");
            tag_qb.build().execute(&mut *tx).await?;

            let mut fetch_qb: QueryBuilder<Sqlite> =
                QueryBuilder::new("SELECT id, name FROM tags WHERE user_id = ");
            fetch_qb.push_bind(&payload.asset.user_id);
            fetch_qb.push(" AND name IN (");

            let mut separated = fetch_qb.separated(", ");
            for (name, _) in &payload.tags {
                separated.push_bind(name);
            }
            separated.push_unseparated(")");

            let tag_rows = fetch_qb.build().fetch_all(&mut *tx).await?;
            let tag_map: std::collections::HashMap<String, i64> = tag_rows
                .into_iter()
                .filter_map(|r| {
                    let id: i64 = r.try_get("id").ok()?;
                    let name: String = r.try_get("name").ok()?;
                    Some((name.to_lowercase(), id))
                })
                .collect();

            let valid_tags: Vec<(i64, f64)> = payload
                .tags
                .iter()
                .filter_map(|(name, conf)| {
                    tag_map
                        .get(&name.to_lowercase())
                        .map(|&id| (id, *conf as f64))
                })
                .collect();

            if !valid_tags.is_empty() {
                let mut link_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                    "INSERT INTO asset_tags (asset_id, tag_id, confidence, source) ",
                );
                link_qb.push_values(valid_tags, |mut b, (tag_id, conf)| {
                    b.push_bind(&payload.asset.id)
                        .push_bind(tag_id)
                        .push_bind(conf)
                        .push_bind("AI");
                });
                link_qb.push(" ON CONFLICT(asset_id, tag_id) DO UPDATE SET confidence = excluded.confidence");
                link_qb.build().execute(&mut *tx).await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }

    /// Commits purely the asynchronous AI inferences (faces, clusters, tags, and CLIP vector)
    /// to an existing asset row without touching base file metadata.
    pub async fn commit_ai_metadata(
        pool: &SqlitePool,
        payload: AiEnrichmentPayload,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;

        // 0. Safety Check: Verify the base asset exists
        let exists: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM assets WHERE id = ? AND user_id = ?"
        )
        .bind(&payload.asset_id)
        .bind(&payload.user_id)
        .fetch_optional(&mut *tx)
        .await?;

        if exists.is_none() {
            // If the base asset was deleted or never committed, abort cleanly
            tx.rollback().await?;
            return Err(sqlx::Error::RowNotFound);
        }

        // 1. Update asset with CLIP Embedding
        if let Some(emb) = payload.clip_embedding {
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("UPDATE assets SET clip_embedding = ");
            qb.push_bind(emb);
            qb.push(" WHERE id = ");
            qb.push_bind(&payload.asset_id);
            qb.push(" AND user_id = ");
            qb.push_bind(&payload.user_id);
            qb.build().execute(&mut *tx).await?;
        }

        // 2. Batch insert new persons (cover_face_id is NULL initially)
        let new_covers: Vec<(String, String)> = payload
            .new_persons
            .iter()
            .filter(|np| !np.cover_face_id.trim().is_empty())
            .map(|np| (np.person_id.clone(), np.cover_face_id.clone()))
            .collect();

        if !payload.new_persons.is_empty() {
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO persons (id, user_id, name, cover_face_id, face_count, centroid_embedding) ",
            );

            qb.push_values(payload.new_persons, |mut b, np| {
                let embedding_bytes: &[u8] = bytemuck::cast_slice(&np.centroid);
                b.push_bind(np.person_id)
                    .push_bind(&payload.user_id)
                    .push_bind(None::<String>)
                    .push_bind(None::<String>) // NULL so it won't trigger FK on asset_faces
                    .push_bind(1i32)
                    .push_bind(embedding_bytes);
            });

            qb.push(" ON CONFLICT(id) DO NOTHING");
            qb.build().execute(&mut *tx).await?;
        }

        // 3. Update existing cluster centroids
        for uc in payload.updated_clusters {
            let embedding_bytes: &[u8] = bytemuck::cast_slice(&uc.new_centroid);
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "UPDATE persons SET centroid_embedding = ",
            );
            qb.push_bind(embedding_bytes);
            qb.push(", face_count = ");
            qb.push_bind(uc.new_face_count);
            qb.push(", updated_at = CURRENT_TIMESTAMP WHERE id = ");
            qb.push_bind(&uc.person_id);
            qb.push(" AND user_id = ");
            qb.push_bind(&payload.user_id);

            qb.build().execute(&mut *tx).await?;
        }

        // 4. Batch insert detected faces
        // 4. Batch insert detected faces
        if !payload.detected_faces.is_empty() {
            // Collect all unique non-empty person_ids referenced by the detected faces
            let referenced_person_ids: Vec<String> = payload
                .detected_faces
                .iter()
                .filter_map(|f| {
                    let pid = f.person_id.trim();
                    if pid.is_empty() { None } else { Some(pid.to_string()) }
                })
                .collect();

            // Query SQLite to see which of these persons ACTUALLY exist right now
            let mut valid_person_set: std::collections::HashSet<String> = std::collections::HashSet::new();
            if !referenced_person_ids.is_empty() {
                let mut check_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                    "SELECT id FROM persons WHERE user_id = "
                );
                check_qb.push_bind(&payload.user_id);
                check_qb.push(" AND id IN (");
                let mut sep = check_qb.separated(", ");
                for pid in &referenced_person_ids {
                    sep.push_bind(pid);
                }
                sep.push_unseparated(")");

                let rows = check_qb.build().fetch_all(&mut *tx).await?;
                for r in rows {
                    if let Ok(id) = r.try_get::<String, _>("id") {
                        valid_person_set.insert(id);
                    }
                }
            }

            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO asset_faces (
                    id, asset_id, person_id, bbox_x, bbox_y, bbox_w, bbox_h,
                    detection_score, face_thumb_path, embedding, is_verified
                ) ",
            );

            qb.push_values(payload.detected_faces, |mut b, face| {
                let embedding_bytes: &[u8] = bytemuck::cast_slice(&face.embedding);
                
                // If the person_id does NOT exist in the database (e.g. was merged or deleted),
                // coerce to NULL so SQLite foreign key constraints are NEVER violated.
                let person_id_opt = if !face.person_id.trim().is_empty() && valid_person_set.contains(face.person_id.trim()) {
                    Some(face.person_id)
                } else {
                    None
                };

                b.push_bind(face.face_id)
                    .push_bind(&payload.asset_id)
                    .push_bind(person_id_opt)
                    .push_bind(face.bbox_x)
                    .push_bind(face.bbox_y)
                    .push_bind(face.bbox_w)
                    .push_bind(face.bbox_h)
                    .push_bind(face.score)
                    .push_bind(face.face_thumb_db_path)
                    .push_bind(embedding_bytes)
                    .push_bind(0i32);
            });

            qb.build().execute(&mut *tx).await?;
        }

        // 4b. Backfill cover_face_id on persons ONLY IF the referenced face actually exists in asset_faces
        for (person_id, cover_face_id) in new_covers {
            sqlx::query(
                r#"
                UPDATE persons 
                SET cover_face_id = ? 
                WHERE id = ? 
                  AND user_id = ? 
                  AND EXISTS (SELECT 1 FROM asset_faces WHERE id = ?)
                "#,
            )
            .bind(&cover_face_id)
            .bind(person_id)
            .bind(&payload.user_id)
            .bind(&cover_face_id)
            .execute(&mut *tx)
            .await?;
        }

        // 5. Batch insert & associate tags
        if !payload.tags.is_empty() {
            let mut tag_qb: QueryBuilder<Sqlite> =
                QueryBuilder::new("INSERT INTO tags (user_id, name, source) ");

            tag_qb.push_values(&payload.tags, |mut b, (name, _)| {
                b.push_bind(&payload.user_id)
                    .push_bind(name)
                    .push_bind("model");
            });
            tag_qb.push(" ON CONFLICT(user_id, name) DO NOTHING");
            tag_qb.build().execute(&mut *tx).await?;

            let mut fetch_qb: QueryBuilder<Sqlite> =
                QueryBuilder::new("SELECT id, name FROM tags WHERE user_id = ");
            fetch_qb.push_bind(&payload.user_id);
            fetch_qb.push(" AND name IN (");

            let mut separated = fetch_qb.separated(", ");
            for (name, _) in &payload.tags {
                separated.push_bind(name);
            }
            separated.push_unseparated(")");

            let tag_rows = fetch_qb.build().fetch_all(&mut *tx).await?;
            let tag_map: std::collections::HashMap<String, i64> = tag_rows
                .into_iter()
                .filter_map(|r| {
                    let id: i64 = r.try_get("id").ok()?;
                    let name: String = r.try_get("name").ok()?;
                    Some((name.to_lowercase(), id))
                })
                .collect();

            let valid_tags: Vec<(i64, f64)> = payload
                .tags
                .iter()
                .filter_map(|(name, conf)| {
                    tag_map
                        .get(&name.to_lowercase())
                        .map(|&id| (id, *conf as f64))
                })
                .collect();

            if !valid_tags.is_empty() {
                let mut link_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                    "INSERT INTO asset_tags (asset_id, tag_id, confidence, source) ",
                );
                link_qb.push_values(valid_tags, |mut b, (tag_id, conf)| {
                    b.push_bind(&payload.asset_id)
                        .push_bind(tag_id)
                        .push_bind(conf)
                        .push_bind("AI");
                });
                link_qb.push(" ON CONFLICT(asset_id, tag_id) DO UPDATE SET confidence = excluded.confidence");
                link_qb.build().execute(&mut *tx).await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }
}