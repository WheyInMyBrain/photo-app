use crate::domain::person::{AssetFaceDetail, PersonCard};
use pgvector::Vector;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

pub struct PersonRepo;

impl PersonRepo {
    /// Returns clustered identities visible strictly for the given user.
    /// Uses LATERAL joins to resolve top-scoring face avatars without table scans.
    pub async fn get_overview(
        pool: &PgPool,
        user_id: Uuid,
        album_id: Option<Uuid>,
    ) -> Result<Vec<PersonCard>, sqlx::Error> {
        let rows = if let Some(aid) = album_id {
            sqlx::query(
                r#"
                SELECT 
                    p.id,
                    p.name,
                    COUNT(af.id)::bigint AS face_count,
                    avatar.face_thumb_path AS avatar_thumb
                FROM persons p
                JOIN asset_faces af ON af.person_id = p.id AND af.user_id = $1
                JOIN assets a ON af.asset_id = a.id AND a.user_id = $1 AND a.deleted_at IS NULL
                JOIN album_assets aa ON aa.asset_id = a.id AND aa.album_id = $2
                LEFT JOIN LATERAL (
                    SELECT af2.face_thumb_path
                    FROM asset_faces af2
                    JOIN assets a2 ON af2.asset_id = a2.id AND a2.user_id = $1 AND a2.deleted_at IS NULL
                    JOIN album_assets aa2 ON aa2.asset_id = a2.id AND aa2.album_id = $2
                    WHERE af2.person_id = p.id
                    ORDER BY af2.detection_score DESC
                    LIMIT 1
                ) avatar ON TRUE
                WHERE p.user_id = $1
                  AND p.is_hidden = FALSE
                GROUP BY p.id, p.name, avatar.face_thumb_path
                HAVING COUNT(af.id) >= 1
                ORDER BY face_count DESC;
                "#,
            )
            .bind(user_id)
            .bind(aid)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT 
                    p.id,
                    p.name,
                    COUNT(af.id)::bigint AS face_count,
                    avatar.face_thumb_path AS avatar_thumb
                FROM persons p
                JOIN asset_faces af ON af.person_id = p.id AND af.user_id = $1
                JOIN assets a ON af.asset_id = a.id AND a.user_id = $1 AND a.deleted_at IS NULL
                LEFT JOIN LATERAL (
                    SELECT af2.face_thumb_path
                    FROM asset_faces af2
                    JOIN assets a2 ON af2.asset_id = a2.id AND a2.user_id = $1 AND a2.deleted_at IS NULL
                    WHERE af2.person_id = p.id
                    ORDER BY af2.detection_score DESC
                    LIMIT 1
                ) avatar ON TRUE
                WHERE p.user_id = $1
                  AND p.is_hidden = FALSE
                GROUP BY p.id, p.name, avatar.face_thumb_path
                HAVING COUNT(af.id) >= 1
                ORDER BY face_count DESC;
                "#,
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?
        };

        let people = rows
            .into_iter()
            .map(|r| PersonCard {
                id: r.get("id"),
                name: r.get("name"),
                face_count: r.get("face_count"),
                avatar_thumb: r.get("avatar_thumb"),
            })
            .collect();

        Ok(people)
    }

    pub async fn get_faces_by_asset(
        pool: &PgPool,
        user_id: Uuid,
        asset_id: Uuid,
    ) -> Result<Vec<AssetFaceDetail>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT 
                af.id AS face_id,
                af.person_id,
                p.name AS person_name,
                af.face_thumb_path,
                af.bbox_x,
                af.bbox_y,
                af.bbox_w,
                af.bbox_h,
                af.detection_score,
                af.is_verified
            FROM asset_faces af
            JOIN assets a ON af.asset_id = a.id AND a.user_id = $2 AND a.deleted_at IS NULL
            LEFT JOIN persons p ON af.person_id = p.id
            WHERE af.asset_id = $1
            ORDER BY af.bbox_x ASC;
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let faces = rows
            .into_iter()
            .map(|r| AssetFaceDetail {
                face_id: r.get("face_id"),
                person_id: r.get("person_id"),
                person_name: r.get("person_name"),
                face_thumb_path: r.get("face_thumb_path"),
                bbox_x: r.get("bbox_x"),
                bbox_y: r.get("bbox_y"),
                bbox_w: r.get("bbox_w"),
                bbox_h: r.get("bbox_h"),
                score: r.get("detection_score"),
                is_verified: r.get("is_verified"),
            })
            .collect();

