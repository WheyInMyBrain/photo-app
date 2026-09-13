use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::{Row, SqlitePool};
use media_processing::KnownPersonCluster;

pub type SharedClusterCache = Arc<RwLock<Vec<KnownPersonCluster>>>;

pub struct ClusterCacheManager;

impl ClusterCacheManager {
    /// Initial load of all centroids into RAM on server startup
    pub async fn load_initial(pool: &SqlitePool) -> Result<SharedClusterCache, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, centroid_embedding, face_count, cover_face_id FROM persons"
        )
        .fetch_all(pool)
        .await?;

        let mut clusters = Vec::with_capacity(rows.len());
        for row in rows {
            let blob: Vec<u8> = row.try_get("centroid_embedding")?;
            if blob.len() == 2048 {
                let centroid: &[f32] = bytemuck::cast_slice(&blob);
                clusters.push(KnownPersonCluster {
                    person_id: row.try_get("id")?,
                    centroid: centroid.to_vec(),
                    face_count: row.try_get("face_count")?,
                    cover_face_id: row.try_get("cover_face_id")?,
                });
            }
        }

        Ok(Arc::new(RwLock::new(clusters)))
    }
}