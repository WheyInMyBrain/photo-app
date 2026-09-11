use image::DynamicImage;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::db::AssetRepo;
use crate::domain::media::NewAssetRecord;
use crate::services::face_engine::FaceEngine;
use crate::services::face_pipeline::FacePipeline;
use crate::services::image_processor::ImageProcessor;
use crate::services::metadata::{ExtractedMetadata, MetadataService};
use crate::services::tag_engine::TagEngine;
use crate::services::tag_pipeline::TagPipeline;
use crate::services::video_processor::VideoProcessor;

#[derive(Debug)]
pub struct ProcessJob {
    pub asset_id: String,
    pub file_name: String,
    pub rel_path: String,
    pub folder_path: String,
    pub disk_path: PathBuf,
    pub sha256: String,
    pub file_size_bytes: i64,
    pub is_private: bool,
}

struct MediaArtifacts {
    meta: ExtractedMetadata,
    thumb_path: String,
    preview_path: String,
    width: i64,
    height: i64,
    aspect_ratio: f64,
    duration_seconds: Option<f64>,
    mime_type: String,
    image_buffer: Option<Arc<DynamicImage>>,
}

pub struct QueueService;

impl QueueService {
    pub fn start_worker(
        mut receiver: mpsc::Receiver<ProcessJob>,
        pool: SqlitePool,
        thumbs_root: PathBuf,
        face_engine: Arc<FaceEngine>,
        tag_engine: Arc<TagEngine>,
    ) {
        tokio::spawn(async move {
            info!("Background media worker running (FaceEngine + TagEngine)");

            while let Some(job) = receiver.recv().await {
                let pool = pool.clone();
                let thumbs_root = thumbs_root.clone();
                let face_engine = face_engine.clone();
                let tag_engine = tag_engine.clone();

                let shard = if job.asset_id.len() >= 2 { &job.asset_id[0..2] } else { "misc" };
                let shard_dir = thumbs_root.join(shard);

                if let Err(e) = std::fs::create_dir_all(&shard_dir) {
                    error!("Failed to create shard directory {:?}: {}", shard_dir, e);
                    continue;
                }

                let disk_path = job.disk_path.clone();
                let asset_id = job.asset_id.clone();

                // 1. Media decoding and thumbnail generation in blocking thread
                let process_res = tokio::task::spawn_blocking(move || {
                    Self::process_file_sync(&disk_path, &asset_id, &shard_dir)
                })
                .await;

                let artifacts = match process_res {
                    Ok(Ok(data)) => data,
                    Ok(Err(e)) => {
                        error!("Processing error on {}: {}", job.asset_id, e);
                        continue;
                    }
                    Err(join_err) => {
                        error!("Worker task panic on {}: {}", job.asset_id, join_err);
                        continue;
                    }
                };

                // 2. Persist to DB and dispatch AI pipelines
                Self::index_and_dispatch(
                    pool,
                    thumbs_root,
                    face_engine,
                    tag_engine,
                    job,
                    artifacts,
                ).await;
            }
        });
    }

    fn process_file_sync(
        disk_path: &Path,
        asset_id: &str,
        shard_dir: &Path,
    ) -> Result<MediaArtifacts, Box<dyn std::error::Error + Send + Sync>> {
        let ext = disk_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if VideoProcessor::is_video(&ext) {
            let v_meta = VideoProcessor::extract_metadata(disk_path)?;
            let (thumb_path, preview_path) = VideoProcessor::generate_poster(disk_path, asset_id, shard_dir)?;
            let aspect = if v_meta.height > 0 { v_meta.width as f64 / v_meta.height as f64 } else { 1.777 };

            let mut meta = ExtractedMetadata::default();
            meta.captured_at = v_meta.captured_at;
            meta.camera_make = v_meta.camera_make;
            meta.camera_model = v_meta.camera_model;

            if let Some(ref dt) = meta.captured_at {
                let p: Vec<&str> = dt.split(|c| c == '-' || c == 'T' || c == ' ' || c == ':').collect();
                if p.len() >= 4 {
                    meta.year = p[0].parse().ok();
                    meta.month = p[1].parse().ok();
                    meta.day = p[2].parse().ok();
                    meta.hour = p[3].parse().ok();
                }
            }

            let mime = if ext == "mp4" { "video/mp4" } else { "video/quicktime" };

            Ok(MediaArtifacts {
                meta,
                thumb_path,
                preview_path,
                width: v_meta.width,
                height: v_meta.height,
                aspect_ratio: aspect,
                duration_seconds: Some(v_meta.duration_seconds),
                mime_type: mime.to_string(),
                image_buffer: None,
            })
        } else {
            let meta = MetadataService::extract(disk_path);
            let img = ImageProcessor::load_image(disk_path)?;
            let width = img.width() as i64;
            let height = img.height() as i64;
            let aspect = if height > 0 { width as f64 / height as f64 } else { 1.0 };
            let (thumb_path, preview_path) = ImageProcessor::generate_derivatives(&img, asset_id, shard_dir)?;

            let mime = match ext.as_str() {
                "heic" | "heif" => "image/heic",
                "png" => "image/png",
                "webp" => "image/webp",
                _ => "image/jpeg",
            };

            Ok(MediaArtifacts {
                meta,
                thumb_path,
                preview_path,
                width,
                height,
                aspect_ratio: aspect,
                duration_seconds: None,
                mime_type: mime.to_string(),
                image_buffer: Some(Arc::new(img)),
            })
        }
    }

