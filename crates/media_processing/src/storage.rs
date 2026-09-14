use std::path::{Path, PathBuf};

pub struct StorageService;

impl StorageService {
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

    /// Resolves disk target: storage_root/users/<user_id>/originals/[folder]
    pub fn resolve_upload_dir(
        storage_root: &Path,
        user_id: &str,
        sanitized_folder: &str,
    ) -> PathBuf {
        let mut dir = storage_root
            .join("users")
            .join(user_id)
            .join("originals");

        if !sanitized_folder.is_empty() && sanitized_folder != "root" {
            dir = dir.join(sanitized_folder);
        }
        dir
    }

    pub fn generate_disk_filename(asset_id: &str, original_filename: &str) -> String {
        let extension = Path::new(original_filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("raw")
            .to_lowercase();

        format!("{}.{}", asset_id, extension)
    }
}