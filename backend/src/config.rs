use media_downloader::DownloaderConfig;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct B2Config {
    pub key_id: String,
    pub application_key: String,
    pub endpoint: String,
    pub bucket_name: String,
    pub backup_interval_hours: u64,
    pub encryption_key: Option<[u8; 32]>,
}

impl B2Config {
    pub fn is_configured(&self) -> bool {
        !self.key_id.is_empty()
            && !self.application_key.is_empty()
            && !self.endpoint.is_empty()
            && !self.bucket_name.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub server_host: String,
    pub server_port: u16,
    pub storage_root: PathBuf,
    pub db_url: String,
    pub vault_api_key: String,
    pub worker_concurrency: usize,
    pub allow_registration: bool,
    pub b2: B2Config,
    pub downloader: DownloaderConfig,
}

impl Config {
    pub fn init() -> Self {
        // Load .env at the very start of configuration initialization
        dotenvy::dotenv().ok();

        // 1. Host and Port
        let server_host = std::env::var("SERVER_HOST")
            .unwrap_or_else(|_| "0.0.0.0".to_string());

        let server_port = std::env::var("SERVER_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(3000);

        // 2. Storage Root
        let storage_root = if let Ok(custom_storage) = std::env::var("STORAGE_ROOT") {
            PathBuf::from(custom_storage)
        } else {
            let manifest_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if manifest_dir.ends_with("backend") {
                manifest_dir.parent().unwrap().join("storage")
            } else {
                manifest_dir.join("storage")
            }
        };

        // 3. Database URL
        let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            let db_path = storage_root.join("db/app.db");
            format!("sqlite://{}?mode=rwc", db_path.to_string_lossy())
        });

        // 4. API Key
        let vault_api_key = std::env::var("VAULT_API_KEY").unwrap_or_default();

        // 5. Workers concurrency
        let worker_concurrency = std::env::var("WORKER_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(5);

        // 6. Registration toggle (defaults to true if unset)
        let allow_registration = std::env::var("ALLOW_REGISTRATION")
            .map(|v| v.trim().eq_ignore_ascii_case("true") || v.trim() == "1")
            .unwrap_or(true);

        // 7. Backblaze B2 Config
        let b2 = B2Config {
            key_id: std::env::var("B2_KEY_ID").unwrap_or_default(),
            application_key: std::env::var("B2_APPLICATION_KEY").unwrap_or_default(),
            endpoint: std::env::var("B2_ENDPOINT").unwrap_or_default(),
            bucket_name: std::env::var("B2_BUCKET_NAME").unwrap_or_default(),
            backup_interval_hours: std::env::var("BACKUP_INTERVAL_HOURS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(168), // default: 7 days
            encryption_key: std::env::var("BACKUP_PASSPHRASE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|pass| {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(pass.trim().as_bytes());
                let result = hasher.finalize();
                let mut key = [0u8; 32];
                key.copy_from_slice(&result);
                key
            }),
        };

        // 8. Downloader config
        let downloader = DownloaderConfig {
            chrome_ws_url: std::env::var("CHROME_WS_URL").ok().filter(|s| !s.trim().is_empty()),
            ig_cookie: std::env::var("IG_COOKIE").ok().filter(|s| !s.trim().is_empty()),
            ig_csrf_token: std::env::var("IG_CSRF_TOKEN").ok().filter(|s| !s.trim().is_empty()),
        };

        Self {
            server_host,
            server_port,
            storage_root,
            db_url,
            vault_api_key,
            worker_concurrency,
            allow_registration,
            b2,
            downloader,
        }
    }
}