        Ok(faces)
    }

    pub async fn rename_person(
        pool: &PgPool,
        user_id: Uuid,
        person_id: Uuid,
        name: &str,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        let res = sqlx::query(
            r#"
            UPDATE persons 
            SET name = $1, updated_at = CURRENT_TIMESTAMP 
            WHERE id = $2 AND user_id = $3
            "#,
        )
        .bind(name.trim())
        .bind(person_id)
        .bind(user_id)
        .execute(pool)
        .await?;

        if res.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }

        let affected_ids: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT af.asset_id 
            FROM asset_faces af
            WHERE af.person_id = $1 AND af.user_id = $2
            "#,
        )
        .bind(person_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(affected_ids)
    }

    pub async fn delete_person(
        pool: &PgPool,
        user_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let person_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM persons WHERE id = $1 AND user_id = $2)",
        )
        .bind(person_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        if !person_exists {
            return Err(sqlx::Error::RowNotFound);
        }

        let affected_asset_ids: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT af.asset_id 
            FROM asset_faces af
            WHERE af.person_id = $1 AND af.user_id = $2
            "#,
        )
        .bind(person_id)
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        // Detach faces before deleting person container
        sqlx::query(
            "UPDATE asset_faces SET person_id = NULL WHERE person_id = $1 AND user_id = $2",
        )
        .bind(person_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM persons WHERE id = $1 AND user_id = $2")
            .bind(person_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(affected_asset_ids)
    }

    /// Deletes a single face detection entry entirely
    pub async fn delete_face(
        pool: &PgPool,
        user_id: Uuid,
        face_id: Uuid,
    ) -> Result<(Uuid, Option<Uuid>), sqlx::Error> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            r#"
            DELETE FROM asset_faces
            WHERE id = $1 AND user_id = $2
            RETURNING asset_id, person_id, face_thumb_path
            "#,
        )
        .bind(face_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

        let asset_id: Uuid = row.get("asset_id");
        let person_id: Option<Uuid> = row.get("person_id");
        let face_thumb_path: Option<String> = row.get("face_thumb_path");

        if let Some(pid) = person_id {
            Self::recompute_cluster_centroid(&mut tx, user_id, pid).await?;
        }

        tx.commit().await?;

        if let Some(thumb) = face_thumb_path {
            let path = std::path::Path::new(&thumb);
            if path.exists() {
                let _ = tokio::fs::remove_file(path).await;
            }
        }

        Ok((asset_id, person_id))
    }

    /// Unlinks a face from its person cluster, setting person_id to NULL
    pub async fn unlink_face(
        pool: &PgPool,
        user_id: Uuid,
        face_id: Uuid,
    ) -> Result<(Uuid, Option<Uuid>), sqlx::Error> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            r#"
            UPDATE asset_faces
            SET person_id = NULL
            WHERE id = $1 AND user_id = $2
            RETURNING asset_id, (SELECT person_id FROM asset_faces WHERE id = $1) AS old_person_id
            "#,
        )
        .bind(face_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

        let asset_id: Uuid = row.get("asset_id");
        let person_id: Option<Uuid> = row.get("old_person_id");

        if let Some(pid) = person_id {
            Self::recompute_cluster_centroid(&mut tx, user_id, pid).await?;
        }

        tx.commit().await?;
        Ok((asset_id, person_id))
    }

    /// Detaches a face from its current cluster and spawns it as a brand-new Person identity
    pub async fn split_face_to_new_person(
        pool: &PgPool,
        user_id: Uuid,
        face_id: Uuid,
    ) -> Result<(Uuid, Uuid), sqlx::Error> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            r#"
            SELECT af.asset_id, af.person_id, af.embedding
            FROM asset_faces af
            WHERE af.id = $1 AND af.user_id = $2
            "#,
        )
        .bind(face_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

        let asset_id: Uuid = row.get("asset_id");
        let old_person_id: Option<Uuid> = row.get("person_id");
        let embedding: Vector = row.get("embedding");

        let new_person_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO persons (user_id, name, face_count, cover_face_id, centroid_embedding)
            VALUES ($1, NULL, 1, $2, $3)
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(face_id)
        .bind(embedding)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE asset_faces 
            SET person_id = $1, is_verified = TRUE 
            WHERE id = $2 AND user_id = $3
            "#,
        )
        .bind(new_person_id)
        .bind(face_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        if let Some(old_pid) = old_person_id {
            Self::recompute_cluster_centroid(&mut tx, user_id, old_pid).await?;
        }

        tx.commit().await?;
        Ok((asset_id, new_person_id))
    }

    pub async fn verify_face(
        pool: &PgPool,
        user_id: Uuid,
        face_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        let res = sqlx::query(
            r#"
            UPDATE asset_faces 
            SET is_verified = TRUE 
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(face_id)
        .bind(user_id)
        .execute(pool)
        .await?;

        if res.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }

        Ok(())
    }

    pub async fn reassign_face(
        pool: &PgPool,
        user_id: Uuid,
        face_id: Uuid,
        target_person_id: Uuid,
    ) -> Result<Uuid, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query(
            r#"
            SELECT af.asset_id, af.person_id 
            FROM asset_faces af
            WHERE af.id = $1 AND af.user_id = $2
            "#,
        )
        .bind(face_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

        let asset_id: Uuid = row.get("asset_id");
        let old_person_id: Option<Uuid> = row.get("person_id");

        let target_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM persons WHERE id = $1 AND user_id = $2)",
        )
        .bind(target_person_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        if !target_exists {
            return Err(sqlx::Error::RowNotFound);
        }

        sqlx::query(
            r#"
            UPDATE asset_faces 
            SET person_id = $1, is_verified = TRUE 
            WHERE id = $2 AND user_id = $3
            "#,
        )
        .bind(target_person_id)
        .bind(face_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        Self::recompute_cluster_centroid(&mut tx, user_id, target_person_id).await?;

        if let Some(old_pid) = old_person_id {
            if old_pid != target_person_id {
                Self::recompute_cluster_centroid(&mut tx, user_id, old_pid).await?;
            }
        }

        tx.commit().await?;
        Ok(asset_id)
    }

    pub async fn merge_persons(
        pool: &PgPool,
        user_id: Uuid,
        source_person_id: Uuid,
        target_person_id: Uuid,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let valid_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)::bigint 
            FROM persons 
            WHERE user_id = $1 AND id IN ($2, $3)
            "#,
        )
        .bind(user_id)
        .bind(source_person_id)
        .bind(target_person_id)
        .fetch_one(&mut *tx)
        .await?;

        if valid_count < 2 {
            return Err(sqlx::Error::RowNotFound);
        }

        let affected_asset_ids: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT af.asset_id 
            FROM asset_faces af
            WHERE af.person_id IN ($1, $2) AND af.user_id = $3
            "#,
        )
        .bind(source_person_id)
        .bind(target_person_id)
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE asset_faces 
            SET person_id = $1 
            WHERE person_id = $2 AND user_id = $3
            "#,
        )
        .bind(target_person_id)
        .bind(source_person_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        Self::recompute_cluster_centroid(&mut tx, user_id, target_person_id).await?;

        sqlx::query("DELETE FROM persons WHERE id = $1 AND user_id = $2")
            .bind(source_person_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(affected_asset_ids)
    }

    /// Computes the mean centroid vector of all constituent face embeddings,
    /// normalizes it to unit length, and picks the most representative cover face.
    async fn recompute_cluster_centroid(
        conn: &mut PgConnection,
        user_id: Uuid,
        person_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT af.id, af.embedding 
            FROM asset_faces af
            WHERE af.person_id = $1 AND af.user_id = $2
            "#,
        )
        .bind(person_id)
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await?;

        if rows.is_empty() {
            sqlx::query("DELETE FROM persons WHERE id = $1 AND user_id = $2")
                .bind(person_id)
                .bind(user_id)
                .execute(&mut *conn)
                .await?;
            return Ok(());
        }

        let count = rows.len();
        let mut mean_vector = vec![0.0f32; 512];
        let mut items: Vec<(Uuid, Vec<f32>)> = Vec::with_capacity(count);

        for r in rows {
            let id: Uuid = r.get("id");
            let vec_type: Vector = r.get("embedding");
            let slice = vec_type.as_slice();

            if slice.len() == 512 {
                for k in 0..512 {
                    mean_vector[k] += slice[k];
                }
                items.push((id, slice.to_vec()));
            }
        }

        if items.is_empty() {
            return Ok(());
        }

        // L2 Unit Normalization
        let norm: f32 = mean_vector.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
        for v in mean_vector.iter_mut() {
            *v /= norm;
        }

        // Identify the exemplar face closest to the computed centroid
        let mut best_face_id = items[0].0;
        let mut best_sim = -1.0f32;

        for (id, emb) in &items {
            let sim: f32 = emb.iter().zip(mean_vector.iter()).map(|(a, b)| a * b).sum();
            if sim > best_sim {
                best_sim = sim;
                best_face_id = *id;
            }
        }

        let centroid_vector = Vector::from(mean_vector);

        sqlx::query(
            r#"
            UPDATE persons 
            SET centroid_embedding = $1, 
                face_count = $2, 
                cover_face_id = $3, 
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $4 AND user_id = $5
            "#,
        )
        .bind(centroid_vector)
        .bind(count as i32)
        .bind(best_face_id)
        .bind(person_id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;

        Ok(())
    }

    pub async fn get_name_directory(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<PersonCard>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, name
            FROM persons
            WHERE user_id = $1 
              AND name IS NOT NULL 
              AND TRIM(name) <> '' 
              AND is_hidden = FALSE
            ORDER BY name ASC;
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let names = rows
            .into_iter()
            .map(|r| PersonCard {
                id: r.get("id"),
                name: r.get("name"),
                face_count: 0,
                avatar_thumb: None,
            })
            .collect();

        Ok(names)
    }
}