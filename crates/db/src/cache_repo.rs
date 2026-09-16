use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

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
    pub centroid: [f32; EMBEDDING_DIM],
}

pub struct CacheRepo;

impl CacheRepo {
    /// Loads all active CLIP vectors from the database for SIMD cache initialization
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

    pub async fn fetch_all_person_clusters(
        pool: &SqlitePool,
    ) -> Result<Vec<RawClusterRecord>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, user_id, face_count, cover_face_id, centroid_embedding \
             FROM persons \
             WHERE centroid_embedding IS NOT NULL",
        );

        let rows = qb.build().fetch_all(pool).await?;
        let expected_byte_len = EMBEDDING_DIM * std::mem::size_of::<f32>();

        let records = rows
            .into_iter()
            .filter_map(|r| {
                let bytes: Vec<u8> = r.try_get("centroid_embedding").ok()?;
                if bytes.len() != expected_byte_len {
                    return None;
                }

                let slice: &[f32] = bytemuck::cast_slice(&bytes);
                let mut centroid = [0.0f32; EMBEDDING_DIM];
                centroid.copy_from_slice(slice);

                let id: String = r.try_get("id").ok()?;
                let user_id: String = r.try_get("user_id").ok()?;
                let face_count: i32 = r.try_get("face_count").unwrap_or(1);
                let cover_face_id: Option<String> = r.try_get("cover_face_id").ok().flatten();

                Some(RawClusterRecord {
                    id,
                    user_id,
                    face_count: face_count as i64,
                    cover_face_id,
                    centroid,
                })
            })
            .collect();

        Ok(records)
    }
}