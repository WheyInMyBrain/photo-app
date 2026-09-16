use sqlx::{Row, SqlitePool, Transaction};
use crate::domain::person::{AssetFaceDetail, PersonCard};

pub struct PersonRepo;

impl PersonRepo {
    /// Returns clustered identities visible strictly for the given user
    pub async fn get_overview(pool: &SqlitePool, user_id: &str) -> Result<Vec<PersonCard>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT 
                p.id,
                p.name,
                COUNT(af.id) AS face_count,
                (
                    SELECT af2.face_thumb_path 
                    FROM asset_faces af2
                    JOIN assets a2 ON af2.asset_id = a2.id
                    WHERE af2.person_id = p.id 
                      AND a2.user_id = ?1
                      AND a2.deleted_at IS NULL
                    ORDER BY af2.detection_score DESC 
                    LIMIT 1
                ) AS avatar_thumb
            FROM persons p
            JOIN asset_faces af ON af.person_id = p.id
            JOIN assets a ON af.asset_id = a.id
            WHERE p.user_id = ?1
              AND a.user_id = ?1
              AND a.deleted_at IS NULL 
              AND p.is_hidden = 0
            GROUP BY p.id
            HAVING face_count >= 1
            ORDER BY face_count DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let people = rows
            .into_iter()
            .filter_map(|r| {
                Some(PersonCard {
                    id: r.try_get("id").ok()?,
                    name: r.try_get("name").ok(),
                    face_count: r.try_get::<i64, _>("face_count").unwrap_or(0),
                    avatar_thumb: r.try_get("avatar_thumb").ok(),
                })
            })
            .collect();

