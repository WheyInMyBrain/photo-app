// photo-app/crates/db/src/domain/job_repo.rs

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Scraped / social metadata payload passed along the job queue
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobPayload {
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_post_id: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub scraped_item_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbJob {
    pub id: String,
    pub user_id: String,
    pub asset_id: String,
    pub file_name: String,
    pub rel_path: String,
    pub folder_path: String,
    pub disk_path: PathBuf,
    pub sha256: String,
    pub file_size_bytes: i64,
    pub job_type: String,
    /// Contextual metadata carried along the pipeline
    pub payload: Option<JobPayload>,
}