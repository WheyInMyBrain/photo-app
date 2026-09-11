use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub storage_root: PathBuf,
    pub db_url: String,
    pub server_port: u16,
}

impl Config {
    pub fn init() -> Self {
        // Resolve storage relative to backend directory cleanly
        let manifest_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = if manifest_dir.ends_with("backend") {
            manifest_dir.parent().unwrap().join("storage")
        } else {
            manifest_dir.join("storage")
        };

        let db_path = root.join("db/app.db");

        Self {
            storage_root: root,
            db_url: format!("sqlite://{}?mode=rwc", db_path.to_string_lossy()),
            server_port: 3000,
        }
    }
}