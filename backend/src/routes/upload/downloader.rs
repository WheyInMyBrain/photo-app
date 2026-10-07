// photo-app/backend/src/routes/upload/downloader.rs

use chrono::Utc;
use media_downloader::{
    download_media, extract_media, inject_metadata,
    websites::instagram::extract_username_from_url,
    ExtractedMediaMetadata, MediaMetadataPayload, MediaType,
};
use media_processing::StorageService;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::AppState;

use super::utils::{build_suggested_filename, ext_from_mime};
use db::domain::{
    BatchUploadReceipt, DbJob, JobPayload, UploadItemResult, CandidateItem,
    CandidateManifest,
};
use db::scrapes_repo::{ScrapedMediaItemRecord, ScrapedVariantRecord, ScrapesRepo};
use db::JobRepo;

pub async fn resolve_or_scrape_manifest(
    state: &AppState,
    auth_user: &AuthUser,
    url: &str,
) -> Result<CandidateManifest, AppError> {
    let clean_url = url.trim();

    let lower_url = clean_url.to_lowercase();
    let is_instagram = lower_url.contains("instagram.com") || lower_url.contains("instagr.am");
    let is_ig_profile = is_instagram
        && !lower_url.contains("/p/")
        && !lower_url.contains("/reel/")
        && !lower_url.contains("/reels/")
        && !lower_url.contains("/stories/");

    // Check PostgreSQL cache for non-profile single links
    if !is_ig_profile {
        if let Some(post) = ScrapesRepo::find_post_by_url(&state.db, auth_user.id, clean_url)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
        {
            let items = ScrapesRepo::fetch_items_for_post(&state.db, post.id)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            if !items.is_empty() {
                let candidate_items = items
                    .into_iter()
                    .map(|i| CandidateItem {
                        id: i.id.to_string(),
                        media_type: i.media_type,
                        mime_type: "application/octet-stream".to_string(),
                        thumbnail_url: i.thumbnail_url.unwrap_or_default(),
                        thumbnail_base64: None,
                        high_res_url: i.cdn_url,
                        audio_url: i.audio_url,
                        suggested_filename: i.suggested_filename,
                        referer: Some(clean_url.to_string()),
                        caption: i.caption.or_else(|| post.caption.clone()),
                        published_at: i.published_at.map(|d| d.to_rfc3339()).or_else(|| post.published_at.map(|d| d.to_rfc3339())),
                        location_name: i.location_name.or_else(|| post.location_name.clone()),
                        latitude: i.latitude.or(post.latitude),
                        longitude: i.longitude.or(post.longitude),
                        tags: if !i.tags.is_empty() { i.tags } else { post.tags.clone() },
                        source_post_url: i.source_url.or_else(|| Some(clean_url.to_string())),
                    })
                    .collect();

                return Ok(CandidateManifest {
                    platform: post.platform.clone(),
                    author: post.author.clone(),
                    caption: post.caption.unwrap_or_default(),
                    suggested_folder: format!("{}/{}", post.platform, post.author),
                    items: candidate_items,
                });
            }
        }
    }

    // Preload known IDs for Instagram profiles
    let mut downloader_config = state.config.downloader.clone();
    if is_ig_profile {
        if let Some(username) = extract_username_from_url(clean_url) {
            let existing_ids = ScrapesRepo::get_existing_post_ids(
                &state.db,
                auth_user.id,
                "instagram",
                &username,
            )
            .await
            .unwrap_or_default();

            info!(
                username = %username,
                count = existing_ids.len(),
                "Pre-loaded existing Instagram post IDs for early-exit profile sync"
            );

            downloader_config.known_post_ids = Some(existing_ids);
        }
    }

    let extracted: ExtractedMediaMetadata = extract_media(clean_url, Some(&downloader_config))
        .await
        .map_err(|e| AppError::BadRequest(format!("Link extraction failed: {e}")))?;

    let post_uuid = Uuid::new_v4();
    let total_items = extracted.items.len();

    let mut post_item_totals: HashMap<String, usize> = HashMap::new();
    for item in &extracted.items {
        let post_key = item
            .source_post_url
            .clone()
            .unwrap_or_else(|| clean_url.to_string());
        *post_item_totals.entry(post_key).or_insert(0) += 1;
    }

    let mut post_item_indices: HashMap<String, usize> = HashMap::new();
    let mut db_items: Vec<ScrapedMediaItemRecord> = Vec::with_capacity(total_items);
    let mut candidate_items: Vec<CandidateItem> = Vec::with_capacity(total_items);

    for item in extracted.items.iter() {
        let media_type_str = match item.media_type {
            MediaType::Video => "video",
            MediaType::Image => {
                if item.mime_type == "image/gif" {
                    "gif"
                } else {
                    "image"
                }
            }
        };

        let post_key = item
            .source_post_url
            .clone()
            .unwrap_or_else(|| clean_url.to_string());

        let this_post_total = *post_item_totals.get(&post_key).unwrap_or(&1);
        let this_post_index = post_item_indices.entry(post_key).or_insert(0);
        let current_index_for_this_post = *this_post_index;
        *this_post_index += 1;

        let caption_for_filename = item
            .caption
            .as_deref()
            .filter(|c| !c.is_empty())
            .unwrap_or(&extracted.caption);

        let ext = ext_from_mime(&item.mime_type, &item.media_type);

        let suggested_filename = build_suggested_filename(
            caption_for_filename,
            current_index_for_this_post,
            this_post_total,
            ext,
        );

        let item_id = Uuid::new_v4();

        let (w, h) = item
            .dimensions
            .as_ref()
            .map(|d| (Some(d.width as i32), Some(d.height as i32)))
            .unwrap_or((None, None));

        let variants = item
            .variants
            .iter()
            .map(|v| {
                let (vw, vh) = v
                    .dimensions
                    .as_ref()
                    .map(|d| (Some(d.width as i32), Some(d.height as i32)))
                    .unwrap_or((None, None));

                ScrapedVariantRecord {
                    url: v.url.clone(),
                    width: vw,
                    height: vh,
                    label: v.label.clone(),
                    file_size_bytes: v.file_size_bytes.map(|s| s as i64),
                    is_master: v.url == item.high_res_url,
                }
            })
            .collect();

        let item_caption = item.caption.clone().or_else(|| {
            if !extracted.caption.is_empty() {
                Some(extracted.caption.clone())
            } else {
                None
            }
        });
        
        let item_published_at_str = item.published_at.clone().or_else(|| extracted.published_at.clone());
        let item_published_at = item_published_at_str.as_deref().and_then(|p| {
            chrono::DateTime::parse_from_rfc3339(p).ok().map(|dt| dt.with_timezone(&Utc))
        });

        let item_location_name = item.location.as_ref().map(|l| l.name.clone())
            .or_else(|| extracted.location.as_ref().map(|l| l.name.clone()));
        let item_lat = item.location.as_ref().and_then(|l| l.latitude)
            .or_else(|| extracted.location.as_ref().and_then(|l| l.latitude));
        let item_lng = item.location.as_ref().and_then(|l| l.longitude)
            .or_else(|| extracted.location.as_ref().and_then(|l| l.longitude));
        let item_tags = if !item.tags.is_empty() {
            item.tags.clone()
        } else {
            extracted.tags.clone()
        };
        let item_source_url = item.source_post_url.clone().or_else(|| Some(clean_url.to_string()));

        db_items.push(ScrapedMediaItemRecord {
            id: item_id,
            media_type: media_type_str.to_string(),
            cdn_url: item.high_res_url.clone(),
            audio_url: item.audio_url.clone(),
            thumbnail_url: item.thumbnail_url.clone(),
            suggested_filename: suggested_filename.clone(),
            width: w,
            height: h,
            variants,
            caption: item_caption.clone(),
            published_at: item_published_at,
            location_name: item_location_name.clone(),
            latitude: item_lat,
            longitude: item_lng,
            tags: item_tags.clone(),
            source_url: item_source_url.clone(),
        });

        candidate_items.push(CandidateItem {
            id: item_id.to_string(),
            media_type: media_type_str.to_string(),
            mime_type: item.mime_type.clone(),
            thumbnail_url: item.thumbnail_url.clone().unwrap_or_default(),
            thumbnail_base64: None,
            high_res_url: item.high_res_url.clone(),
            audio_url: item.audio_url.clone(),
            suggested_filename,
            referer: item
                .referer_required
                .clone()
                .or_else(|| item_source_url.clone()),
            caption: item_caption,
            published_at: item_published_at_str,
            location_name: item_location_name,
            latitude: item_lat,
            longitude: item_lng,
            tags: item_tags,
            source_post_url: item_source_url,
        });
    }

    let location_name = extracted.location.as_ref().map(|l| l.name.as_str());
    let latitude = extracted.location.as_ref().and_then(|l| l.latitude);
    let longitude = extracted.location.as_ref().and_then(|l| l.longitude);
    let post_published_at = extracted.published_at.as_deref().and_then(|p| {
        chrono::DateTime::parse_from_rfc3339(p).ok().map(|dt| dt.with_timezone(&Utc))
    });

    ScrapesRepo::save_scraped_post_and_items(
        &state.db,
        post_uuid,
        auth_user.id,
        &extracted.platform,
        clean_url,
        clean_url,
        &extracted.author,
        Some(&extracted.caption),
        &extracted.tags,
        location_name,
        latitude,
        longitude,
        post_published_at,
        &extracted.discovered_post_urls,
        extracted.next_page_url.as_deref(),
        &db_items,
    )
    .await
    .map_err(|e| AppError::Internal(format!("Failed saving scrape: {e}")))?;

    let suggested_folder = format!("{}/{}", extracted.platform, extracted.author);

    Ok(CandidateManifest {
        platform: extracted.platform,
        author: extracted.author,
        caption: extracted.caption,
        suggested_folder,
        items: candidate_items,
    })
}