        Ok(people)
    }

    pub async fn get_faces_by_asset(
        pool: &SqlitePool,
        user_id: &str,
        asset_id: &str,
    ) -> Result<Vec<AssetFaceDetail>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT 
                af.id as face_id,
                af.person_id,
                p.name as person_name,
                af.face_thumb_path,
                af.bbox_x,
                af.bbox_y,
                af.bbox_w,
                af.bbox_h,
                af.detection_score,
                af.is_verified
            FROM asset_faces af
            JOIN assets a ON af.asset_id = a.id
            LEFT JOIN persons p ON af.person_id = p.id
            WHERE af.asset_id = ?1
              AND a.user_id = ?2
              AND a.deleted_at IS NULL
            ORDER BY af.bbox_x ASC
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        let faces = rows
            .into_iter()
            .filter_map(|r| {
                Some(AssetFaceDetail {
                    face_id: r.try_get("face_id").ok()?,
                    person_id: r.try_get("person_id").ok(),
                    person_name: r.try_get("person_name").ok(),
                    face_thumb_path: r.try_get("face_thumb_path").ok()?,
                    bbox_x: r.try_get("bbox_x").unwrap_or(0.0),
                    bbox_y: r.try_get("bbox_y").unwrap_or(0.0),
                    bbox_w: r.try_get("bbox_w").unwrap_or(0.0),
                    bbox_h: r.try_get("bbox_h").unwrap_or(0.0),
                    score: r.try_get("detection_score").unwrap_or(0.0),
                    is_verified: r.try_get::<i64, _>("is_verified").unwrap_or(0) == 1,
                })
            })
            .collect();

        Ok(faces)
    }

    pub async fn rename_person(
        pool: &SqlitePool,
        user_id: &str,
        person_id: &str,
        name: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let res = sqlx::query(
            "UPDATE persons SET name = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2 AND user_id = ?3",
        )
        .bind(name)
        .bind(person_id)
        .bind(user_id)
        .execute(pool)
        .await?;

        if res.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }

        let affected_ids: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT af.asset_id 
            FROM asset_faces af
            JOIN assets a ON af.asset_id = a.id
            WHERE af.person_id = ?1 AND a.user_id = ?2
            "#,
        )
        .bind(person_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(affected_ids)
    }

    pub async fn verify_face(pool: &SqlitePool, user_id: &str, face_id: &str) -> Result<(), sqlx::Error> {
        let res = sqlx::query(
            r#"
            UPDATE asset_faces 
            SET is_verified = 1 
            WHERE id = ?1 
              AND asset_id IN (SELECT id FROM assets WHERE user_id = ?2)
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
        pool: &SqlitePool,
        user_id: &str,
        face_id: &str,
        target_person_id: &str,
    ) -> Result<String, sqlx::Error> {
        let mut tx = pool.begin().await?;

        // Ensure both the face and target person belong to the user
        let row = sqlx::query(
            r#"
            SELECT af.asset_id, af.person_id 
            FROM asset_faces af
            JOIN assets a ON af.asset_id = a.id
            WHERE af.id = ?1 AND a.user_id = ?2
            "#,
        )
        .bind(face_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

        let asset_id: String = row.get("asset_id");
        let old_person_id: Option<String> = row.get("person_id");

        let target_exists: bool = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM persons WHERE id = ?1 AND user_id = ?2",
        )
        .bind(target_person_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .is_some();

        if !target_exists {
            return Err(sqlx::Error::RowNotFound);
        }

        sqlx::query("UPDATE asset_faces SET person_id = ?1, is_verified = 1 WHERE id = ?2")
            .bind(target_person_id)
            .bind(face_id)
            .execute(&mut *tx)
            .await?;

        Self::recompute_cluster_centroid(&mut tx, user_id, target_person_id).await?;

        if let Some(old_pid) = old_person_id {
            if old_pid != target_person_id {
                Self::recompute_cluster_centroid(&mut tx, user_id, &old_pid).await?;
            }
        }

        tx.commit().await?;
        Ok(asset_id)
    }

    pub async fn merge_persons(
        pool: &SqlitePool,
        user_id: &str,
        source_person_id: &str,
        target_person_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let mut tx = pool.begin().await?;

        // Verify both persons belong to this user
        let valid_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM persons WHERE user_id = ?1 AND id IN (?2, ?3)",
        )
        .bind(user_id)
        .bind(source_person_id)
        .bind(target_person_id)
        .fetch_one(&mut *tx)
        .await?;

        if valid_count < 2 {
            return Err(sqlx::Error::RowNotFound);
        }

        let affected_asset_ids: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT DISTINCT af.asset_id 
            FROM asset_faces af
            JOIN assets a ON af.asset_id = a.id
            WHERE af.person_id IN (?1, ?2) AND a.user_id = ?3
            "#,
        )
        .bind(source_person_id)
        .bind(target_person_id)
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        // Reassign faces to the target identity
        sqlx::query("UPDATE asset_faces SET person_id = ?1 WHERE person_id = ?2")
            .bind(target_person_id)
            .bind(source_person_id)
            .execute(&mut *tx)
            .await?;

        // Recalculate target centroid
        Self::recompute_cluster_centroid(&mut tx, user_id, target_person_id).await?;

        // Delete the source person
        sqlx::query("DELETE FROM persons WHERE id = ?1 AND user_id = ?2")
            .bind(source_person_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(affected_asset_ids)
    }

    async fn recompute_cluster_centroid(
        tx: &mut Transaction<'_, sqlx::Sqlite>,
        user_id: &str,
        person_id: &str,
    ) -> Result<(), sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT af.id, af.embedding 
            FROM asset_faces af
            JOIN assets a ON af.asset_id = a.id
            WHERE af.person_id = ?1 AND a.user_id = ?2
            "#,
        )
        .bind(person_id)
        .bind(user_id)
        .fetch_all(&mut **tx)
        .await?;

        if rows.is_empty() {
            sqlx::query("DELETE FROM persons WHERE id = ?1 AND user_id = ?2")
                .bind(person_id)
                .bind(user_id)
                .execute(&mut **tx)
                .await?;
            return Ok(());
        }

        let count = rows.len();
        let mut mean_vector = vec![0.0f32; 512];
        let mut items: Vec<(String, Vec<f32>)> = Vec::with_capacity(count);

        for r in rows {
            let id: String = r.get("id");
            let blob: Vec<u8> = r.get("embedding");
            if blob.len() == 2048 {
                let slice: &[f32] = bytemuck::cast_slice(&blob);
                for k in 0..512 {
                    mean_vector[k] += slice[k];
                }
                items.push((id, slice.to_vec()));
            }
        }

        if items.is_empty() {
            return Ok(());
        }

        let norm: f32 = mean_vector.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
        for v in mean_vector.iter_mut() {
            *v /= norm;
        }

        let mut best_face_id = items[0].0.clone();
        let mut best_sim = -1.0f32;

        for (id, emb) in &items {
            let sim: f32 = emb.iter().zip(mean_vector.iter()).map(|(a, b)| a * b).sum();
            if sim > best_sim {
                best_sim = sim;
                best_face_id = id.clone();
            }
        }

        let centroid_bytes: &[u8] = bytemuck::cast_slice(&mean_vector);

        sqlx::query(
            r#"
            UPDATE persons 
            SET centroid_embedding = ?1, 
                face_count = ?2, 
                cover_face_id = ?3, 
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?4 AND user_id = ?5
            "#,
        )
        .bind(centroid_bytes)
        .bind(count as i64)
        .bind(best_face_id)
        .bind(person_id)
        .bind(user_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    /// Returns autocomplete name suggestions strictly for the authenticated user
    pub async fn get_name_directory(pool: &SqlitePool, user_id: &str) -> Result<Vec<PersonCard>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, name
            FROM persons
            WHERE user_id = ?1 
              AND name IS NOT NULL 
              AND TRIM(name) != '' 
              AND is_hidden = 0
            ORDER BY name ASC
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