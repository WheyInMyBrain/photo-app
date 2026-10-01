// photo-app/backend/src/services/queue.rs

use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Notify, Semaphore};
use tracing::{error, info, warn};

use db::domain::{DbJob, NewAssetRecord};
use db::tag_repo::{IngestionTagInput, TagRepo};
use db::{AlbumRepo, AssetRepo, IngestionRepo, JobRepo};

use crate::services::engine_coordinator::EngineCoordinator;
use crate::AppState;
use crate::WsMediaEvent;
use media_processing::{AiEnrichmentResult, DerivativeResult, MediaEngine, StorageService};

pub type ProcessJob = DbJob;

pub struct QueueService;

impl QueueService {
    pub async fn enqueue(
        pool: &SqlitePool,
        notify: &Arc<Notify>,
        mut job: ProcessJob,
    ) -> Result<(), sqlx::Error> {
        if job.job_type.trim().is_empty() {
            job.job_type = "assemble".to_string();
        }
        JobRepo::enqueue(pool, &job).await?;
        notify.notify_one();
        Ok(())
    }

    pub fn start_worker(
        state: AppState,
        storage_root: PathBuf,
        coordinator: EngineCoordinator,
        concurrency: usize,
        notify: Arc<Notify>,
        tx_events: broadcast::Sender<WsMediaEvent>,
    ) {
        let pool = state.db.clone();
        let thumb_semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
        let ai_semaphore = Arc::new(Semaphore::new(1));

        info!(
            thumb_concurrency = concurrency,
            ai_concurrency = 1,
            "Three-stage prioritized media queue workers active (Assemble -> Thumbnail -> Background AI)"
        );

        // =========================================================================
        // WORKER 0: High-Priority Background Ingest (Zero Double-Copy Direct Stitch)
        // =========================================================================
        {
            let pool = pool.clone();
            let storage_root = storage_root.clone();
            let notify = notify.clone();

            tokio::spawn(async move {
                let _ = JobRepo::reset_interrupted(&pool).await;

                loop {
                    match JobRepo::fetch_next_job(&pool, "assemble").await {
                        Ok(Some(job)) => {
                            let pool = pool.clone();
                            let storage_root = storage_root.clone();
                            let notify = notify.clone();

                            tokio::task::spawn(async move {
                                let job_id = job.id.clone();
                                let input_path = job.disk_path.clone();
                                let asset_id = job.asset_id.clone();
                                let user_id = job.user_id.clone();
                                let file_name = job.file_name.clone();
                                let folder_path = job.folder_path.clone();

                                // Resolve target originals directory upfront
                                let target_dir = StorageService::resolve_upload_dir(
                                    &storage_root,
                                    &user_id,
                                    &folder_path,
                                );

                                if let Err(e) = tokio::fs::create_dir_all(&target_dir).await {
                                    error!("Failed creating storage directory for {}: {}", asset_id, e);
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &e.to_string()).await;
                                    return;
                                }

                                let disk_filename = StorageService::generate_disk_filename(&asset_id, &file_name);
                                let final_destination = target_dir.join(&disk_filename);
                                let destination_tmp = target_dir.join(format!("{}.tmp", disk_filename));

                                let dest_tmp_clone = destination_tmp.clone();

                                // Heavy disk I/O & SHA-256 calculation offloaded to blocking thread
                                let res = tokio::task::spawn_blocking(move || -> Result<(String, i64), String> {
                                    use std::io::{Read, Write};
                                    use sha2::{Digest, Sha256};

                                    let mut hasher = Sha256::new();
                                    let mut buffer = [0u8; 128 * 1024]; // 128KB stream buffer
                                    let mut total_bytes = 0i64;

                                    if input_path.is_dir() {
                                        // Case A: Chunked upload directory
                                        // Write directly into destination_tmp on the destination filesystem!
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
                                        drop(assembled);
                                    } else {
                                        // Case B: Staged temporary file from direct / multipart upload
                                        // Read & hash the staged file
                                        let mut f = std::fs::File::open(&input_path)
                                            .map_err(|e| format!("Failed opening staged file: {e}"))?;

                                        loop {
                                            let n = f.read(&mut buffer).map_err(|e| e.to_string())?;
                                            if n == 0 { break; }
                                            hasher.update(&buffer[..n]);
                                            total_bytes += n as i64;
                                        }
                                        drop(f);

                                        // Move staged file directly to destination_tmp
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
                                        error!("Assemble processing error on {}: {}", asset_id, e);
                                        let _ = tokio::fs::remove_file(&destination_tmp).await;
                                        let _ = JobRepo::mark_failed(&pool, &job_id, &e).await;
                                        return;
                                    }
                                    Err(join_err) => {
                                        error!("Assemble thread panic on {}: {}", asset_id, join_err);
                                        let _ = tokio::fs::remove_file(&destination_tmp).await;
                                        let _ = JobRepo::mark_failed(&pool, &job_id, &join_err.to_string()).await;
                                        return;
                                    }
                                };

                                // Deduplication check in SQLite
                                if let Ok(Some(existing_id)) = AssetRepo::find_user_asset_by_sha256(&pool, &user_id, &sha256).await {
                                    // Remove temporary destination file
                                    let _ = tokio::fs::remove_file(&destination_tmp).await;

                                    // Clean chunk session directory if present
                                    if job.disk_path.is_dir() {
                                        let _ = tokio::fs::remove_dir_all(&job.disk_path).await;
                                    }

                                    // If destination folder/album requested, link existing asset
                                    if !folder_path.is_empty() && folder_path != "root" {
                                        if let Ok(mut tx) = pool.begin().await {
                                            let _ = AlbumRepo::link_asset_to_folder_albums_tx(
                                                &mut tx,
                                                &user_id,
                                                &existing_id,
                                                &folder_path,
                                            ).await;
                                            let _ = tx.commit().await;
                                        }
                                    }

                                    let _ = JobRepo::mark_completed(&pool, &job_id).await;
                                    info!(asset_id = %existing_id, "Duplicate resolved and linked to folder");
                                    return;
                                }

                                // Instant atomic in-place rename on the same directory / filesystem
                                if let Err(e) = tokio::fs::rename(&destination_tmp, &final_destination).await {
                                    error!("Failed final rename for {}: {}", asset_id, e);
                                    let _ = tokio::fs::remove_file(&destination_tmp).await;
                                    let _ = JobRepo::mark_failed(&pool, &job_id, &e.to_string()).await;
                                    return;
                                }

                                // Clean chunk directory
                                if job.disk_path.is_dir() {
                                    let _ = tokio::fs::remove_dir_all(&job.disk_path).await;
                                }

                                let rel_path = if folder_path.is_empty() || folder_path == "root" {
                                    disk_filename
                                } else {
                                    format!("{}/{}", folder_path, disk_filename)
                                };

                                // Mark Stage 0 complete
                                let _ = JobRepo::mark_completed(&pool, &job_id).await;

                                // Enqueue Stage 1: "thumbnail"
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
                                };

                                if let Err(e) = JobRepo::enqueue(&pool, &thumb_job).await {
                                    error!("Failed scheduling thumbnail job: {}", e);
                                } else {
                                    notify.notify_one();
                                }
                            });
                        }
                        Ok(None) => {
                            tokio::select! {
                                _ = notify.notified() => {},
                                _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Failed polling assemble jobs");
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
                    }
                }
            });
        }

        // =========================================================================
        // WORKER 1: Interactive Thumbnail & Preview Generation
        // =========================================================================
        {
            let pool = pool.clone();
            let storage_root = storage_root.clone();
            let notify = notify.clone();
            let tx_events = tx_events.clone();
            let state_tracker = state.clone();

            tokio::spawn(async move {
                loop {
                    // YIELD TO ACTIVE UPLOADS: Allow network streaming priority
                    if state_tracker.seconds_since_last_upload() < 5 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }

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

                                // Insert base asset record, social tags, and link staging
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

                                // Enqueue background AI enrichment job (Tier 2)
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
                                _ = tokio::time::sleep(Duration::from_secs(5)) => {},
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
        // WORKER 2: Low-Priority Background AI Enrichment (Faces, Tags, CLIP)
        // =========================================================================
        {
            let pool = pool.clone();
            let storage_root = storage_root.clone();
            let notify = notify.clone();
            let coordinator = coordinator.clone();
            let state_tracker = state.clone();

            tokio::spawn(async move {
                loop {
                    // IDLE GUARD: AI jobs run only after a quiet window (>= 30 seconds since last upload activity)
                    if state_tracker.seconds_since_last_upload() < 30 {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }

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

                    // Strict concurrency 1 to keep host memory and compute cool
                    let _permit = match ai_semaphore.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => break,
                    };

                    let job_id = job.id.clone();
                    let asset_id = job.asset_id.clone();
                    let user_id = job.user_id.clone();

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

                    // Update in-memory face cluster cache
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
                                    exemplars: vec![emb_arr],
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

                    // Update SIMD CLIP cache
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

                    // Persist AI enrichments
                    if let Err(e) = Self::commit_ai_enrichment(&pool, &job, ai_result).await {
                        warn!("Failed persisting AI metadata for {}: {}", job.asset_id, e);
                    }

                    let _ = JobRepo::mark_completed(&pool, &job_id).await;
                    coordinator.keep_warm().await;
                    info!(id = %job.asset_id, user_id = %job.user_id, "Asset fully indexed with AI");
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

        AlbumRepo::link_asset_to_folder_albums_tx(
            &mut *tx,
            &job.user_id,
            &job.asset_id,
            &job.folder_path,
        )
        .await
        .map_err(|e| format!("Failed linking asset to album: {e}"))?;

        // 2. Commit scraped hashtags and author to tags and FTS5 index immediately
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

            // FTS5 safe update: Delete existing row first, then re-insert
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

        // 3. Mark scraped item downloaded if linked
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

        // Map YOLO detected human poses & keypoints
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
                    bbox_w: face.bbox_w,
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