pub async fn execute_item_downloads(
    state: &AppState,
    auth_user: &AuthUser,
    items: Vec<CandidateItem>,
    target_folder: &str,
    platform: &str,
) -> Result<BatchUploadReceipt, AppError> {
    if items.is_empty() {
        return Err(AppError::BadRequest("No items selected for download".to_string()));
    }

    let sanitized_folder = StorageService::sanitize_folder_path(target_folder);
    let temp_download_dir = state.config.storage_root.join("temp").join("downloads");
    tokio::fs::create_dir_all(&temp_download_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed creating temp dir: {e}")))?;

    let known_downloaded_posts: HashSet<String> = if platform == "instagram" {
        let downloaded_from_scrapes: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT sp.external_post_id 
            FROM scraped_media_items smi
            JOIN scraped_posts sp ON smi.scraped_post_id = sp.id
            WHERE sp.user_id = $1 
              AND sp.platform = 'instagram'
              AND smi.status = 'downloaded'
            "#
        )
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let downloaded_from_assets: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT source_post_id 
            FROM assets 
            WHERE user_id = $1 
              AND source_platform = 'instagram' 
              AND deleted_at IS NULL 
              AND source_post_id IS NOT NULL
            "#
        )
        .bind(auth_user.id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        downloaded_from_scrapes
            .into_iter()
            .chain(downloaded_from_assets)
            .collect()
    } else {
        HashSet::new()
    };

    info!(
        total_candidates = items.len(),
        already_downloaded = known_downloaded_posts.len(),
        platform = %platform,
        "Starting download execution queue"
    );

    let download_sem = Arc::new(Semaphore::new(4));
    let mut tasks = Vec::with_capacity(items.len());

    for item in items {
        if let Some(ref post_url) = item.source_post_url {
            let code_opt = post_url
                .split("/p/")
                .nth(1)
                .or_else(|| post_url.split("/reel/").nth(1))
                .or_else(|| post_url.split("/reels/").nth(1))
                .and_then(|s| s.split('/').next());

            if let Some(code) = code_opt {
                if known_downloaded_posts.contains(code) {
                    info!(shortcode = %code, "Skipping already downloaded post");
                    continue;
                }
            }
        }

        let permit = download_sem.clone();
        let state = state.clone();
        let auth_user = auth_user.clone();
        let sanitized_folder = sanitized_folder.clone();
        let temp_download_dir = temp_download_dir.clone();
        let platform_str = platform.to_string();

        tasks.push(tokio::spawn(async move {
            let _guard = permit.acquire().await.ok();

            let temp_file_path = temp_download_dir.join(format!("{}_{}", Uuid::new_v4(), item.suggested_filename));

            let referer = item.referer.as_deref().or_else(|| match platform_str.as_str() {
                "instagram" => Some("https://www.instagram.com/"),
                "reddit" => Some("https://www.reddit.com/"),
                _ => None,
            });

            if let Err(e) = download_media(
                &item.high_res_url,
                item.audio_url.as_deref(),
                &temp_file_path,
                referer,
            )
            .await
            {
                let _ = tokio::fs::remove_file(&temp_file_path).await;
                return UploadItemResult {
                    file_name: item.suggested_filename,
                    status: "error".to_string(),
                    id: None,
                    relative_path: None,
                    message: Some(format!("Download failed: {e}")),
                };
            }

            // Fetch DB context with native Uuid parsing
            let scraped_item_uuid = Uuid::parse_str(&item.id).ok();
            let ctx = match scraped_item_uuid {
                Some(uid) => ScrapesRepo::get_item_context(&state.db, uid).await.unwrap_or(None),
                None => None,
            };

            let platform_val = ctx.as_ref().map(|c| c.platform.clone()).or_else(|| Some(platform_str));
            let author = ctx.as_ref().map(|c| c.author.clone());

            let caption = item.caption.clone().or_else(|| ctx.as_ref().and_then(|c| c.caption.clone()));
            let published_at = item.published_at.clone().or_else(|| ctx.as_ref().and_then(|c| c.published_at.map(|d| d.to_rfc3339())));
            let location_name = item.location_name.clone().or_else(|| ctx.as_ref().and_then(|c| c.location_name.clone()));
            let latitude = item.latitude.or_else(|| ctx.as_ref().and_then(|c| c.latitude));
            let longitude = item.longitude.or_else(|| ctx.as_ref().and_then(|c| c.longitude));

            let tags = if !item.tags.is_empty() {
                item.tags.clone()
            } else {
                ctx.as_ref().map(|c| c.tags.clone()).unwrap_or_default()
            };

            let source_url = item.source_post_url.clone().or_else(|| {
                ctx.as_ref().map(|c| c.source_url.clone()).or_else(|| item.referer.clone())
            });

            let source_post_id = source_url.as_ref().and_then(|u| {
                u.split("/p/")
                    .nth(1)
                    .or_else(|| u.split("/reel/").nth(1))
                    .or_else(|| u.split("/reels/").nth(1))
                    .and_then(|s| s.split('/').next())
                    .map(|s| s.to_string())
            });

            let meta_payload = MediaMetadataPayload {
                author: author.as_deref(),
                caption: caption.as_deref(),
                source_url: source_url.as_deref(),
                tags: &tags,
                published_at: published_at.as_deref(),
                location_name: location_name.as_deref(),
                latitude,
                longitude,
            };

            let final_disk_path = match inject_metadata(&temp_file_path, &meta_payload).await {
                Ok(new_path) => new_path,
                Err(err) => {
                    warn!(file = %item.suggested_filename, error = %err, "Metadata injection skipped");
                    temp_file_path.clone()
                }
            };

            let actual_filename = final_disk_path
                .file_name()
                .and_then(|f| f.to_str())
                .map(|f| {
                    if let Some((_, orig)) = f.split_once('_') {
                        orig.to_string()
                    } else {
                        item.suggested_filename.clone()
                    }
                })
                .unwrap_or(item.suggested_filename.clone());

            let asset_id = Uuid::new_v4();
            let file_size_bytes = match tokio::fs::metadata(&final_disk_path).await {
                Ok(m) => m.len() as i64,
                Err(_) => 0,
            };

            let job_payload = JobPayload {
                author,
                platform: platform_val,
                source_url,
                source_post_id,
                caption,
                tags,
                scraped_item_id: Some(item.id.clone()),
                latitude,
                longitude,
            };

            let job = DbJob {
                id: Uuid::new_v4(),
                user_id: auth_user.id,
                asset_id,
                file_name: actual_filename,
                rel_path: String::new(),
                folder_path: sanitized_folder.clone(),
                disk_path: final_disk_path,
                sha256: String::new(),
                file_size_bytes,
                status: "pending".to_string(),
                current_stage: "assemble".to_string(),
                payload: Some(job_payload),
                assemble_done: false,
                thumb_done: false,
                ai_faces_done: false,
                ai_clip_done: false,
                ai_tags_done: false,
                ai_poses_done: false,
            };

            if let Err(e) = JobRepo::enqueue_with_status(&state.db, &job, "pending", None).await {
                let _ = tokio::fs::remove_file(&job.disk_path).await;
                UploadItemResult {
                    file_name: item.suggested_filename,
                    status: "error".to_string(),
                    id: None,
                    relative_path: None,
                    message: Some(format!("Failed to enqueue assemble job: {e}")),
                }
            } else {
                let _ = state.channels.assemble_tx.send(job).await;

                UploadItemResult {
                    file_name: item.suggested_filename,
                    status: "queued".to_string(),
                    id: Some(asset_id.to_string()),
                    relative_path: None,
                    message: Some("Queued for assemble & derivative processing".to_string()),
                }
            }
        }));
    }

    let mut results = Vec::with_capacity(tasks.len());
    for task in tasks {
        if let Ok(res) = task.await {
            results.push(res);
        }
    }

    let success_count = results.iter().filter(|r| r.status == "queued").count();
    info!(
        total_attempted = results.len(),
        queued_for_processing = success_count,
        folder = %sanitized_folder,
        "Batch download dispatch complete"
    );

    Ok(BatchUploadReceipt {
        total_uploaded: success_count,
        folder: sanitized_folder,
        items: results,
    })
}