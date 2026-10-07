// photo-app/backend/src/services/queue.rs

use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Semaphore};
use tracing::{error, info, warn};

use db::domain::{DbJob, NewAssetRecord};
use db::tag_repo::{IngestionTagInput, TagRepo};
use db::{AlbumRepo, AssetRepo, IngestionRepo, JobRepo};

use crate::services::engine_coordinator::EngineCoordinator;
use crate::AppState;
use crate::WsMediaEvent;
use media_processing::{AiEnrichmentResult, DerivativeResult, MediaEngine, StorageService};

/// Explicit sender handles for each pipeline stage.
/// Held in `AppState.channels` so HTTP routes can push directly to workers.
#[derive(Clone)]
pub struct QueueChannels {
    pub assemble_tx: mpsc::Sender<DbJob>,
    pub thumb_tx: mpsc::Sender<DbJob>,
    pub ai_tx: mpsc::Sender<DbJob>,
}

pub struct QueueService;

impl QueueService {
    /// Bootstraps all three pipeline workers driven strictly by in-memory mpsc channels.
    /// Integrated with a Tokio watch pause gate to prioritize active upload batches.
    pub fn start_pipeline(
        state: AppState,
        storage_root: PathBuf,
        coordinator: EngineCoordinator,
        concurrency: usize,
        tx_events: broadcast::Sender<WsMediaEvent>,
        mut assemble_rx: mpsc::Receiver<DbJob>,
        mut thumb_rx: mpsc::Receiver<DbJob>,
        mut ai_rx: mpsc::Receiver<DbJob>,
    ) {
        let pool = state.db.clone();
        let pause_rx = state.upload_pause_rx.clone();

        info!(
            assemble_concurrency = 1,
            thumb_concurrency = concurrency,
            ai_concurrency = 1,
            ai_globally_enabled = state.config.ai.enabled,
            ai_faces = state.config.ai.enable_faces,
            ai_clip = state.config.ai.enable_clip,
            ai_tags = state.config.ai.enable_tags,
            ai_poses = state.config.ai.enable_poses,
            "Three-stage push-driven mpsc media pipeline active with upload pause gate"
        );

        // =========================================================================
        // WORKER 0: Assemble & Deduplication (Strict Sequential Disk I/O = 1)
        // =========================================================================
        {
            let pool = pool.clone();
            let storage_root = storage_root.clone();
            let thumb_tx = state.channels.thumb_tx.clone();
            let mut pause_rx = pause_rx.clone();

            tokio::spawn(async move {
                while let Some(job) = assemble_rx.recv().await {
                    // PAUSE GATE: If an upload batch is transferring, yield disk I/O
                    if *pause_rx.borrow() {
                        info!("Worker 0 paused: waiting for active upload transfers to complete...");
                        let _ = pause_rx.wait_for(|paused| !*paused).await;
                        info!("Worker 0 resumed");
                    }

                    let job_id = job.id.clone();
                    let input_path = job.disk_path.clone();
                    let asset_id = job.asset_id.clone();
                    let user_id = job.user_id.clone();
                    let file_name = job.file_name.clone();

                    let folder_path = if job.folder_path.is_empty() || job.folder_path == "root" {
                        format!("Camera Roll/{}", chrono::Local::now().format("%Y-%m"))
                    } else {
                        job.folder_path.clone()
                    };

                    let target_dir = StorageService::resolve_upload_dir(
                        &storage_root,
                        &user_id,
                        &folder_path,
                    );

                    if let Err(e) = tokio::fs::create_dir_all(&target_dir).await {
                        error!("Failed creating storage directory for {}: {}", asset_id, e);
                        let _ = JobRepo::mark_failed(&pool, &job_id, &e.to_string()).await;
                        continue;
                    }

                    let disk_filename = StorageService::generate_disk_filename(&asset_id, &file_name);
                    let final_destination = target_dir.join(&disk_filename);
                    let destination_tmp = target_dir.join(format!("{}.tmp", disk_filename));
                    let dest_tmp_clone = destination_tmp.clone();

                    // Heavy disk I/O & SHA-256 calculation offloaded to blocking thread pool
                    let res = tokio::task::spawn_blocking(move || -> Result<(String, i64), String> {
                        use std::io::{Read, Write};
                        use sha2::{Digest, Sha256};

                        let mut hasher = Sha256::new();
                        let mut buffer = [0u8; 128 * 1024];
                        let mut total_bytes = 0i64;

                        if input_path.is_dir() {
                            // Chunked upload session
                            let meta_path = input_path.join("total_chunks");
                            let total_chunks_str = std::fs::read_to_string(&meta_path)
                                .map_err(|e| format!("Missing total_chunks metadata: {e}"))?;
                            let total_chunks: u32 = total_chunks_str.trim().parse()
                                .map_err(|e| format!("Invalid total_chunks value: {e}"))?;

                            let mut assembled = std::fs::OpenOptions::new()
                                .create(true)
                                .write(true)
                                .truncate(true)
                                .open(&dest_tmp_clone)
                                .map_err(|e| format!("Failed creating destination file: {e}"))?;

                            for idx in 0..total_chunks {
                                let part_path = input_path.join(format!("{:06}.part", idx));
                                let mut part = std::fs::File::open(&part_path)
                                    .map_err(|e| format!("Missing chunk part {idx}: {e}"))?;

                                loop {
                                    let n = part.read(&mut buffer).map_err(|e| e.to_string())?;
                                    if n == 0 { break; }
                                    assembled.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
                                    hasher.update(&buffer[..n]);
                                    total_bytes += n as i64;
                                }
                            }

                            assembled.flush().map_err(|e| e.to_string())?;
                        } else {
                            // Direct staged file
                            let mut f = std::fs::File::open(&input_path)
                                .map_err(|e| format!("Failed opening staged file: {e}"))?;

                            loop {
                                let n = f.read(&mut buffer).map_err(|e| e.to_string())?;
                                if n == 0 { break; }
                                hasher.update(&buffer[..n]);
                                total_bytes += n as i64;
                            }
                            drop(f);

                            if let Err(_) = std::fs::rename(&input_path, &dest_tmp_clone) {
                                std::fs::copy(&input_path, &dest_tmp_clone)
                                    .map_err(|e| format!("Failed copying to destination: {e}"))?;
                                let _ = std::fs::remove_file(&input_path);
                            }
                        }

                        let sha256 = hex::encode(hasher.finalize());
                        Ok((sha256, total_bytes))
                    }).await;

                    let (sha256, file_size_bytes) = match res {
                        Ok(Ok(val)) => val,
                        Ok(Err(e)) => {
                            error!("Assemble error on {}: {}", asset_id, e);
                            let _ = tokio::fs::remove_file(&destination_tmp).await;
                            let _ = JobRepo::mark_failed(&pool, &job_id, &e).await;
                            continue;
                        }
                        Err(join_err) => {
                            error!("Assemble panic on {}: {}", asset_id, join_err);
                            let _ = tokio::fs::remove_file(&destination_tmp).await;
                            let _ = JobRepo::mark_failed(&pool, &job_id, &join_err.to_string()).await;
                            continue;
                        }
                    };

                    // Deduplication check in SQLite
                    if let Ok(Some(existing_id)) = AssetRepo::find_user_asset_by_sha256(&pool, &user_id, &sha256).await {
                        let _ = tokio::fs::remove_file(&destination_tmp).await;
                        if job.disk_path.is_dir() {
                            let _ = tokio::fs::remove_dir_all(&job.disk_path).await;
                        }

                        if let Ok(mut tx) = pool.begin().await {
                            let _ = AlbumRepo::link_asset_to_folder_albums_tx(
                                &mut tx,
                                &user_id,
                                &existing_id,
                                &folder_path,
                            ).await;
                            let _ = tx.commit().await;
                        }

                        let _ = JobRepo::mark_completed(&pool, &job_id).await;
                        info!(asset_id = %existing_id, "Duplicate resolved and linked to album");
                        continue;
                    }

                    // Atomic in-place rename on filesystem
                    if let Err(e) = tokio::fs::rename(&destination_tmp, &final_destination).await {
                        error!("Failed final rename for {}: {}", asset_id, e);
                        let _ = tokio::fs::remove_file(&destination_tmp).await;
                        let _ = JobRepo::mark_failed(&pool, &job_id, &e.to_string()).await;
                        continue;
                    }

                    if job.disk_path.is_dir() {
                        let _ = tokio::fs::remove_dir_all(&job.disk_path).await;
                    }

                    let rel_path = format!("{}/{}", folder_path, disk_filename);
                    let _ = JobRepo::mark_completed(&pool, &job_id).await;

                    // Push directly into Stage 1 (Thumbnail channel)
                    let thumb_job = DbJob {
                        id: uuid::Uuid::new_v4().to_string(),
                        user_id,
                        asset_id,
                        file_name,
                        rel_path,
                        folder_path,
                        disk_path: final_destination,
                        sha256,
                        job_type: "thumbnail".to_string(),
                        file_size_bytes,
                        payload: job.payload,
                        ai_faces_done: 0,
                        ai_clip_done: 0,
                        ai_tags_done: 0,
                        ai_poses_done: 0,
                    };

                    // Persist to WAL
                    let _ = JobRepo::enqueue(&pool, &thumb_job).await;
                    // Push to Worker 1
                    let _ = thumb_tx.send(thumb_job).await;
                }
            });
        }

        // =========================================================================
        // WORKER 1: Thumbnails & Derivatives (Parallelized Concurrency: 2 - 4)
        // =========================================================================
        {
            let pool = pool.clone();
            let storage_root = storage_root.clone();
            let tx_events = tx_events.clone();
            let ai_tx = state.channels.ai_tx.clone();
            let thumb_sem = Arc::new(Semaphore::new(concurrency.max(1)));
            let mut pause_rx = pause_rx.clone();
            let ai_enabled = state.config.ai.enabled;

            tokio::spawn(async move {
                while let Some(job) = thumb_rx.recv().await {
                    // PAUSE GATE: Yield before acquiring work permits if an upload is active
                    if *pause_rx.borrow() {
                        let _ = pause_rx.wait_for(|paused| !*paused).await;
                    }

                    let permit = match thumb_sem.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => break,
                    };

                    let pool = pool.clone();
                    let storage_root = storage_root.clone();
                    let tx_events = tx_events.clone();
                    let ai_tx = ai_tx.clone();

                    tokio::spawn(async move {
                        let job_id = job.id.clone();
                        let asset_id = job.asset_id.clone();
                        let user_id = job.user_id.clone();

                        let user_thumbs_dir = storage_root.join("users").join(&user_id).join("thumbs");
                        if let Err(e) = tokio::fs::create_dir_all(&user_thumbs_dir).await {
                            error!("Failed creating thumbs dir for {}: {}", user_id, e);
                            let _ = JobRepo::mark_failed(&pool, &job_id, &e.to_string()).await;
                            drop(permit);
                            return;
                        }

                        let disk_path = job.disk_path.clone();
                        let asset_id_clone = asset_id.clone();
                        let thumbs_dir_clone = user_thumbs_dir.clone();

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
                                let _ = tx_events.send(WsMediaEvent::AssetFailed {
                                    asset_id,
                                    error: err_msg,
                                });
                                drop(permit);
                                return;
                            }
                            Err(join_err) => {
                                let err_msg = join_err.to_string();
                                error!("Thumbnail thread panic on {}: {}", asset_id, err_msg);
                                let _ = JobRepo::mark_failed(&pool, &job_id, &err_msg).await;
                                let _ = tx_events.send(WsMediaEvent::AssetFailed {
                                    asset_id,
                                    error: err_msg,
                                });
                                drop(permit);
                                return;
                            }
                        };

                        match Self::commit_base_asset(&pool, &job, derivatives).await {
                            Ok(thumb_path) => {
                                let _ = JobRepo::mark_completed(&pool, &job_id).await;

                                // 1. Notify that thumbnail/preview image is live
                                let _ = tx_events.send(WsMediaEvent::AssetReady {
                                    asset_id: job.asset_id.clone(),
                                    thumb_path,
                                    folder_path: job.folder_path.clone(),
                                });

                                // 2. Notify that the album/folder collection was updated
                                if !job.folder_path.is_empty() && job.folder_path != "root" {
                                    let _ = tx_events.send(WsMediaEvent::AlbumUpdated {
                                        album_id: None,
                                        folder_path: job.folder_path.clone(),
                                        asset_id: job.asset_id.clone(),
                                    });
                                }
                            }
                            Err(e) => {
                                error!("Base asset DB commit failed on {}: {}", asset_id, e);
                                let _ = JobRepo::mark_failed(&pool, &job_id, &e).await;
                                let _ = tx_events.send(WsMediaEvent::AssetFailed {
                                    asset_id,
                                    error: e,
                                });
                                drop(permit);
                                return;
                            }
                        };

                        // Release the thumbnail concurrency permit immediately so
                        // subsequent thumbnail jobs can start decoding without waiting on downstream AI
                        drop(permit);

                        // Stage 2: Hand off to AI Enrichment ONLY if AI is enabled globally
                        if ai_enabled {
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
                                payload: job.payload,
                                ai_faces_done: 0,
                                ai_clip_done: 0,
                                ai_tags_done: 0,
                                ai_poses_done: 0,
                            };

                            let _ = JobRepo::enqueue(&pool, &ai_job).await;

                            // Send with a 5-second timeout safeguard to prevent deadlock if ai_rx is congested
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(5),
                                ai_tx.send(ai_job),
                            )
                            .await;
                        }
                    });
                }
            });
        }

        // =========================================================================
        // WORKER 2: Config-Gated Background AI Enrichment (Strict Concurrency = 1)
        // =========================================================================
        {
            let pool = pool.clone();
            let storage_root = storage_root.clone();
            let coordinator = coordinator.clone();
            let tx_events = tx_events.clone();
            let config_ai = state.config.ai.clone();
            let mut pause_rx = pause_rx.clone();

            tokio::spawn(async move {
                while let Some(job) = ai_rx.recv().await {
                    let job_id = job.id.clone();
                    let asset_id = job.asset_id.clone();
                    let user_id = job.user_id.clone();

                    // 1. If AI is globally disabled, skip entirely and leave checkpoints as 0
                    if !config_ai.enabled {
                        info!(asset_id = %asset_id, "AI globally disabled: leaving enrichment pending");
                        // We mark failed or keep pending so backfill scanner picks it up when turned back on
                        let _ = sqlx::query(
                            "UPDATE processing_jobs SET status = 'pending', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                        )
                        .bind(&job_id)
                        .execute(&pool)
                        .await;
                        continue;
                    }

                    // 2. Identify remaining work based on config toggles AND prior job checkpoints
                    let need_faces = config_ai.enable_faces && job.ai_faces_done == 0;
                    let need_clip = config_ai.enable_clip && job.ai_clip_done == 0;
                    let need_tags = config_ai.enable_tags && job.ai_tags_done == 0;
                    let need_poses = config_ai.enable_poses && job.ai_poses_done == 0;

                    // If everything configured is already done for this asset, finalize and proceed
                    if !need_faces && !need_clip && !need_tags && !need_poses {
                        let _ = JobRepo::mark_completed(&pool, &job_id).await;
                        continue;
                    }

                    // PAUSE GATE: Yield tensor inference immediately during active network uploads
                    if *pause_rx.borrow() {
                        let _ = pause_rx.wait_for(|paused| !*paused).await;
                    }

                    let media_engine = match coordinator.ensure_pipeline_engine().await {
                        Ok(e) => e,
                        Err(err) => {
                            let _ = JobRepo::mark_failed(&pool, &job_id, &err.to_string()).await;
                            continue;
                        }
                    };

                    let cluster_cache = match coordinator.ensure_cluster_cache().await {
                        Ok(c) => c,
                        Err(err) => {
                            let _ = JobRepo::mark_failed(&pool, &job_id, &err.to_string()).await;
                            continue;
                        }
                    };

                    let clip_cache = match coordinator.ensure_clip_cache().await {
                        Ok(c) => c,
                        Err(err) => {
                            let _ = JobRepo::mark_failed(&pool, &job_id, &err.to_string()).await;
                            continue;
                        }
                    };

                    coordinator.keep_warm().await;

                    let known_clusters = if need_faces {
                        let read_guard = cluster_cache.read().await;
                        read_guard.get(&user_id).cloned().unwrap_or_default()
                    } else {
                        Vec::new()
                    };

                    let user_thumbs_dir = storage_root.join("users").join(&user_id).join("thumbs");
                    let disk_path = job.disk_path.clone();
                    let engine = media_engine.clone();
                    let asset_id_for_ai = asset_id.clone();

                    let phase2_res = tokio::task::spawn_blocking(move || {
                        engine.process_ai_sync(
                            &disk_path,
                            &asset_id_for_ai,
                            &user_thumbs_dir,
                            known_clusters,
                        )
                    })
                    .await;

                    let mut ai_result = match phase2_res {
                        Ok(Ok(data)) => data,
                        Ok(Err(e)) => {
                            warn!("AI inference skipped/failed on {}: {}", asset_id, e);
                            let _ = JobRepo::mark_failed(&pool, &job_id, &e.to_string()).await;
                            continue;
                        }
                        Err(join_err) => {
                            warn!("AI inference panic on {}: {}", asset_id, join_err);
                            let _ = JobRepo::mark_failed(&pool, &job_id, &join_err.to_string()).await;
                            continue;
                        }
                    };

                    // 3. Filter outputs according to what was requested and not yet done
                    if !need_faces {
                        ai_result.detected_faces.clear();
                        ai_result.new_persons.clear();
                        ai_result.updated_clusters.clear();
                    }

                    if !need_tags {
                        ai_result.tags.clear();
                    }

                    if !need_clip {
                        ai_result.clip_embedding = None;
                    }

                    if !need_poses {
                        ai_result.poses.clear();
                    }

                    // Face clustering updates (if faces were processed)
                    let mut affected_ids: Vec<String> = Vec::new();
                    let new_people_count = ai_result.new_persons.len();

                    if need_faces && (!ai_result.new_persons.is_empty() || !ai_result.updated_clusters.is_empty()) {
                        let mut write_guard = cluster_cache.write().await;
                        let user_bucket = write_guard.entry(user_id.clone()).or_default();

                        for np in &ai_result.new_persons {
                            affected_ids.push(np.person_id.clone());
                            if np.centroid.len() == media_processing::EMBEDDING_DIM {
                                let mut emb_arr = [0.0f32; media_processing::EMBEDDING_DIM];
                                emb_arr.copy_from_slice(&np.centroid);
                                user_bucket.push(media_processing::KnownPersonCluster {
                                    person_id: np.person_id.clone(),
                                    face_count: 1,
                                    cover_face_id: Some(np.cover_face_id.clone()),
                                    exemplars: vec![emb_arr],
                                });
                            }
                        }

                        for uc in &ai_result.updated_clusters {
                            affected_ids.push(uc.person_id.clone());
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

                    // SIMD CLIP vector cache update (if CLIP was processed)
                    if need_clip {
                        if let Some(ref embedding_vec) = ai_result.clip_embedding {
                            if embedding_vec.len() == media_processing::EMBEDDING_DIM {
                                let mut emb_array = [0.0f32; media_processing::EMBEDDING_DIM];
                                emb_array.copy_from_slice(embedding_vec);

                                if let Ok(Some(meta)) = AssetRepo::get_cache_metadata(&pool, &asset_id, &user_id).await {
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
                    }

                    let detected_faces_count = ai_result.detected_faces.len();
                    let tags_count = ai_result.tags.len();

                    // Persist enriched metadata
                    if let Err(e) = Self::commit_ai_enrichment(&pool, &job, ai_result).await {
                        warn!("Failed persisting AI metadata for {}: {}", job.asset_id, e);
                    }

                    // 4. Update checkpoints in DB atomically
                    let _ = JobRepo::update_ai_checkpoints(
                        &pool,
                        &job_id,
                        need_faces,
                        need_clip,
                        need_tags,
                        need_poses,
                        config_ai.enable_faces,
                        config_ai.enable_clip,
                        config_ai.enable_tags,
                        config_ai.enable_poses,
                    )
                    .await;

                    coordinator.keep_warm().await;

                    if !affected_ids.is_empty() {
                        let _ = tx_events.send(WsMediaEvent::PeopleUpdated {
                            user_id: user_id.clone(),
                            new_people_count,
                            affected_person_ids: affected_ids,
                        });
                    }

                    let _ = tx_events.send(WsMediaEvent::AiCompleted {
                        asset_id: job.asset_id.clone(),
                        faces_detected: detected_faces_count,
                        tags_count,
                    });

                    info!(id = %job.asset_id, user_id = %job.user_id, "Granular AI sub-tasks executed and committed");
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

        let p = job.payload.as_ref();
        let author = p.and_then(|x| x.author.clone());
        let platform = p.and_then(|x| x.platform.clone());
        let source_url = p.and_then(|x| x.source_url.clone());
        let source_post_id = p.and_then(|x| x.source_post_id.clone());
        let caption = p.and_then(|x| x.caption.clone());

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
            author: author.clone(),
            source_platform: platform,
            source_url,
            source_post_id,
            caption: caption.clone(),
            clip_embedding: None,
        };

        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

        // 1. Insert core asset record
        AssetRepo::insert_asset_tx(&mut *tx, &asset)
            .await
            .map_err(|e| e.to_string())?;

        // 2. Link asset to folder albums (always receives the normalized folder_path)
        if !job.folder_path.is_empty() && job.folder_path != "root" {
            AlbumRepo::link_asset_to_folder_albums_tx(
                &mut *tx,
                &job.user_id,
                &job.asset_id,
                &job.folder_path,
            )
            .await
            .map_err(|e| format!("Failed linking asset to album: {e}"))?;
        }

        // 3. Commit scraped hashtags and author to tags and FTS5 index
        if let Some(payload) = p {
            let mut initial_tags = Vec::new();

            for tag_str in &payload.tags {
                initial_tags.push(IngestionTagInput::new_hashtag(tag_str));
            }

            if let Some(ref a) = payload.author {
                if !a.is_empty() && a != "unknown" && a != "web" {
                    initial_tags.push(IngestionTagInput::new_author(a));
                }
            }

            if !initial_tags.is_empty() {
                TagRepo::save_asset_tags_tx(&mut tx, &job.user_id, &job.asset_id, &initial_tags)
                    .await
                    .map_err(|e| e.to_string())?;
            }

            let author_val = author.unwrap_or_default();
            let caption_val = caption.unwrap_or_default();

            sqlx::query("DELETE FROM asset_search_index WHERE asset_id = ?1")
                .bind(&job.asset_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

            let tags_str = payload.tags.join(" ");

            sqlx::query(
                r#"
                INSERT INTO asset_search_index (asset_id, user_id, caption, author, tags, file_name)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
            )
            .bind(&job.asset_id)
            .bind(&job.user_id)
            .bind(&caption_val)
            .bind(&author_val)
            .bind(&tags_str)
            .bind(&job.file_name)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

        // 4. Mark scraped item downloaded if linked
        if let Some(scraped_id) = p.and_then(|x| x.scraped_item_id.as_deref()) {
            sqlx::query(
                "UPDATE scraped_media_items SET status = 'downloaded', asset_id = ?1 WHERE id = ?2",
            )
            .bind(&job.asset_id)
            .bind(scraped_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }

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

        let ai_tags = res
            .tags
            .into_iter()
            .map(|t| IngestionTagInput::new_ai(t.name, t.confidence))
            .collect();

        let poses = res
            .poses
            .into_iter()
            .map(|pose| {
                let keypoints_json = serde_json::to_string(&pose.keypoints)
                    .unwrap_or_else(|_| "[]".to_string());

                db::ingestion_repo::IngestionDetectedPose {
                    id: uuid::Uuid::new_v4().to_string(),
                    score: pose.score,
                    bbox_x: pose.x,
                    bbox_y: pose.y,
                    bbox_w: pose.w,
                    bbox_h: pose.h,
                    keypoints_json,
                }
            })
            .collect();

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
                    bbox_w: face.bbox_h,
                    bbox_h: face.bbox_h,
                    score: face.score,
                    face_thumb_db_path: format!("users/{}/thumbs/{}", job.user_id, face.face_thumb_rel_path),
                    embedding: face.embedding,
                })
                .collect(),
            poses,
            tags: ai_tags,
        };

        IngestionRepo::commit_ai_metadata(pool, payload)
            .await
            .map_err(|e| e.to_string())
    }
}