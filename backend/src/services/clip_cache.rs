use sqlx::SqlitePool;
pub use media_processing::{CachedEmbedding, ClipCacheManager, EMBEDDING_DIM};

pub async fn load_clip_cache(pool: &SqlitePool) -> Result<ClipCacheManager, sqlx::Error> {
    let rows: Vec<(String, String, String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT id, user_id, thumb_path, mime_type, clip_embedding 
         FROM assets 
         WHERE deleted_at IS NULL 
           AND clip_embedding IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let records: Vec<CachedEmbedding> = rows
        .into_iter()
        .filter_map(|(id, user_id, thumb_path, mime_type, bytes)| {
            let slice: &[f32] = bytemuck::cast_slice(&bytes);
            if slice.len() == EMBEDDING_DIM {
                let mut emb = [0.0f32; EMBEDDING_DIM];
                emb.copy_from_slice(slice);
                Some(CachedEmbedding {
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

    let manager = ClipCacheManager::new();
    manager.init_from_records(records).await;

    Ok(manager)
}