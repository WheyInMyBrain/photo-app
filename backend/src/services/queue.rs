use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Notify, Semaphore};
use tracing::{error, info, warn};

use crate::db::{AssetRepo, JobRepo};
use crate::domain::job_repo::DbJob;
use crate::domain::media::NewAssetRecord;
use crate::services::engine_coordinator::EngineCoordinator;
use crate::WsMediaEvent;

use media_processing::ProcessedMediaResult;

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
        storage_root: PathBuf,
        coordinator: EngineCoordinator,
        concurrency: usize,
        notify: Arc<Notify>,
        tx_events: broadcast::Sender<WsMediaEvent>,
    ) {
        let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
        info!(concurrency = concurrency, "Unified media queue worker active (Idle-Evicting)");

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
                        let storage_root = storage_root.clone();
                        let coordinator = coordinator.clone();
                        let tx_events = tx_events.clone();

                        tokio::spawn(async move {
                            let _guard = permit;
                            let job_id = job.id.clone();
                            let asset_id = job.asset_id.clone();
                            let user_id = job.user_id.clone();

                            // 1. Wake or acquire Tier 2 & Tier 3 resources on demand
                            let media_engine = match coordinator.ensure_pipeline_engine().await {
                                Ok(e) => e,
                                Err(err) => {
                                    let err_msg = err.to_string();
                                    error!("Failed to acquire MediaEngine for {}: {}", asset_id, err_msg);
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                                    return;
                                }
                            };

                            let cluster_cache = match coordinator.ensure_cluster_cache().await {
                                Ok(c) => c,
                                Err(err) => {
                                    let err_msg = err.to_string();
                                    error!("Failed to acquire ClusterCache for {}: {}", asset_id, err_msg);
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                                    return;
                                }
                            };

                            let clip_cache = match coordinator.ensure_clip_cache().await {
                                Ok(c) => c,
                                Err(err) => {
                                    let err_msg = err.to_string();
                                    error!("Failed to acquire ClipCache for {}: {}", asset_id, err_msg);
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                                    return;
                                }
                            };

                            // Refresh the idle lease so resources do not evict mid-batch
                            coordinator.keep_warm().await;

                            // 2. Read existing clusters from RAM strictly for this user
                            let known_clusters = {
                                let read_guard = cluster_cache.read().await;
                                read_guard.get(&user_id).cloned().unwrap_or_default()
                            };

                            // Output target on disk: storage_root/users/<user_id>/thumbs/
                            let user_thumbs_dir = storage_root
                                .join("users")
                                .join(&user_id)
                                .join("thumbs");

                            if let Err(e) = tokio::fs::create_dir_all(&user_thumbs_dir).await {
                                error!("Failed creating user thumbs directory for {}: {}", user_id, e);
                                let _ = JobRepo::mark_failed(&pool, &job_id, &e.to_string()).await;
                                return;
                            }

                            let disk_path = job.disk_path.clone();
                            let engine = media_engine.clone();

                            // 3. Offload heavy CPU/ONNX inference via spawn_blocking
                            let process_res = tokio::task::spawn_blocking(move || {
                                engine.process_asset_sync(
                                    &disk_path,
                                    &asset_id,
                                    &user_thumbs_dir,
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

                            // 4. Update in-memory face cluster cache strictly for this user
                            if !processed.new_persons.is_empty() || !processed.updated_clusters.is_empty() {
                                let mut write_guard = cluster_cache.write().await;
                                let user_bucket = write_guard.entry(user_id.clone()).or_default();

                                for np in &processed.new_persons {
                                    if np.centroid.len() == media_processing::EMBEDDING_DIM {
                                        let mut centroid_arr = [0.0f32; media_processing::EMBEDDING_DIM];
                                        centroid_arr.copy_from_slice(&np.centroid);

                                        user_bucket.push(media_processing::KnownPersonCluster {
                                            person_id: np.person_id.clone(),
                                            centroid: centroid_arr,
                                            face_count: 1,
                                            cover_face_id: Some(np.cover_face_id.clone()),
                                        });
                                    }
                                }

                                for uc in &processed.updated_clusters {
                                    if uc.new_centroid.len() == media_processing::EMBEDDING_DIM {
                                        if let Some(c) = user_bucket.iter_mut().find(|c| c.person_id == uc.person_id) {
                                            c.centroid.copy_from_slice(&uc.new_centroid);
                                            c.face_count = uc.new_face_count;
                                        }
                                    }
                                }
                            }

                            let clip_embedding = processed.clip_embedding.clone();
                            let mime_type = processed.mime_type.clone();

                            // 5. Atomic database transaction
                            match Self::commit_to_db(&pool, &job, processed).await {
                                Ok(thumb_path) => {
                                    let _ = JobRepo::mark_completed(&pool, &job_id).await;
                                    info!(id = %job.asset_id, user_id = %job.user_id, "Asset processed & indexed successfully");

                                    // 6. Convert Vec<f32> to [f32; 512] and update SIMD cache
                                    if let Some(embedding_vec) = clip_embedding {
                                        if embedding_vec.len() == media_processing::EMBEDDING_DIM {
                                            let mut emb_array = [0.0f32; media_processing::EMBEDDING_DIM];
                                            emb_array.copy_from_slice(&embedding_vec);

                                            clip_cache
                                                .insert(media_processing::CachedEmbedding {
                                                    id: job.asset_id.clone(),
                                                    user_id: job.user_id.clone(),
                                                    thumb_path: thumb_path.clone(),
                                                    mime_type,
                                                    embedding: emb_array,
                                                })
                                                .await;
                                        } else {
                                            warn!(
                                                "Embedding dimension mismatch for {}: expected {}, got {}",
                                                job.asset_id,
                                                media_processing::EMBEDDING_DIM,
                                                embedding_vec.len()
                                            );
                                        }
                                    }

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

        let db_thumb_path = format!("users/{}/thumbs/{}", job.user_id, res.thumb_path);
        let db_preview_path = format!("users/{}/thumbs/{}", job.user_id, res.preview_path);

        let clip_embedding_bytes: Option<Vec<u8>> = res
            .clip_embedding
            .as_ref()
            .map(|emb| bytemuck::cast_slice(emb.as_slice()).to_vec());

        let record = NewAssetRecord {
            id: job.asset_id.clone(),
            user_id: job.user_id.clone(),
            sha256: job.sha256.clone(),
            file_name: job.file_name.clone(),
            rel_path: job.rel_path.clone(),
            folder_path: job.folder_path.clone(),
            thumb_path: db_thumb_path.clone(),
            preview_path: db_preview_path,
            file_size_bytes: job.file_size_bytes,
            mime_type: res.mime_type,
            width: res.width,
            height: res.height,
            aspect_ratio: res.aspect_ratio,
            duration_seconds: res.duration_seconds,
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

        // 1. Batch insert new persons with cover_face_id = NULL to satisfy foreign keys
        let new_covers: Vec<(String, String)> = res
            .new_persons
            .iter()
            .filter(|np| !np.cover_face_id.is_empty())
            .map(|np| (np.person_id.clone(), np.cover_face_id.clone()))
            .collect();

        if !res.new_persons.is_empty() {
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO persons (id, user_id, name, cover_face_id, face_count, centroid_embedding) ",
            );

            qb.push_values(res.new_persons, |mut b, np| {
                let embedding_bytes: &[u8] = bytemuck::cast_slice(&np.centroid);
                b.push_bind(np.person_id)
                    .push_bind(&job.user_id)
                    .push_bind(None::<String>) // name is NULL initially
                    .push_bind(None::<String>) // cover_face_id is NULL initially!
                    .push_bind(1i32)
                    .push_bind(embedding_bytes);
            });

            qb.build()
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }

        // 2. Update existing cluster centroids
        for uc in res.updated_clusters {
            let embedding_bytes: &[u8] = bytemuck::cast_slice(&uc.new_centroid);
            sqlx::query(
                "UPDATE persons 
                SET centroid_embedding = ?1, face_count = ?2, updated_at = CURRENT_TIMESTAMP
                WHERE id = ?3 AND user_id = ?4",
            )
            .bind(embedding_bytes)
            .bind(uc.new_face_count)
            .bind(&uc.person_id)
            .bind(&job.user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        // 3. Batch insert detected faces (now both asset_id and person_id exist!)
        if !res.detected_faces.is_empty() {
            let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                "INSERT INTO asset_faces (
                    id, asset_id, person_id, bbox_x, bbox_y, bbox_w, bbox_h,
                    detection_score, face_thumb_path, embedding, is_verified
                ) ",
            );

            qb.push_values(res.detected_faces, |mut b, face| {
                let embedding_bytes: &[u8] = bytemuck::cast_slice(&face.embedding);
                let face_thumb_db_path = format!("users/{}/thumbs/{}", job.user_id, face.face_thumb_rel_path);

                b.push_bind(face.face_id)
                    .push_bind(&job.asset_id)
                    .push_bind(face.person_id)
                    .push_bind(face.bbox_x)
                    .push_bind(face.bbox_y)
                    .push_bind(face.bbox_w)
                    .push_bind(face.bbox_h)
                    .push_bind(face.score)
                    .push_bind(face_thumb_db_path)
                    .push_bind(embedding_bytes)
                    .push_bind(0i32);
            });

            qb.build()
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }

        // 3b. Backfill cover_face_id for new persons now that asset_faces exist
        for (person_id, cover_face_id) in new_covers {
            sqlx::query(
                "UPDATE persons SET cover_face_id = ?1 WHERE id = ?2 AND user_id = ?3",
            )
            .bind(cover_face_id)
            .bind(person_id)
            .bind(&job.user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        // 4. Batch insert/link tags
        if !res.tags.is_empty() {
            let mut tag_qb: QueryBuilder<Sqlite> =
                QueryBuilder::new("INSERT INTO tags (user_id, name, source) ");

            tag_qb.push_values(&res.tags, |mut b, tag| {
                b.push_bind(&job.user_id)
                    .push_bind(&tag.name)
                    .push_bind("model");
            });
            tag_qb.push(" ON CONFLICT(user_id, name) DO NOTHING");

            tag_qb
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

            let mut fetch_qb: QueryBuilder<Sqlite> =
                QueryBuilder::new("SELECT id, name FROM tags WHERE user_id = ");
            fetch_qb.push_bind(&job.user_id);
            fetch_qb.push(" AND name IN (");

            let mut separated = fetch_qb.separated(", ");
            for tag in &res.tags {
                separated.push_bind(&tag.name);
            }
            separated.push_unseparated(")");

            let tag_rows = fetch_qb
                .build()
                .fetch_all(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

            let tag_map: std::collections::HashMap<String, i64> = tag_rows
                .into_iter()
                .filter_map(|r| {
                    let id: i64 = r.try_get("id").ok()?;
                    let name: String = r.try_get("name").ok()?;
                    Some((name.to_lowercase(), id))
                })
                .collect();

            let valid_tags: Vec<(i64, f64)> = res
                .tags
                .iter()
                .filter_map(|t| {
                    tag_map
                        .get(&t.name.to_lowercase())
                        .map(|&id| (id, t.confidence as f64))
                })
                .collect();

            if !valid_tags.is_empty() {
                let mut link_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
                    "INSERT INTO asset_tags (asset_id, tag_id, confidence, source) ",
                );

                link_qb.push_values(valid_tags, |mut b, (tag_id, conf)| {
                    b.push_bind(&job.asset_id)
                        .push_bind(tag_id)
                        .push_bind(conf)
                        .push_bind("AI");
                });

                link_qb.push(" ON CONFLICT(asset_id, tag_id) DO UPDATE SET confidence = excluded.confidence");

                link_qb
                    .build()
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(db_thumb_path)
    }
}