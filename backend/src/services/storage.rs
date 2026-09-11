use std::path::{Path, PathBuf};

pub struct StorageService;

impl StorageService {
    /// Cleans and normalizes folder paths (removes leading/trailing slashes, prevents `..`)
    pub fn sanitize_folder_path(folder: &str) -> String {
        let trimmed = folder.trim().trim_matches('/');
        if trimmed.is_empty() {
            return "root".to_string();
        }

        let parts: Vec<&str> = trimmed
            .split('/')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty() && *p != "..")
            .collect();

        if parts.is_empty() {
            "root".to_string()
        } else {
            parts.join("/")
        }
    }

    /// Resolves the absolute directory on disk where the original file should be written
    pub fn resolve_upload_dir(base_storage: &Path, is_private: bool, folder_path: &str) -> PathBuf {
        let access_tier = if is_private { "private" } else { "public" };
        base_storage
            .join("originals")
            .join(access_tier)
            .join(folder_path)
    }
}