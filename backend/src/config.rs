use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub server_host: String,
    pub server_port: u16,
    pub storage_root: PathBuf,
    pub db_url: String,
    pub vault_api_key: String,
    pub worker_concurrency: usize,
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

        // 2. Storage Root (Reads STORAGE_ROOT if set, otherwise resolves dynamically)
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

        // 3. Database URL (Reads DATABASE_URL if set, otherwise points to <storage_root>/db/app.db)
        let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            let db_path = storage_root.join("db/app.db");
            format!("sqlite://{}?mode=rwc", db_path.to_string_lossy())
        });

        // 4. API Key for Shortcuts / Mobile upload
        let vault_api_key = std::env::var("VAULT_API_KEY").unwrap_or_default();

        // 5. Workers for media processing
        let worker_concurrency = std::env::var("WORKER_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(5);

        Self {
            server_host,
            server_port,
            storage_root,
            db_url,
            vault_api_key,
            worker_concurrency,
        }
    }
}