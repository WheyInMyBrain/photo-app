use media_downloader::DownloaderConfig;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub server_host: String,
    pub server_port: u16,
    pub storage_root: PathBuf,
    pub db_url: String,
    pub vault_api_key: String,
    pub worker_concurrency: usize,
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

        // 6. Downloader config
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
            downloader,
        }
    }
}