    async fn index_and_dispatch(
        pool: SqlitePool,
        thumbs_root: PathBuf,
        face_engine: Arc<FaceEngine>,
        tag_engine: Arc<TagEngine>,
        job: ProcessJob,
        art: MediaArtifacts,
    ) {
        let asset_id = job.asset_id.clone();

        let record = NewAssetRecord {
            id: job.asset_id,
            sha256: job.sha256,
            file_name: job.file_name,
            rel_path: job.rel_path,
            folder_path: job.folder_path,
            thumb_path: art.thumb_path,
            preview_path: art.preview_path,
            file_size_bytes: job.file_size_bytes,
            mime_type: art.mime_type.clone(),
            width: art.width,
            height: art.height,
            aspect_ratio: art.aspect_ratio,
            duration_seconds: art.duration_seconds,
            is_private: job.is_private,
            captured_at: art.meta.captured_at,
            year: art.meta.year,
            month: art.meta.month,
            day: art.meta.day,
            hour: art.meta.hour,
            latitude: art.meta.latitude,
            longitude: art.meta.longitude,
            altitude: art.meta.altitude,
            city: art.meta.city,
            subdivision: art.meta.subdivision,
            country: art.meta.country,
            country_code: art.meta.country_code,
            camera_make: art.meta.camera_make,
            camera_model: art.meta.camera_model,
        };

        if let Err(e) = AssetRepo::insert_asset(&pool, &record).await {
            error!("Database write failure for {}: {}", asset_id, e);
            return;
        }

        info!(id = %asset_id, mime = %art.mime_type, "Indexed asset successfully");

        // 2. Initial instant search sync using unified AssetRepo
        if let Err(e) = AssetRepo::sync_search_index(&pool, &asset_id).await {
            error!("Initial search sync failed for {}: {}", asset_id, e);
        }

        // 3. Asynchronous AI enrichment for images
        if let Some(img_arc) = art.image_buffer {
            let pool_clone = pool.clone();
            let a_id = asset_id.clone();

            tokio::spawn(async move {
                let f_engine = face_engine.clone();
                let f_pool = pool_clone.clone();
                let f_thumbs = thumbs_root.clone();
                let f_id = a_id.clone();
                let f_img = img_arc.clone();

                let t_engine = tag_engine.clone();
                let t_pool = pool_clone.clone();
                let t_id = a_id.clone();
                let t_img = img_arc.clone();

                let (face_res, tag_res) = tokio::join!(
                    tokio::spawn(async move {
                        FacePipeline::process_asset_faces(&f_engine, &f_pool, &f_thumbs, &f_id, &f_img).await
                    }),
                    tokio::spawn(async move {
                        TagPipeline::process_asset_tags(&t_engine, &t_pool, &t_id, &t_img).await
                    })
                );

                match face_res {
                    Ok(Ok(cnt)) => info!(id = %a_id, faces = cnt, "Faces clustered"),
                    Ok(Err(e)) => error!("Face pipeline failure on {}: {}", a_id, e),
                    Err(join_err) => error!("Face task panicked on {}: {}", a_id, join_err),
                }

                match tag_res {
                    Ok(Ok(cnt)) => info!(id = %a_id, tags = cnt, "WD Tags stored"),
                    Ok(Err(e)) => error!("Tag pipeline failure on {}: {}", a_id, e),
                    Err(join_err) => error!("Tag task panicked on {}: {}", a_id, join_err),
                }

                // 4. Final FTS5 refresh using unified AssetRepo
                if let Err(e) = AssetRepo::sync_search_index(&pool_clone, &a_id).await {
                    error!("Final FTS5 search sync failed for {}: {}", a_id, e);
                } else {
                    info!(id = %a_id, "FTS5 search index refreshed with tags and faces");
                }
            });
        }
    }
}