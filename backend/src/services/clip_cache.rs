use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;
use crate::domain::media::SimilarMediaItem;

#[derive(Clone, Debug)]
pub struct CachedEmbedding {
    pub id: String,
    pub user_id: String,
    pub thumb_path: String,
    pub mime_type: String,
    pub embedding: Vec<f32>,
}

#[derive(Clone)]
pub struct ClipCacheManager {
    // user_id -> list of embeddings for that user
    items: Arc<RwLock<HashMap<String, Vec<CachedEmbedding>>>>,
}

impl ClipCacheManager {
    pub async fn load_initial(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let rows: Vec<(String, String, String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, user_id, thumb_path, mime_type, clip_embedding 
             FROM assets 
             WHERE deleted_at IS NULL 
               AND clip_embedding IS NOT NULL"
        )
        .fetch_all(pool)
        .await?;

        let mut user_map: HashMap<String, Vec<CachedEmbedding>> = HashMap::new();
        let mut total_count = 0;

        for (id, user_id, thumb_path, mime_type, bytes) in rows {
            let slice: &[f32] = bytemuck::cast_slice(&bytes);
            if slice.len() == 512 {
                user_map.entry(user_id.clone()).or_default().push(CachedEmbedding {
                    id,
                    user_id,
                    thumb_path,
                    mime_type,
                    embedding: slice.to_vec(),
                });
                total_count += 1;
            }
        }

        tracing::info!("Loaded {} CLIP embeddings into RAM cache across {} users", total_count, user_map.len());

        Ok(Self {
            items: Arc::new(RwLock::new(user_map)),
        })
    }

    pub async fn insert(&self, item: CachedEmbedding) {
        let mut write_guard = self.items.write().await;
        let user_items = write_guard.entry(item.user_id.clone()).or_default();
        if let Some(pos) = user_items.iter().position(|e| e.id == item.id) {
            user_items[pos] = item;
        } else {
            user_items.push(item);
        }
    }

    pub async fn remove(&self, user_id: &str, id: &str) {
        let mut write_guard = self.items.write().await;
        if let Some(user_items) = write_guard.get_mut(user_id) {
            user_items.retain(|e| e.id != id);
        }
    }

    pub async fn find_similar_for_user(
        &self,
        user_id: &str,
        target_id: &str,
        threshold: f32,
        limit: usize,
    ) -> Vec<SimilarMediaItem> {
        let guard = self.items.read().await;
        let user_items = match guard.get(user_id) {
            Some(items) => items,
            None => return Vec::new(),
        };

        let target = match user_items.iter().find(|e| e.id == target_id) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let target_emb = &target.embedding;

        let mut scored: Vec<SimilarMediaItem> = user_items
            .iter()
            .filter(|e| e.id != target_id)
            .filter_map(|e| {
                let sim: f32 = target_emb
                    .iter()
                    .zip(e.embedding.iter())
                    .map(|(a, b)| a * b)
                    .sum();

                if sim >= threshold {
                    Some(SimilarMediaItem {
                        id: e.id.clone(),
                        thumb_path: e.thumb_path.clone(),
                        mime_type: e.mime_type.clone(),
                        similarity: sim,
                    })
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);

        scored
    }

    pub async fn search_by_vector(
        &self,
        user_id: &str,
        vector: &[f32],
        threshold: f32,
        limit: usize,
    ) -> Vec<(String, f32)> {
        let guard = self.items.read().await;
        let user_items = match guard.get(user_id) {
            Some(items) => items,
            None => return Vec::new(),
        };

        let mut scored: Vec<(String, f32)> = user_items
            .iter()
            .filter_map(|item| {
                let sim: f32 = vector
                    .iter()
                    .zip(item.embedding.iter())
                    .map(|(a, b)| a * b)
                    .sum();

                if sim >= threshold {
                    Some((item.id.clone(), sim))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        scored
    }
}