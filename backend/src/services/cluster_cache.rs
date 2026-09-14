use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::{Row, SqlitePool};
use media_processing::KnownPersonCluster;

/// Cache mapping: user_id -> Vec<KnownPersonCluster>
pub type SharedClusterCache = Arc<RwLock<HashMap<String, Vec<KnownPersonCluster>>>>;

pub struct ClusterCacheManager;

impl ClusterCacheManager {
    /// Initial load of all centroids into RAM partitioned strictly by user_id
    pub async fn load_initial(pool: &SqlitePool) -> Result<SharedClusterCache, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, user_id, centroid_embedding, face_count, cover_face_id FROM persons"
        )
        .fetch_all(pool)
        .await?;

        let mut user_clusters: HashMap<String, Vec<KnownPersonCluster>> = HashMap::new();

        for row in rows {
            let blob: Vec<u8> = row.try_get("centroid_embedding")?;
            if blob.len() == 2048 {
                let centroid: &[f32] = bytemuck::cast_slice(&blob);
                let user_id: String = row.try_get("user_id")?;

                let cluster = KnownPersonCluster {
                    person_id: row.try_get("id")?,
                    centroid: centroid.to_vec(),
                    face_count: row.try_get("face_count")?,
                    cover_face_id: row.try_get("cover_face_id")?,
                };

                user_clusters.entry(user_id).or_default().push(cluster);
            }
        }

        Ok(Arc::new(RwLock::new(user_clusters)))
    }
}