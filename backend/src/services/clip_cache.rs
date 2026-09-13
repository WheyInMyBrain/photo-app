use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;
use crate::domain::media::SimilarMediaItem;

#[derive(Clone, Debug)]
pub struct CachedEmbedding {
    pub id: String,
    pub thumb_path: String,
    pub mime_type: String,
    pub is_private: bool,
    pub embedding: Vec<f32>,
}

#[derive(Clone)]
pub struct ClipCacheManager {
    items: Arc<RwLock<Vec<CachedEmbedding>>>,
}

impl ClipCacheManager {
    pub async fn load_initial(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let rows: Vec<(String, String, String, bool, Vec<u8>)> = sqlx::query_as(
            "SELECT id, thumb_path, mime_type, is_private, clip_embedding 
             FROM assets 
             WHERE deleted_at IS NULL 
               AND clip_embedding IS NOT NULL"
        )
        .fetch_all(pool)
        .await?;

        let items: Vec<CachedEmbedding> = rows
            .into_iter()
            .filter_map(|(id, thumb_path, mime_type, is_private, bytes)| {
                let slice: &[f32] = bytemuck::cast_slice(&bytes);
                if slice.len() == 512 {
                    Some(CachedEmbedding {
                        id,
                        thumb_path,
                        mime_type,
                        is_private,
                        embedding: slice.to_vec(),
                    })
                } else {
                    None
                }
            })
            .collect();

        tracing::info!("Loaded {} CLIP embeddings into RAM cache", items.len());

        Ok(Self {
            items: Arc::new(RwLock::new(items)),
        })
    }

    pub async fn insert(&self, item: CachedEmbedding) {
        let mut write_guard = self.items.write().await;
        if let Some(pos) = write_guard.iter().position(|e| e.id == item.id) {
            write_guard[pos] = item;
        } else {
            write_guard.push(item);
        }
    }

    pub async fn remove(&self, id: &str) {
        let mut write_guard = self.items.write().await;
        write_guard.retain(|e| e.id != id);
    }

    pub async fn find_similar(
        &self,
        target_id: &str,
        threshold: f32,
        limit: usize,
    ) -> Vec<SimilarMediaItem> {
        let guard = self.items.read().await;

        let target = match guard.iter().find(|e| e.id == target_id) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let target_emb = &target.embedding;
        let is_private = target.is_private;

        let mut scored: Vec<SimilarMediaItem> = guard
            .iter()
            .filter(|e| e.id != target_id && e.is_private == is_private)
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
        vector: &[f32],
        is_private: bool,
        threshold: f32,
        limit: usize,
    ) -> Vec<(String, f32)> {
        let guard = self.items.read().await;

        let mut scored: Vec<(String, f32)> = guard
            .iter()
            .filter(|item| item.is_private == is_private)
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

        // Sort descending by score
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        scored
    }
}