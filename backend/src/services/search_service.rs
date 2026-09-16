use std::sync::Arc;
use sqlx::SqlitePool;
use media_processing::ClipEngine;
use db::domain::media::MediaQuery;
use crate::services::clip_cache::ClipCacheManager;

pub struct ParsedSearchQuery {
    pub extracted_person_id: Option<String>,
    pub visual_prompt: Option<String>,
    pub candidate_asset_ids: Option<Vec<String>>,
}

pub struct SearchCoordinator;

impl SearchCoordinator {
    /// Parses incoming raw user query, resolves named entities, and retrieves vector candidates
    pub async fn resolve_hybrid_query(
        pool: &SqlitePool,
        clip_engine: &Arc<ClipEngine>,
        clip_cache: &ClipCacheManager,
        raw_query: &str,
        is_private: bool,
    ) -> ParsedSearchQuery {
        let trimmed = raw_query.trim();
        if trimmed.is_empty() {
            return ParsedSearchQuery {
                extracted_person_id: None,
                visual_prompt: None,
                candidate_asset_ids: None,
            };
        }

        let mut words: Vec<String> = trimmed.split_whitespace().map(|s| s.to_string()).collect();
        let mut extracted_person_id = None;

        // 1. Resolve identified people names (case-insensitive)
        let named_persons: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, name FROM persons WHERE name IS NOT NULL"
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (pid, name) in named_persons {
            let name_lower = name.to_lowercase();
            // Check if any word or consecutive pair matches person name
            if let Some(pos) = words.iter().position(|w| w.to_lowercase() == name_lower) {
                extracted_person_id = Some(pid);
                words.remove(pos); // Strip name out of visual description
                break;
            }
        }

        let remaining_text = words.join(" ");

        // 2. If visual description remains, generate CLIP text vector
        let candidate_asset_ids = if !remaining_text.is_empty() {
            if let Ok(text_vector) = clip_engine.extract_text_embedding(&remaining_text) {
                // Cross-modal text-to-image similarity threshold: 0.24+ is a solid match
                let matches = clip_cache.search_by_vector(&text_vector, is_private, 0.24, 200).await;
                if matches.is_empty() {
                    // Fallback to empty list so we don't dump random items
                    Some(Vec::new())
                } else {
                    Some(matches.into_iter().map(|(id, _)| id).collect())
                }
            } else {
                None
            }
        } else {
            None
        };

        ParsedSearchQuery {
            extracted_person_id,
            visual_prompt: if remaining_text.is_empty() { None } else { Some(remaining_text) },
            candidate_asset_ids,
        }
    }
}