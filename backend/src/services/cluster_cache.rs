use db::CacheRepo;
use media_processing::KnownPersonCluster;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type SharedClusterCache = Arc<RwLock<HashMap<String, Vec<KnownPersonCluster>>>>;

pub struct ClusterCacheManager;

impl ClusterCacheManager {
    pub async fn load_initial(pool: &sqlx::SqlitePool) -> Result<SharedClusterCache, sqlx::Error> {
        let raw_clusters = CacheRepo::fetch_all_person_clusters(pool).await?;
        let mut user_buckets: HashMap<String, Vec<KnownPersonCluster>> = HashMap::new();

        for rc in raw_clusters {
            user_buckets
                .entry(rc.user_id)
                .or_default()
                .push(KnownPersonCluster {
                    person_id: rc.id,
                    face_count: rc.face_count as i32,
                    cover_face_id: rc.cover_face_id,
                    exemplars: rc.exemplars,
                });
        }

        Ok(Arc::new(RwLock::new(user_buckets)))
    }
}