// photo-app/crates/db/src/domain/job_repo.rs

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobPayload {
    pub author: Option<String>,
    pub platform: Option<String>,
    pub source_url: Option<String>,
    pub source_post_id: Option<String>,
    pub caption: Option<String>,
    pub tags: Vec<String>,
    pub scraped_item_id: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct DbJob {
    pub id: Uuid,
    pub user_id: Uuid,
    pub asset_id: Uuid,
    pub file_name: String,
    pub rel_path: String,
    pub folder_path: String,
    pub disk_path: PathBuf,
    pub sha256: String,
    pub file_size_bytes: i64,
    pub status: String,
    pub current_stage: String,
    pub payload: Option<JobPayload>,

    pub assemble_done: bool,
    pub thumb_done: bool,
    pub ai_faces_done: bool,
    pub ai_clip_done: bool,
    pub ai_tags_done: bool,
    pub ai_poses_done: bool,
}