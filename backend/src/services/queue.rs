use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Notify, Semaphore};
use tracing::{error, info, warn};

use db::domain::{DbJob, NewAssetRecord};
use db::{AssetRepo, IngestionRepo, JobRepo};

use crate::services::engine_coordinator::EngineCoordinator;
use crate::WsMediaEvent;
use media_processing::{AiEnrichmentResult, DerivativeResult, MediaEngine};

pub type ProcessJob = DbJob;

pub struct QueueService;

impl QueueService {
    pub async fn enqueue(
        pool: &SqlitePool,
        notify: &Arc<Notify>,
        mut job: ProcessJob,
    ) -> Result<(), sqlx::Error> {
        if job.job_type.trim().is_empty() {
            job.job_type = "thumbnail".to_string();
        }
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
        let thumb_semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
        // Keep AI worker concurrency strictly constrained to avoid CPU/GPU lockup
        let ai_semaphore = Arc::new(Semaphore::new(1));

        info!(
            thumb_concurrency = concurrency,
            ai_concurrency = 1,
            "Dual-stage media queue workers active (Thumbnail + Background AI)"
        );

        // =========================================================================
        // WORKER 1: High-Speed Thumbnail & Preview Generation
        // =========================================================================
        {
            let pool = pool.clone();
            let storage_root = storage_root.clone();
            let notify = notify.clone();
            let tx_events = tx_events.clone();

            tokio::spawn(async move {
                let _ = JobRepo::reset_interrupted(&pool).await;

                loop {
                    match JobRepo::fetch_next_job(&pool, "thumbnail").await {
                        Ok(Some(job)) => {
                            let permit = match thumb_semaphore.clone().acquire_owned().await {
                                Ok(p) => p,
                                Err(_) => break,
                            };

                            let pool = pool.clone();
                            let storage_root = storage_root.clone();
                            let tx_events = tx_events.clone();
                            let notify = notify.clone();

                            tokio::spawn(async move {
                                let _guard = permit;
                                let job_id = job.id.clone();
                                let asset_id = job.asset_id.clone();
                                let user_id = job.user_id.clone();

                                let user_thumbs_dir = storage_root
                                    .join("users")
                                    .join(&user_id)
                                    .join("thumbs");

                                if let Err(e) = tokio::fs::create_dir_all(&user_thumbs_dir).await {
                                    error!("Failed creating user thumbs dir for {}: {}", user_id, e);
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &e.to_string()).await;
                                    return;
                                }

                                let disk_path = job.disk_path.clone();
                                let asset_id_clone = asset_id.clone();
                                let thumbs_dir_clone = user_thumbs_dir.clone();

                                // Fast derivative generation: zero ONNX models loaded
                                let phase1_res = tokio::task::spawn_blocking(move || {
                                    MediaEngine::process_derivatives_sync(
                                        &disk_path,
                                        &asset_id_clone,
                                        &thumbs_dir_clone,
                                    )
                                })
                                .await;

                                let derivatives = match phase1_res {
                                    Ok(Ok(data)) => data,
                                    Ok(Err(e)) => {
                                        let err_msg = e.to_string();
                                        error!("Thumbnail extraction error on {}: {}", asset_id, err_msg);
                                        let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                                        let _ = tx_events.send(WsMediaEvent {
                                            event_type: "asset_failed".to_string(),
                                            asset_id,
                                            thumb_path: String::new(),
                                        });
                                        return;
                                    }
                                    Err(join_err) => {
                                        let err_msg = join_err.to_string();
                                        error!("Thumbnail thread panic on {}: {}", asset_id, err_msg);
                                        let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                                        let _ = tx_events.send(WsMediaEvent {
                                            event_type: "asset_failed".to_string(),
                                            asset_id,
                                            thumb_path: String::new(),
                                        });
                                        return;
                                    }
                                };

                                // Insert base asset record into SQLite and emit WS event immediately
                                match Self::commit_base_asset(&pool, &job, derivatives).await {
                                    Ok(thumb_path) => {
                                        let _ = JobRepo::mark_completed(&pool, &job_id).await;
                                        let _ = tx_events.send(WsMediaEvent {
                                            event_type: "asset_ready".to_string(),
                                            asset_id: job.asset_id.clone(),
                                            thumb_path,
                                        });
                                    }
                                    Err(e) => {
                                        error!("Base asset DB commit failed on {}: {}", asset_id, e);
                                        let _ = JobRepo::mark_failed(&pool, &job_id, &e).await;
                                        let _ = tx_events.send(WsMediaEvent {
                                            event_type: "asset_failed".to_string(),
                                            asset_id,
                                            thumb_path: String::new(),
                                        });
                                        return;
                                    }
                                };

                                // Enqueue background AI enrichment job
                                let ai_job = DbJob {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    user_id: job.user_id,
                                    asset_id: job.asset_id,
                                    file_name: job.file_name,
                                    rel_path: job.rel_path,
                                    folder_path: job.folder_path,
                                    disk_path: job.disk_path,
                                    sha256: job.sha256,
                                    job_type: "ai_enrichment".to_string(),
                                    file_size_bytes: job.file_size_bytes,
                                };

                                if let Err(e) = JobRepo::enqueue(&pool, &ai_job).await {
                                    warn!("Failed scheduling AI job for {}: {}", ai_job.asset_id, e);
                                } else {
                                    notify.notify_one();
                                }
                            });
                        }
                        Ok(None) => {
                            tokio::select! {
                                _ = notify.notified() => {},
                                _ = tokio::time::sleep(Duration::from_secs(10)) => {},
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to poll thumbnail jobs");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
            });
        }

        // =========================================================================
        // WORKER 2: Background AI Enrichment (Faces, Tags, CLIP)
        // =========================================================================
        {
            let pool = pool.clone();
            let storage_root = storage_root.clone();
            let notify = notify.clone();
            let coordinator = coordinator.clone();

            tokio::spawn(async move {
                loop {
                    // Poll next pending AI job
                    let job = match JobRepo::fetch_next_job(&pool, "ai_enrichment").await {
                        Ok(Some(job)) => job,
                        Ok(None) => {
                            tokio::select! {
                                _ = notify.notified() => {},
                                _ = tokio::time::sleep(Duration::from_secs(10)) => {},
                            }
                            continue;
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to poll AI jobs");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    };

                    // Acquire concurrency permit (strictly 1 to preserve CPU/NPU & prevent DB locks)
                    let _permit = match ai_semaphore.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => break,
                    };

                    let job_id = job.id.clone();
                    let asset_id = job.asset_id.clone();
                    let user_id = job.user_id.clone();

                    // 1. Acquire Model Engines & Caches
                    let media_engine = match coordinator.ensure_pipeline_engine().await {
                        Ok(e) => e,
                        Err(err) => {
                            let err_msg = err.to_string();
                            error!("Failed acquiring MediaEngine for {}: {}", asset_id, err_msg);
                            let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                            continue;
                        }
                    };

                    let cluster_cache = match coordinator.ensure_cluster_cache().await {
                        Ok(c) => c,
                        Err(err) => {
                            let err_msg = err.to_string();
                            error!("Failed acquiring ClusterCache for {}: {}", asset_id, err_msg);
                            let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                            continue;
                        }
                    };

                    let clip_cache = match coordinator.ensure_clip_cache().await {
                        Ok(c) => c,
                        Err(err) => {
                            let err_msg = err.to_string();
                            error!("Failed acquiring ClipCache for {}: {}", asset_id, err_msg);
                            let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                            continue;
                        }
                    };

                    coordinator.keep_warm().await;

                    let known_clusters = {
                        let read_guard = cluster_cache.read().await;
                        read_guard.get(&user_id).cloned().unwrap_or_default()
                    };

                    let user_thumbs_dir = storage_root
                        .join("users")
                        .join(&user_id)
                        .join("thumbs");

                    let disk_path = job.disk_path.clone();
                    let engine = media_engine.clone();
                    let asset_id_for_ai = asset_id.clone();

                    // 2. Offload heavy ML inference via spawn_blocking and AWAIT it
                    let phase2_res = tokio::task::spawn_blocking(move || {
                        engine.process_ai_sync(
                            &disk_path,
                            &asset_id_for_ai,
                            &user_thumbs_dir,
                            known_clusters,
                        )
                    })
                    .await;

                    let ai_result = match phase2_res {
                        Ok(Ok(data)) => data,
                        Ok(Err(e)) => {
                            warn!("AI inference skipped/failed on {}: {}", asset_id, e);
                            let _ = JobRepo::mark_completed(&pool, &job_id).await;
                            continue;
                        }
                        Err(join_err) => {
                            warn!("AI inference panic on {}: {}", asset_id, join_err);
                            let _ = JobRepo::mark_completed(&pool, &job_id).await;
                            continue;
                        }
                    };

                    // 3. Update in-memory face cluster cache
                    if !ai_result.new_persons.is_empty() || !ai_result.updated_clusters.is_empty() {
                        let mut write_guard = cluster_cache.write().await;
                        let user_bucket = write_guard.entry(user_id.clone()).or_default();

                        for np in &ai_result.new_persons {
                            if np.centroid.len() == media_processing::EMBEDDING_DIM {
                                let mut emb_arr = [0.0f32; media_processing::EMBEDDING_DIM];
                                emb_arr.copy_from_slice(&np.centroid);

                                user_bucket.push(media_processing::KnownPersonCluster {
                                    person_id: np.person_id.clone(),
                                    face_count: 1,
                                    cover_face_id: Some(np.cover_face_id.clone()),
                                    exemplars: vec![emb_arr], // Initial exemplar
                                });
                            }
                        }

                        for uc in &ai_result.updated_clusters {
                            if let Some(c) = user_bucket.iter_mut().find(|c| c.person_id == uc.person_id) {
                                c.face_count = uc.new_face_count;
                                if uc.new_centroid.len() == media_processing::EMBEDDING_DIM && c.exemplars.len() < 5 {
                                    let mut emb_arr = [0.0f32; media_processing::EMBEDDING_DIM];
                                    emb_arr.copy_from_slice(&uc.new_centroid);
                                    c.exemplars.push(emb_arr);
                                }
                            }
                        }
                    }

                    // 4. Update SIMD CLIP vector cache
                    if let Some(ref embedding_vec) = ai_result.clip_embedding {
                        if embedding_vec.len() == media_processing::EMBEDDING_DIM {
                            let mut emb_array = [0.0f32; media_processing::EMBEDDING_DIM];
                            emb_array.copy_from_slice(embedding_vec);

                            let meta_opt = match AssetRepo::get_cache_metadata(&pool, &asset_id, &user_id).await {
                                Ok(m) => m,
                                Err(e) => {
                                    warn!("Failed fetching asset metadata for cache on {}: {}", asset_id, e);
                                    None
                                }
                            };

                            if let Some(meta) = meta_opt {
                                clip_cache
                                    .insert(media_processing::CachedEmbedding {
                                        id: job.asset_id.clone(),
                                        user_id: job.user_id.clone(),
                                        thumb_path: meta.thumb_path,
                                        mime_type: meta.mime_type,
                                        embedding: emb_array,
                                    })
                                    .await;
                            }
                        }
                    }

                    // 5. Persist AI enrichments to database
                    if let Err(e) = Self::commit_ai_enrichment(&pool, &job, ai_result).await {
                        warn!("Failed persisting AI metadata for {}: {}", job.asset_id, e);
                    }

                    let _ = JobRepo::mark_completed(&pool, &job_id).await;
                    coordinator.keep_warm().await;
                    info!(id = %job.asset_id, user_id = %job.user_id, "Asset fully indexed with AI");

                    // `_permit` drops here automatically at the end of the job iteration
                }
            });
        }
    }

    async fn commit_base_asset(
        pool: &SqlitePool,
        job: &DbJob,
        derivatives: DerivativeResult,
    ) -> Result<String, String> {
        let db_thumb_path = format!("users/{}/thumbs/{}", job.user_id, derivatives.thumb_path);
        let db_preview_path = format!("users/{}/thumbs/{}", job.user_id, derivatives.preview_path);

        let asset = NewAssetRecord {
            id: job.asset_id.clone(),
            user_id: job.user_id.clone(),
            sha256: job.sha256.clone(),
            file_name: job.file_name.clone(),
            rel_path: job.rel_path.clone(),
            folder_path: job.folder_path.clone(),
            thumb_path: db_thumb_path.clone(),
            preview_path: db_preview_path,
            file_size_bytes: job.file_size_bytes,
            mime_type: derivatives.mime_type,
            width: derivatives.width,
            height: derivatives.height,
            aspect_ratio: derivatives.aspect_ratio,
            duration_seconds: derivatives.duration_seconds,
            captured_at: derivatives.meta.captured_at,
            year: derivatives.meta.year,
            month: derivatives.meta.month,
            day: derivatives.meta.day,
            hour: derivatives.meta.hour,
            latitude: derivatives.meta.latitude,
            longitude: derivatives.meta.longitude,
            altitude: derivatives.meta.altitude,
            city: derivatives.meta.city,
            subdivision: derivatives.meta.subdivision,
            country: derivatives.meta.country,
            country_code: derivatives.meta.country_code,
            camera_make: derivatives.meta.camera_make,
            camera_model: derivatives.meta.camera_model,
            clip_embedding: None,
        };

        // AssetRepo requires an active transaction handle
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        AssetRepo::insert_asset_tx(&mut *tx, &asset)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;

        Ok(db_thumb_path)
    }

    async fn commit_ai_enrichment(
        pool: &SqlitePool,
        job: &DbJob,
        res: AiEnrichmentResult,
    ) -> Result<(), String> {
        let clip_embedding_bytes = res
            .clip_embedding
            .as_ref()
            .map(|emb| bytemuck::cast_slice(emb.as_slice()).to_vec());

        let payload = db::AiEnrichmentPayload {
            asset_id: job.asset_id.clone(),
            user_id: job.user_id.clone(),
            clip_embedding: clip_embedding_bytes,
            new_persons: res
                .new_persons
                .into_iter()
                .map(|np| db::IngestionNewPerson {
                    person_id: np.person_id,
                    cover_face_id: np.cover_face_id,
                    centroid: np.centroid,
                })
                .collect(),
            updated_clusters: res
                .updated_clusters
                .into_iter()
                .map(|uc| db::IngestionUpdatedCluster {
                    person_id: uc.person_id,
                    new_centroid: uc.new_centroid,
                    new_face_count: uc.new_face_count,
                })
                .collect(),
            detected_faces: res
                .detected_faces
                .into_iter()
                .map(|face| db::IngestionDetectedFace {
                    face_id: face.face_id,
                    person_id: face.person_id,
                    bbox_x: face.bbox_x,
                    bbox_y: face.bbox_y,
                    bbox_w: face.bbox_w,
                    bbox_h: face.bbox_h,
                    score: face.score,
                    face_thumb_db_path: format!("users/{}/thumbs/{}", job.user_id, face.face_thumb_rel_path),
                    embedding: face.embedding,
                })
                .collect(),
            tags: res.tags.into_iter().map(|t| (t.name, t.confidence)).collect(),
        };

        IngestionRepo::commit_ai_metadata(pool, payload)
            .await
            .map_err(|e| e.to_string())
    }
}