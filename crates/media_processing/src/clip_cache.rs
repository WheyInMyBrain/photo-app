use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::simd::{dot_product_512, normalize_l2, EMBEDDING_DIM};

#[derive(Clone, Debug)]
pub struct CachedEmbedding {
    pub id: String,
    pub user_id: String,
    pub thumb_path: String,
    pub mime_type: String,
    pub embedding: [f32; EMBEDDING_DIM],
}

#[derive(Clone, Debug)]
pub struct VectorSearchResult {
    pub id: String,
    pub thumb_path: String,
    pub mime_type: String,
    pub similarity: f32,
}

#[derive(Clone, Default)]
pub struct ClipCacheManager {
    // user_id -> contiguous array of normalized embeddings
    items: Arc<RwLock<HashMap<String, Vec<CachedEmbedding>>>>,
}

impl ClipCacheManager {
    pub fn new() -> Self {
        Self {
            items: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Bulk initialize the cache from pre-loaded records (called by backend at boot)
    pub async fn init_from_records(&self, mut records: Vec<CachedEmbedding>) {
        let mut user_map: HashMap<String, Vec<CachedEmbedding>> = HashMap::new();

        for mut item in records.drain(..) {
            normalize_l2(&mut item.embedding);
            user_map.entry(item.user_id.clone()).or_default().push(item);
        }

        let mut write_guard = self.items.write().await;
        *write_guard = user_map;
    }

    pub async fn insert(&self, mut item: CachedEmbedding) {
        normalize_l2(&mut item.embedding);

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
    ) -> Vec<VectorSearchResult> {
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

        let mut scored: Vec<VectorSearchResult> = user_items
            .iter()
            .filter(|e| e.id != target_id)
            .filter_map(|e| {
                let sim = dot_product_512(target_emb, &e.embedding);
                if sim >= threshold {
                    Some(VectorSearchResult {
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

        scored.sort_unstable_by(|a, b| {
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
        if vector.len() != EMBEDDING_DIM {
            return Vec::new();
        }

        let mut query_emb = [0.0f32; EMBEDDING_DIM];
        query_emb.copy_from_slice(vector);
        normalize_l2(&mut query_emb);

        let guard = self.items.read().await;
        let user_items = match guard.get(user_id) {
            Some(items) => items,
            None => return Vec::new(),
        };

        let mut scored: Vec<(String, f32)> = user_items
            .iter()
            .filter_map(|item| {
                let sim = dot_product_512(&query_emb, &item.embedding);
                if sim >= threshold {
                    Some((item.id.clone(), sim))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        scored
    }
}