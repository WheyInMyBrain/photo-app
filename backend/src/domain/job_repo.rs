use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DbJob {
    pub id: String,
    pub asset_id: String,
    pub file_name: String,
    pub rel_path: String,
    pub folder_path: String,
    pub disk_path: PathBuf,
    pub sha256: String,
    pub file_size_bytes: i64,
    pub is_private: bool,
}