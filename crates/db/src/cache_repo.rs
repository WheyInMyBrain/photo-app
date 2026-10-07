// photo-app/crates/db/src/cache_repo.rs

use pgvector::Vector;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub const EMBEDDING_DIM: usize = 512;

#[derive(Debug, Clone)]
pub struct RawClusterRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub face_count: i64,
    pub cover_face_id: Option<Uuid>,
    pub exemplars: Vec<[f32; EMBEDDING_DIM]>,
}

#[derive(Debug, Clone)]
pub struct ClipSearchResult {
    pub id: Uuid,
    pub thumb_path: String,
    pub mime_type: String,
    pub similarity: f32,
}

pub struct CacheRepo;

impl CacheRepo {
    /// In-database visual similarity search using the HNSW cosine index.
    /// Finds other assets belonging to the same user similar to the given asset.
    pub async fn search_similar_by_asset_id(
        pool: &PgPool,
        user_id: Uuid,
        asset_id: Uuid,
        threshold: f32,
        limit: i64,
    ) -> Result<Vec<ClipSearchResult>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            WITH target AS (
                SELECT clip_embedding
                FROM assets
                WHERE id = $1 AND user_id = $2 AND clip_embedding IS NOT NULL
            )
            SELECT 
                a.id,
                a.thumb_path,
                a.mime_type,
                (1.0 - (a.clip_embedding <=> t.clip_embedding))::real AS similarity
            FROM assets a, target t
            WHERE a.user_id = $2
              AND a.id != $1
              AND a.deleted_at IS NULL
              AND a.clip_embedding IS NOT NULL
              AND (1.0 - (a.clip_embedding <=> t.clip_embedding)) >= $3
            ORDER BY a.clip_embedding <=> t.clip_embedding ASC
            LIMIT $4;
            "#,
        )
        .bind(asset_id)
        .bind(user_id)
        .bind(threshold)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|r| ClipSearchResult {
                id: r.get("id"),
                thumb_path: r.get("thumb_path"),
                mime_type: r.get("mime_type"),
                similarity: r.get("similarity"),
            })
            .collect();

        Ok(results)
    }

    /// Semantic text search via pgvector: finds asset IDs matching an embedded prompt vector.
    pub async fn search_by_vector(
        pool: &PgPool,
        user_id: Uuid,
        query_vector: &[f32],
        threshold: f32,
        limit: i64,
    ) -> Result<Vec<Uuid>, sqlx::Error> {
        let vec_param = Vector::from(query_vector.to_vec());

        let rows = sqlx::query(
            r#"
            SELECT a.id
            FROM assets a
            WHERE a.user_id = $1
              AND a.deleted_at IS NULL
              AND a.clip_embedding IS NOT NULL
              AND (1.0 - (a.clip_embedding <=> $2)) >= $3
            ORDER BY a.clip_embedding <=> $2 ASC
            LIMIT $4;
            "#,
        )
        .bind(user_id)
        .bind(vec_param)
        .bind(threshold)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let ids = rows.into_iter().map(|r| r.get("id")).collect();
        Ok(ids)
    }

    /// Hydrates person clusters with their top 5 distinct face exemplars.
    pub async fn fetch_all_person_clusters(
        pool: &PgPool,
    ) -> Result<Vec<RawClusterRecord>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            WITH ranked_faces AS (
                SELECT 
                    af.person_id,
                    af.embedding,
                    ROW_NUMBER() OVER (
                        PARTITION BY af.person_id 
                        ORDER BY af.is_verified DESC, af.detection_score DESC
                    ) AS rn
                FROM asset_faces af
                JOIN assets a ON af.asset_id = a.id
                WHERE af.person_id IS NOT NULL 
                  AND a.deleted_at IS NULL
            ),
            grouped_exemplars AS (
                SELECT 
                    person_id,
                    array_agg(embedding) AS exemplars
                FROM ranked_faces
                WHERE rn <= 5
                GROUP BY person_id
            )
            SELECT 
                p.id,
                p.user_id,
                p.face_count::bigint AS face_count,
                p.cover_face_id,
                COALESCE(ge.exemplars, ARRAY[p.centroid_embedding]) AS final_exemplars
            FROM persons p
            LEFT JOIN grouped_exemplars ge ON ge.person_id = p.id
            WHERE p.centroid_embedding IS NOT NULL;
            "#,
        )
        .fetch_all(pool)
        .await?;

        let records = rows
            .into_iter()
            .filter_map(|r| {
                let id: Uuid = r.get("id");
                let user_id: Uuid = r.get("user_id");
                let face_count: i64 = r.get("face_count");
                let cover_face_id: Option<Uuid> = r.get("cover_face_id");
                let raw_exemplars: Vec<Vector> = r.get("final_exemplars");

                let mut exemplars = Vec::with_capacity(raw_exemplars.len());
                for v in raw_exemplars {
                    let slice = v.as_slice();
                    if slice.len() == EMBEDDING_DIM {
                        let mut arr = [0.0f32; EMBEDDING_DIM];
                        arr.copy_from_slice(slice);
                        exemplars.push(arr);
                    }
                }

                if exemplars.is_empty() {
                    return None;
                }

                Some(RawClusterRecord {
                    id,
                    user_id,
                    face_count,
                    cover_face_id,
                    exemplars,
                })
            })
            .collect();

        Ok(records)
    }
}