use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::collections::HashMap;

pub const EMBEDDING_DIM: usize = 512;

#[derive(Debug, Clone)]
pub struct RawClipRecord {
    pub id: String,
    pub user_id: String,
    pub thumb_path: String,
    pub mime_type: String,
    pub embedding: [f32; EMBEDDING_DIM],
}

#[derive(Debug, Clone)]
pub struct RawClusterRecord {
    pub id: String,
    pub user_id: String,
    pub face_count: i64,
    pub cover_face_id: Option<String>,
    pub exemplars: Vec<[f32; EMBEDDING_DIM]>,
}

pub struct CacheRepo;

impl CacheRepo {
    pub async fn fetch_all_clip_embeddings(
        pool: &SqlitePool,
    ) -> Result<Vec<RawClipRecord>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, user_id, thumb_path, mime_type, clip_embedding 
             FROM assets 
             WHERE deleted_at IS NULL 
               AND clip_embedding IS NOT NULL",
        );

        let rows = qb.build().fetch_all(pool).await?;

        let records = rows
            .into_iter()
            .filter_map(|r| {
                let id: String = r.get("id");
                let user_id: String = r.get("user_id");
                let thumb_path: String = r.get("thumb_path");
                let mime_type: String = r.get("mime_type");
                let bytes: Vec<u8> = r.get("clip_embedding");

                let slice: &[f32] = bytemuck::cast_slice(&bytes);
                if slice.len() == EMBEDDING_DIM {
                    let mut emb = [0.0f32; EMBEDDING_DIM];
                    emb.copy_from_slice(slice);
                    Some(RawClipRecord {
                        id,
                        user_id,
                        thumb_path,
                        mime_type,
                        embedding: emb,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(records)
    }

    /// Hydrates person clusters with their top 5 distinct face exemplars.
    pub async fn fetch_all_person_clusters(
        pool: &SqlitePool,
    ) -> Result<Vec<RawClusterRecord>, sqlx::Error> {
        let expected_byte_len = EMBEDDING_DIM * std::mem::size_of::<f32>();

        // 1. Fetch top 5 face vectors per person ordered by detection quality
        // Window function groups and caps exemplars per person in a single scan
        let exemplar_rows = sqlx::query(
            r#"
            SELECT person_id, embedding
            FROM (
                SELECT 
                    af.person_id,
                    af.embedding,
                    ROW_NUMBER() OVER (
                        PARTITION BY af.person_id 
                        ORDER BY af.is_verified DESC, af.detection_score DESC
                    ) as rn
                FROM asset_faces af
                JOIN assets a ON af.asset_id = a.id
                WHERE af.person_id IS NOT NULL 
                  AND a.deleted_at IS NULL
            )
            WHERE rn <= 5
            "#,
        )
        .fetch_all(pool)
        .await?;

        let mut exemplars_map: HashMap<String, Vec<[f32; EMBEDDING_DIM]>> = HashMap::new();
        for r in exemplar_rows {
            let pid: String = r.get("person_id");
            let bytes: Vec<u8> = r.get("embedding");

            if bytes.len() == expected_byte_len {
                let slice: &[f32] = bytemuck::cast_slice(&bytes);
                let mut emb = [0.0f32; EMBEDDING_DIM];
                emb.copy_from_slice(slice);
                exemplars_map.entry(pid).or_default().push(emb);
            }
        }

        // 2. Fetch all person entities and attach their exemplars (fallback to centroid if no face rows exist)
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, user_id, face_count, cover_face_id, centroid_embedding 
             FROM persons",
        );

        let rows = qb.build().fetch_all(pool).await?;

        let records = rows
            .into_iter()
            .filter_map(|r| {
                let id: String = r.try_get("id").ok()?;
                let user_id: String = r.try_get("user_id").ok()?;
                let face_count: i32 = r.try_get("face_count").unwrap_or(1);
                let cover_face_id: Option<String> = r.try_get("cover_face_id").ok().flatten();

                // Take the 5 exemplars from asset_faces, or fall back to the person's stored centroid
                let mut person_exemplars = exemplars_map.remove(&id).unwrap_or_default();

                if person_exemplars.is_empty() {
                    if let Ok(bytes) = r.try_get::<Vec<u8>, _>("centroid_embedding") {
                        if bytes.len() == expected_byte_len {
                            let slice: &[f32] = bytemuck::cast_slice(&bytes);
                            let mut centroid = [0.0f32; EMBEDDING_DIM];
                            centroid.copy_from_slice(slice);
                            person_exemplars.push(centroid);
                        }
                    }
                }

                if person_exemplars.is_empty() {
                    return None;
                }

                Some(RawClusterRecord {
                    id,
                    user_id,
                    face_count: face_count as i64,
                    cover_face_id,
                    exemplars: person_exemplars,
                })
            })
            .collect();

        Ok(records)
    }
}