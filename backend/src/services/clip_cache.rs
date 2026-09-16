use db::CacheRepo;
use media_processing::{CachedEmbedding, ClipCacheManager};
use sqlx::SqlitePool;

pub async fn load_clip_cache(pool: &SqlitePool) -> Result<ClipCacheManager, sqlx::Error> {
    let raw_records = CacheRepo::fetch_all_clip_embeddings(pool).await?;

    let records: Vec<CachedEmbedding> = raw_records
        .into_iter()
        .map(|r| CachedEmbedding {
            id: r.id,
            user_id: r.user_id,
            thumb_path: r.thumb_path,
            mime_type: r.mime_type,
            embedding: r.embedding,
        })
        .collect();

    let manager = ClipCacheManager::new();
    manager.init_from_records(records).await;

    Ok(manager)
}