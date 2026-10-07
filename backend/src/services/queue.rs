// photo-app/backend/src/services/queue.rs

use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Semaphore};
use tracing::{error, info, warn};
use uuid::Uuid;

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

                    let job_id = job.id;
                    let input_path = job.disk_path.clone();
                    let asset_id = job.asset_id;
                    let user_id = job.user_id;
                    let file_name = job.file_name.clone();

                    let folder_path = if job.folder_path.is_empty() || job.folder_path == "root" {
                        format!("Camera Roll/{}", chrono::Local::now().format("%Y-%m"))
                    } else {
                        job.folder_path.clone()
                    };

                    let target_dir = StorageService::resolve_upload_dir(
                        &storage_root,
                        &user_id.to_string(),
                        &folder_path,
                    );

                    if let Err(e) = tokio::fs::create_dir_all(&target_dir).await {
                        error!("Failed creating storage directory for {}: {}", asset_id, e);
                        let _ = JobRepo::mark_failed(&pool, job_id, &e.to_string()).await;
                        continue;
                    }

                    let disk_filename = StorageService::generate_disk_filename(&asset_id.to_string(), &file_name);
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
                            let _ = JobRepo::mark_failed(&pool, job_id, &e).await;
                            continue;
                        }
                        Err(join_err) => {
                            error!("Assemble panic on {}: {}", asset_id, join_err);
                            let _ = tokio::fs::remove_file(&destination_tmp).await;
                            let _ = JobRepo::mark_failed(&pool, job_id, &join_err.to_string()).await;
                            continue;
                        }
                    };

                    // Deduplication check in PostgreSQL
                    if let Ok(Some(existing_id)) = AssetRepo::find_user_asset_by_sha256(&pool, user_id, &sha256).await {
                        let _ = tokio::fs::remove_file(&destination_tmp).await;
                        if job.disk_path.is_dir() {
                            let _ = tokio::fs::remove_dir_all(&job.disk_path).await;
                        }

                        if let Ok(mut tx) = pool.begin().await {
                            let _ = AlbumRepo::link_asset_to_folder_albums_tx(
                                &mut tx,
                                user_id,
                                existing_id,
                                &folder_path,
                            ).await;
                            let _ = tx.commit().await;
                        }

                        let _ = JobRepo::mark_completed(&pool, job_id).await;
                        info!(asset_id = %existing_id, "Duplicate resolved and linked to album");
                        continue;
                    }

                    // Atomic in-place rename on filesystem
                    if let Err(e) = tokio::fs::rename(&destination_tmp, &final_destination).await {
                        error!("Failed final rename for {}: {}", asset_id, e);
                        let _ = tokio::fs::remove_file(&destination_tmp).await;
                        let _ = JobRepo::mark_failed(&pool, job_id, &e.to_string()).await;
                        continue;
                    }

                    if job.disk_path.is_dir() {
                        let _ = tokio::fs::remove_dir_all(&job.disk_path).await;
                    }

                    let rel_path = format!("{}/{}", folder_path, disk_filename);

                    // 1. Mark assemble complete on the single job record
                    if let Err(e) = JobRepo::mark_assemble_done(
                        &pool,
                        job_id,
                        &sha256,
                        &rel_path,
                        &final_destination,
                        file_size_bytes,
                    )
                    .await
                    {
                        error!("Failed updating assemble stage for {}: {}", asset_id, e);
                        continue;
                    }

                    // 2. Mutate in-memory job handle and push directly to Worker 1
                    let mut thumb_job = job;
                    thumb_job.folder_path = folder_path;
                    thumb_job.rel_path = rel_path;
                    thumb_job.disk_path = final_destination;
                    thumb_job.sha256 = sha256;
                    thumb_job.file_size_bytes = file_size_bytes;
                    thumb_job.assemble_done = true;
                    thumb_job.current_stage = "thumbnail".to_string();

                    let _ = thumb_tx.send(thumb_job).await;
                }
            });
        }

        // =========================================================================
        // WORKER 1: Thumbnails & Derivatives (Parallelized Concurrency: 2 - 4)
        // =========================================================================
        // =========================================================================
        // WORKER 1: Thumbnails & Derivatives (2-Tier Sharded Storage)
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
                        let job_id = job.id;
                        let asset_id = job.asset_id;
                        let user_id = job.user_id;

                        // 1. Compute the 2-tier shard path from asset_id (e.g. "a1/b2")
                        let asset_id_clean = asset_id.to_string().replace('-', "");
                        let shard_1 = &asset_id_clean[0..2];
                        let shard_2 = &asset_id_clean[2..4];

                        // Target leaf directory: users/<user_id>/thumbs/<shard_1>/<shard_2>
                        let target_shard_dir = storage_root
                            .join("users")
                            .join(user_id.to_string())
                            .join("thumbs")
                            .join(shard_1)
                            .join(shard_2);

                        // Ensure the nested directories exist before generating files
                        if let Err(e) = tokio::fs::create_dir_all(&target_shard_dir).await {
                            error!("Failed creating shard directory {:?} for {}: {}", target_shard_dir, user_id, e);
                            let _ = JobRepo::mark_failed(&pool, job_id, &e.to_string()).await;
                            drop(permit);
                            return;
                        }

                        let disk_path = job.disk_path.clone();
                        let asset_id_str = asset_id.to_string();
                        let shard_dir_clone = target_shard_dir.clone();

                        // 2. Offload derivative processing to blocking pool
                        let phase1_res = tokio::task::spawn_blocking(move || {
                            MediaEngine::process_derivatives_sync(
                                &disk_path,
                                &asset_id_str,
                                &shard_dir_clone,
                            )
                        })
                        .await;

                        let derivatives = match phase1_res {
                            Ok(Ok(data)) => data,
                            Ok(Err(e)) => {
                                let err_msg = e.to_string();
                                error!("Thumbnail extraction error on {}: {}", asset_id, err_msg);
                                let _ = JobRepo::mark_failed(&pool, job_id, &err_msg).await;
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
                                let _ = JobRepo::mark_failed(&pool, job_id, &err_msg).await;
                                let _ = tx_events.send(WsMediaEvent::AssetFailed {
                                    asset_id,
                                    error: err_msg,
                                });
                                drop(permit);
                                return;
                            }
                        };

                        // 3. Commit base asset (stores derivatives.thumb_path which now contains "a1/b2/...")
                        match Self::commit_base_asset(&pool, &job, derivatives).await {
                            Ok(thumb_path) => {
                                let _ = JobRepo::mark_thumb_done(&pool, job_id, ai_enabled).await;

                                let _ = tx_events.send(WsMediaEvent::AssetReady {
                                    asset_id: job.asset_id,
                                    thumb_path,
                                    folder_path: job.folder_path.clone(),
                                });

                                if !job.folder_path.is_empty() && job.folder_path != "root" {
                                    let _ = tx_events.send(WsMediaEvent::AlbumUpdated {
                                        album_id: None,
                                        folder_path: job.folder_path.clone(),
                                        asset_id: job.asset_id,
                                    });
                                }
                            }
                            Err(e) => {
                                error!("Base asset DB commit failed on {}: {}", asset_id, e);
                                let _ = JobRepo::mark_failed(&pool, job_id, &e).await;
                                let _ = tx_events.send(WsMediaEvent::AssetFailed {
                                    asset_id,
                                    error: e,
                                });
                                drop(permit);
                                return;
                            }
                        };

                        drop(permit);

                        // Stage 2: Hand off to AI Enrichment
                        if ai_enabled {
                            let mut ai_job = job;
                            ai_job.thumb_done = true;
                            ai_job.current_stage = "ai_enrichment".to_string();

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
                    let job_id = job.id;
                    let asset_id = job.asset_id;
                    let user_id = job.user_id;

                    // 1. If AI is globally disabled, skip entirely and leave checkpoints as false
                    if !config_ai.enabled {
                        info!(asset_id = %asset_id, "AI globally disabled: leaving enrichment pending");
                        let _ = sqlx::query(
                            "UPDATE processing_jobs SET status = 'pending', updated_at = CURRENT_TIMESTAMP WHERE id = $1",
                        )
                        .bind(job_id)
                        .execute(&pool)
                        .await;
                        continue;
                    }

                    // 2. Identify remaining work based on config toggles AND boolean job checkpoints
                    let need_faces = config_ai.enable_faces && !job.ai_faces_done;
                    let need_clip = config_ai.enable_clip && !job.ai_clip_done;
                    let need_tags = config_ai.enable_tags && !job.ai_tags_done;
                    let need_poses = config_ai.enable_poses && !job.ai_poses_done;

                    // If everything configured is already done for this asset, finalize and proceed
                    if !need_faces && !need_clip && !need_tags && !need_poses {
                        let _ = JobRepo::mark_completed(&pool, job_id).await;
                        continue;
                    }

                    // PAUSE GATE: Yield tensor inference immediately during active network uploads
                    if *pause_rx.borrow() {
                        let _ = pause_rx.wait_for(|paused| !*paused).await;
                    }

                    let media_engine = match coordinator.ensure_pipeline_engine().await {
                        Ok(e) => e,
                        Err(err) => {
                            let _ = JobRepo::mark_failed(&pool, job_id, &err.to_string()).await;
                            continue;
                        }
                    };

                    let cluster_cache = match coordinator.ensure_cluster_cache().await {
                        Ok(c) => c,
                        Err(err) => {
                            let _ = JobRepo::mark_failed(&pool, job_id, &err.to_string()).await;
                            continue;
                        }
                    };

                    coordinator.keep_warm().await;

                    let user_id_str = user_id.to_string();
                    let known_clusters = if need_faces {
                        let read_guard = cluster_cache.read().await;
                        read_guard.get(&user_id_str).cloned().unwrap_or_default()
                    } else {
                        Vec::new()
                    };

                    let user_thumbs_dir = storage_root.join("users").join(&user_id_str).join("thumbs");
                    let disk_path = job.disk_path.clone();
                    let engine = media_engine.clone();
                    let asset_id_str = asset_id.to_string();

                    let phase2_res = tokio::task::spawn_blocking(move || {
                        engine.process_ai_sync(
                            &disk_path,
                            &asset_id_str,
                            &user_thumbs_dir,
                            known_clusters,
                        )
                    })
                    .await;

                    let mut ai_result = match phase2_res {
                        Ok(Ok(data)) => data,
                        Ok(Err(e)) => {
                            warn!("AI inference skipped/failed on {}: {}", asset_id, e);
                            let _ = JobRepo::mark_failed(&pool, job_id, &e.to_string()).await;
                            continue;
                        }
                        Err(join_err) => {
                            warn!("AI inference panic on {}: {}", asset_id, join_err);
                            let _ = JobRepo::mark_failed(&pool, job_id, &join_err.to_string()).await;
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

                    // Face clustering updates in cache
                    let mut affected_ids: Vec<Uuid> = Vec::new();
                    let new_people_count = ai_result.new_persons.len();

                    if need_faces && (!ai_result.new_persons.is_empty() || !ai_result.updated_clusters.is_empty()) {
                        let mut write_guard = cluster_cache.write().await;
                        let user_bucket = write_guard.entry(user_id_str.clone()).or_default();

                        for np in &ai_result.new_persons {
                            if let Ok(pid) = Uuid::parse_str(&np.person_id) {
                                affected_ids.push(pid);
                            }
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
                            if let Ok(pid) = Uuid::parse_str(&uc.person_id) {
                                affected_ids.push(pid);
                            }
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

                    let detected_faces_count = ai_result.detected_faces.len();
                    let tags_count = ai_result.tags.len();

                    // Persist enriched metadata (PostgreSQL automatically writes to assets.clip_embedding and HNSW index)
                    if let Err(e) = Self::commit_ai_enrichment(&pool, &job, ai_result).await {
                        warn!("Failed persisting AI metadata for {}: {}", job.asset_id, e);
                    }

                    // 4. Update checkpoints in DB atomically using boolean flags
                    let _ = JobRepo::update_ai_checkpoints(
                        &pool,
                        job_id,
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
                            user_id,
                            new_people_count,
                            affected_person_ids: affected_ids,
                        });
                    }

                    let _ = tx_events.send(WsMediaEvent::AiCompleted {
                        asset_id,
                        faces_detected: detected_faces_count,
                        tags_count,
                    });

                    info!(id = %job.asset_id, user_id = %job.user_id, "Granular AI sub-tasks executed and committed");
                }
            });
        }
    }

    async fn commit_base_asset(
        pool: &PgPool,
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

        let captured_at_parsed = derivatives.meta.captured_at.as_deref().and_then(|d| {
            chrono::DateTime::parse_from_rfc3339(d)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M:%S")
                        .map(|ndt| ndt.and_utc())
                })
                .ok()
        });

        let asset = NewAssetRecord {
            id: job.asset_id,
            user_id: job.user_id,
            sha256: job.sha256.clone(),
            file_name: job.file_name.clone(),
            rel_path: job.rel_path.clone(),
            folder_path: job.folder_path.clone(),
            thumb_path: db_thumb_path.clone(),
            preview_path: db_preview_path,
            file_size_bytes: job.file_size_bytes,
            mime_type: derivatives.mime_type,
            width: Some(derivatives.width as i32),
            height: Some(derivatives.height as i32),
            aspect_ratio: Some(derivatives.aspect_ratio as f32),
            duration_seconds: derivatives.duration_seconds.map(|d| d as f32),
            captured_at: captured_at_parsed,
            year: derivatives.meta.year,
            month: derivatives.meta.month,
            day: derivatives.meta.day,
            hour: derivatives.meta.hour,
            latitude: derivatives.meta.latitude,
            longitude: derivatives.meta.longitude,
            altitude: derivatives.meta.altitude.map(|a| a as f32),
            city: derivatives.meta.city,
            subdivision: derivatives.meta.subdivision,
            country: derivatives.meta.country,
            country_code: derivatives.meta.country_code,
            camera_make: derivatives.meta.camera_make,
            camera_model: derivatives.meta.camera_model,
            author,
            source_platform: platform,
            source_url,
            source_post_id,
            caption,
            clip_embedding: None,
        };

        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

        // 1. Insert core asset record into PostgreSQL
        AssetRepo::insert_asset_tx(&mut tx, &asset)
            .await
            .map_err(|e| e.to_string())?;

        // 2. Link asset to folder albums
        if !job.folder_path.is_empty() && job.folder_path != "root" {
            AlbumRepo::link_asset_to_folder_albums_tx(
                &mut tx,
                job.user_id,
                job.asset_id,
                &job.folder_path,
            )
            .await
            .map_err(|e| format!("Failed linking asset to album: {e}"))?;
        }

        // 3. Commit scraped hashtags and author to tags
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
                TagRepo::save_asset_tags_tx(&mut tx, job.user_id, job.asset_id, &initial_tags)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }

        // 4. Mark scraped item downloaded if linked
        if let Some(scraped_id_str) = p.and_then(|x| x.scraped_item_id.as_deref()) {
            if let Ok(scraped_id) = Uuid::parse_str(scraped_id_str) {
                sqlx::query(
                    "UPDATE scraped_media_items SET status = 'downloaded', asset_id = $1 WHERE id = $2",
                )
                .bind(job.asset_id)
                .bind(scraped_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            }
        }

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(db_thumb_path)
    }

    async fn commit_ai_enrichment(
        pool: &PgPool,
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
                let keypoints_json = serde_json::to_value(&pose.keypoints)
                    .unwrap_or_else(|_| serde_json::json!([]));

                db::ingestion_repo::IngestionDetectedPose {
                    id: Uuid::new_v4(),
                    score: pose.score,
                    bbox_x: pose.x,
                    bbox_y: pose.y,
                    bbox_w: pose.w,
                    bbox_h: pose.h,
                    keypoints: keypoints_json,
                }
            })
            .collect();

        let new_persons = res
            .new_persons
            .into_iter()
            .filter_map(|np| {
                let person_id = Uuid::parse_str(&np.person_id).ok()?;
                let cover_face_id = Uuid::parse_str(&np.cover_face_id).ok();
                Some(db::IngestionNewPerson {
                    person_id,
                    cover_face_id,
                    centroid: np.centroid,
                })
            })
            .collect();

        let updated_clusters = res
            .updated_clusters
            .into_iter()
            .filter_map(|uc| {
                let person_id = Uuid::parse_str(&uc.person_id).ok()?;
                Some(db::IngestionUpdatedCluster {
                    person_id,
                    new_centroid: uc.new_centroid,
                    new_face_count: uc.new_face_count,
                })
            })
            .collect();

        let detected_faces = res
            .detected_faces
            .into_iter()
            .filter_map(|face| {
                let face_id = Uuid::parse_str(&face.face_id).ok()?;
                let person_id = Uuid::parse_str(&face.person_id).ok();
                Some(db::IngestionDetectedFace {
                    face_id,
                    person_id,
                    bbox_x: face.bbox_x,
                    bbox_y: face.bbox_y,
                    bbox_w: face.bbox_w,
                    bbox_h: face.bbox_h,
                    score: face.score,
                    face_thumb_db_path: format!("users/{}/thumbs/{}", job.user_id, face.face_thumb_rel_path),
                    embedding: face.embedding,
                })
            })
            .collect();

        let payload = db::AiEnrichmentPayload {
            asset_id: job.asset_id,
            user_id: job.user_id,
            clip_embedding: clip_embedding_bytes,
            new_persons,
            updated_clusters,
            detected_faces,
            poses,
            tags: ai_tags,
        };

        IngestionRepo::commit_ai_metadata(pool, payload)
            .await
            .map_err(|e| e.to_string())
    }
}