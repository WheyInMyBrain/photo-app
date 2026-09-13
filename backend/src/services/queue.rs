use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Notify, Semaphore};
use tracing::{error, info};

use crate::db::{AssetRepo, JobRepo};
use crate::domain::job_repo::DbJob;
use crate::domain::media::NewAssetRecord;
use crate::services::clip_cache::{CachedEmbedding, ClipCacheManager};
use crate::services::cluster_cache::SharedClusterCache;
use crate::WsMediaEvent;

use media_processing::{MediaEngine, ProcessedMediaResult};

pub type ProcessJob = DbJob;

pub struct QueueService;

impl QueueService {
    pub async fn enqueue(
        pool: &SqlitePool,
        notify: &Arc<Notify>,
        job: ProcessJob,
    ) -> Result<(), sqlx::Error> {
        JobRepo::enqueue(pool, &job).await?;
        notify.notify_one();
        Ok(())
    }

    pub fn start_worker(
        pool: SqlitePool,
        thumbs_root: PathBuf,
        media_engine: Arc<MediaEngine>,
        cluster_cache: SharedClusterCache,
        clip_cache: ClipCacheManager, // <-- Injected cache
        concurrency: usize,
        notify: Arc<Notify>,
        tx_events: broadcast::Sender<WsMediaEvent>,
    ) {
        let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
        info!(concurrency = concurrency, "Unified media queue worker active");

        tokio::spawn(async move {
            let _ = JobRepo::reset_interrupted(&pool).await;

            loop {
                match JobRepo::fetch_next_job(&pool).await {
                    Ok(Some(job)) => {
                        let permit = match semaphore.clone().acquire_owned().await {
                            Ok(p) => p,
                            Err(_) => break,
                        };

                        let pool = pool.clone();
                        let thumbs_root = thumbs_root.clone();
                        let media_engine = media_engine.clone();
                        let cluster_cache = cluster_cache.clone();
                        let clip_cache = clip_cache.clone();
                        let tx_events = tx_events.clone();

                        tokio::spawn(async move {
                            let _guard = permit;
                            let job_id = job.id.clone();
                            let asset_id = job.asset_id.clone();

                            // 1. Read existing clusters from RAM
                            let known_clusters = {
                                let read_guard = cluster_cache.read().await;
                                read_guard.clone()
                            };

                            let disk_path = job.disk_path.clone();
                            let thumbs_dir = thumbs_root.clone();
                            let engine = media_engine.clone();

                            // 2. Offload processing to CPU/ONNX via spawn_blocking
                            let process_res = tokio::task::spawn_blocking(move || {
                                engine.process_asset_sync(
                                    &disk_path,
                                    &asset_id,
                                    &thumbs_dir,
                                    known_clusters,
                                    true,
                                )
                            })
                            .await;

                            let processed = match process_res {
                                Ok(Ok(data)) => data,
                                Ok(Err(e)) => {
                                    let err_msg = e.to_string();
                                    error!("Processing error on {}: {}", job.asset_id, err_msg);
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                                    let _ = tx_events.send(WsMediaEvent {
                                        event_type: "asset_failed".to_string(),
                                        asset_id: job.asset_id.clone(),
                                        thumb_path: String::new(),
                                    });
                                    return;
                                }
                                Err(join_err) => {
                                    let err_msg = join_err.to_string();
                                    error!("Worker panic on {}: {}", job.asset_id, err_msg);
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                                    let _ = tx_events.send(WsMediaEvent {
                                        event_type: "asset_failed".to_string(),
                                        asset_id: job.asset_id.clone(),
                                        thumb_path: String::new(),
                                    });
                                    return;
                                }
                            };

                            // 3. Atomically update in-memory face cache
                            if !processed.new_persons.is_empty() || !processed.updated_clusters.is_empty() {
                                let mut write_guard = cluster_cache.write().await;
                                for np in &processed.new_persons {
                                    write_guard.push(media_processing::KnownPersonCluster {
                                        person_id: np.person_id.clone(),
                                        centroid: np.centroid.clone(),
                                        face_count: 1,
                                        cover_face_id: Some(np.cover_face_id.clone()),
                                    });
                                }
                                for uc in &processed.updated_clusters {
                                    if let Some(c) = write_guard.iter_mut().find(|c| c.person_id == uc.person_id) {
                                        c.centroid = uc.new_centroid.clone();
                                        c.face_count = uc.new_face_count;
                                    }
                                }
                            }

                            // Keep clone of embedding & metadata for in-memory CLIP cache
                            let clip_embedding = processed.clip_embedding.clone();
                            let mime_type = processed.mime_type.clone();

                            // 4. Single database commit for asset, tags, and faces
                            match Self::commit_to_db(&pool, &job, processed).await {
                                Ok(thumb_path) => {
                                    let _ = JobRepo::mark_completed(&pool, &job_id).await;
                                    info!(id = %job.asset_id, "Asset processed & indexed successfully");

                                    // 5. Update in-memory CLIP cache
                                    if let Some(embedding) = clip_embedding {
                                        clip_cache
                                            .insert(CachedEmbedding {
                                                id: job.asset_id.clone(),
                                                thumb_path: thumb_path.clone(),
                                                mime_type,
                                                is_private: job.is_private,
                                                embedding,
                                            })
                                            .await;
                                    }

                                    // Emit real-time completion event to frontends
                                    let _ = tx_events.send(WsMediaEvent {
                                        event_type: "asset_ready".to_string(),
                                        asset_id: job.asset_id.clone(),
                                        thumb_path,
                                    });
                                }
                                Err(e) => {
                                    error!("DB commit failed on {}: {}", job.asset_id, e);
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &e).await;
                                    let _ = tx_events.send(WsMediaEvent {
                                        event_type: "asset_failed".to_string(),
                                        asset_id: job.asset_id.clone(),
                                        thumb_path: String::new(),
                                    });
                                }
                            }
                        });
                    }
                    Ok(None) => {
                        tokio::select! {
                            _ = notify.notified() => {},
                            _ = tokio::time::sleep(Duration::from_secs(30)) => {},
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to poll processing_jobs");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });
    }

    async fn commit_to_db(
        pool: &SqlitePool,
        job: &DbJob,
        res: ProcessedMediaResult,
    ) -> Result<String, String> {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        let thumb_path_out = res.thumb_path.clone();

        let clip_embedding_bytes: Option<Vec<u8>> = res
            .clip_embedding
            .as_ref()
            .map(|emb| bytemuck::cast_slice(emb.as_slice()).to_vec());

        let has_clip = clip_embedding_bytes.is_some();

        let record = NewAssetRecord {
            id: job.asset_id.clone(),
            sha256: job.sha256.clone(),
            file_name: job.file_name.clone(),
            rel_path: job.rel_path.clone(),
            folder_path: job.folder_path.clone(),
            thumb_path: res.thumb_path,
            preview_path: res.preview_path,
            file_size_bytes: job.file_size_bytes,
            mime_type: res.mime_type,
            width: res.width,
            height: res.height,
            aspect_ratio: res.aspect_ratio,
            duration_seconds: res.duration_seconds,
            is_private: job.is_private,
            captured_at: res.meta.captured_at,
            year: res.meta.year,
            month: res.meta.month,
            day: res.meta.day,
            hour: res.meta.hour,
            latitude: res.meta.latitude,
            longitude: res.meta.longitude,
            altitude: res.meta.altitude,
            city: res.meta.city,
            subdivision: res.meta.subdivision,
            country: res.meta.country,
            country_code: res.meta.country_code,
            camera_make: res.meta.camera_make,
            camera_model: res.meta.camera_model,
            clip_embedding: clip_embedding_bytes,
        };

        AssetRepo::insert_asset_tx(&mut *tx, &record)
            .await
            .map_err(|e| e.to_string())?;

        for np in res.new_persons {
            let embedding_bytes: &[u8] = bytemuck::cast_slice(&np.centroid);
            sqlx::query(
                "INSERT INTO persons (id, name, cover_face_id, face_count, centroid_embedding)
                VALUES (?1, NULL, ?2, 1, ?3)",
            )
            .bind(&np.person_id)
            .bind(&np.cover_face_id)
            .bind(embedding_bytes)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        for uc in res.updated_clusters {
            let embedding_bytes: &[u8] = bytemuck::cast_slice(&uc.new_centroid);
            sqlx::query(
                "UPDATE persons SET centroid_embedding = ?1, face_count = ?2, updated_at = CURRENT_TIMESTAMP
                WHERE id = ?3",
            )
            .bind(embedding_bytes)
            .bind(uc.new_face_count)
            .bind(&uc.person_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        for face in res.detected_faces {
            let embedding_bytes: &[u8] = bytemuck::cast_slice(&face.embedding);
            sqlx::query(
                "INSERT INTO asset_faces (
                    id, asset_id, person_id, bbox_x, bbox_y, bbox_w, bbox_h,
                    detection_score, face_thumb_path, embedding, is_verified
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)",
            )
            .bind(&face.face_id)
            .bind(&job.asset_id)
            .bind(&face.person_id)
            .bind(face.bbox_x)
            .bind(face.bbox_y)
            .bind(face.bbox_w)
            .bind(face.bbox_h)
            .bind(face.score)
            .bind(&face.face_thumb_rel_path)
            .bind(embedding_bytes)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        for tag in res.tags {
            let tag_id: i64 = match sqlx::query("SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE")
                .bind(&tag.name)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
            {
                Some(row) => row.try_get("id").map_err(|e| e.to_string())?,
                None => {
                    let r = sqlx::query("INSERT INTO tags (name, source) VALUES (?1, 'model')")
                        .bind(&tag.name)
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| e.to_string())?;
                    r.last_insert_rowid()
                }
            };

            sqlx::query(
                "INSERT INTO asset_tags (asset_id, tag_id, confidence, source)
                VALUES (?1, ?2, ?3, 'AI')
                ON CONFLICT(asset_id, tag_id) DO UPDATE SET confidence = excluded.confidence",
            )
            .bind(&job.asset_id)
            .bind(tag_id)
            .bind(tag.confidence as f64)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        sqlx::query(
            "UPDATE assets 
            SET face_processed = 1, 
                tags_processed = 1,
                clip_processed = ?1 
            WHERE id = ?2"
        )
        .bind(if has_clip { 1 } else { 0 })
        .bind(&job.asset_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        let _ = AssetRepo::sync_search_index(pool, &job.asset_id).await;

        Ok(thumb_path_out)
    }
}