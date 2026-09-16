use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;

use db::CacheRepo;
use media_processing::{normalize_l2, KnownPersonCluster};

pub type SharedClusterCache = Arc<RwLock<HashMap<String, Vec<KnownPersonCluster>>>>;

pub struct ClusterCacheManager;

impl ClusterCacheManager {
    pub async fn load_initial(pool: &SqlitePool) -> Result<SharedClusterCache, sqlx::Error> {
        let raw_clusters = CacheRepo::fetch_all_person_clusters(pool).await?;
        let mut user_clusters: HashMap<String, Vec<KnownPersonCluster>> = HashMap::new();

        for mut record in raw_clusters {
            // Pre-normalize centroid so clustering similarity is a pure SIMD dot product
            normalize_l2(&mut record.centroid);

            let cluster = KnownPersonCluster {
                person_id: record.id,
                centroid: record.centroid,
                face_count: record.face_count as i32,
                cover_face_id: record.cover_face_id,
            };

            user_clusters.entry(record.user_id).or_default().push(cluster);
        }

        Ok(Arc::new(RwLock::new(user_clusters)))
    }
}