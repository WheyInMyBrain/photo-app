mod config;
mod error;
mod middleware;
mod routes;
mod services;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue},
    routing::{get, post},
    Extension, Router,
};
use config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, Notify};
use tower_http::{
    services::ServeDir,
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Use mimalloc globally to guarantee OS pages are reclaimed on drop
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use services::engine_coordinator::EngineCoordinator;
use services::queue::QueueService;
use services::trash_purger::TrashPurgerService;

#[derive(Clone, Debug, serde::Serialize)]
pub struct WsMediaEvent {
    pub event_type: String, // "asset_ready", "asset_failed"
    pub asset_id: String,
    pub thumb_path: String,
}

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub config: Config,
    pub queue_notify: Arc<Notify>,
    pub tx_events: broadcast::Sender<WsMediaEvent>,
    pub coordinator: EngineCoordinator,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::init();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Initialize required user storage directories
    tokio::fs::create_dir_all(&config.storage_root.join("db")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("users")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("temp_chunks")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("models")).await?;

    let pool = db::init_db_pool(&config.db_url).await?;

    TrashPurgerService::start(pool.clone(), config.storage_root.clone());

    // 1. Initialize coordinator (Zero ONNX models and zero vector caches loaded at boot)
    let models_dir = config.storage_root.join("models");
    let coordinator = EngineCoordinator::new(pool.clone(), models_dir);

    let queue_notify = Arc::new(Notify::new());
    let (tx_events, _) = broadcast::channel::<WsMediaEvent>(100);

    // 2. Start worker with the coordinator (Idle worker consumes ~0 MB until a job arrives)
    QueueService::start_worker(
        pool.clone(),
        config.storage_root.clone(),
        coordinator.clone(),
        config.worker_concurrency,
        queue_notify.clone(),
        tx_events.clone(),
    );

    let state = AppState {
        db: pool.clone(),
        config: config.clone(),
        queue_notify,
        tx_events,
        coordinator,
    };

    // Serves /users/<user_id>/thumbs/<shard>/<file> from <storage_root>/users/
    let users_static_router = Router::new()
        .fallback_service(ServeDir::new(config.storage_root.join("users")))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ));

    // Serves /thumbs/users/<user_id>/thumbs/<shard>/<file> if requested with /thumbs prefix
    let thumbs_router = Router::new()
        .fallback_service(ServeDir::new(&config.storage_root))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ));

    let app = Router::new()
        .route("/api/health", get(|| async { "OK" }))
        .merge(routes::auth::auth_routes())

        // Unified Media Endpoints
        .route("/api/media", get(routes::media::list_media))
        .route("/api/media/filters", get(routes::media::get_available_filters))
        .route("/api/assets/{id}/stream", get(routes::media::stream_asset))
        .route("/api/assets/{id}/favorite", post(routes::media::toggle_favorite))
        .route("/api/assets/{id}/similar", get(routes::media::get_similar_assets))
        .route("/api/assets/{id}/delete", post(routes::media::toggle_soft_delete))
        .route("/api/assets/{id}/purge", post(routes::media::hard_delete_asset))
        .route("/api/assets/batch/delete", post(routes::media::batch_toggle_soft_delete))
        .route("/api/assets/batch/purge", post(routes::media::batch_purge_assets))

        // Upload
        .route("/api/upload", post(routes::upload::upload_photo))
        .route("/api/upload/raw", post(routes::upload::upload_raw_binary))
        .route("/api/upload/chunk", post(routes::upload::upload_chunk))
        .route("/api/upload/chunk/finalize", post(routes::upload::finalize_chunk))
        .route("/api/upload/inspect", post(routes::upload::inspect_link))
        .route("/api/upload/commit", post(routes::upload::commit_link_download))

        // Event Stream
        .route("/api/events", get(routes::events::stream_events))

        // Folder Structure
        .route("/api/albums", get(routes::albums::get_album_contents))
        .route("/api/albums/suggestions", get(routes::albums::get_folder_suggestions))

        // People & Face Metadata
        .route("/api/smart-albums/people", get(routes::people::get_people_overview))
        .route("/api/assets/{id}/faces", get(routes::people::get_asset_faces))
        .route("/api/persons/{id}/name", post(routes::people::name_person))
        .route("/api/persons/merge", post(routes::people::merge_persons))
        .route("/api/faces/{face_id}/reassign", post(routes::people::reassign_face))
        .route("/api/faces/{face_id}/verify", post(routes::people::verify_face))

        // Locations & Map data
        .route("/api/media/locations", get(routes::media::get_media_locations))

        // Tag Metadata
        .route("/api/assets/{id}/tags", get(routes::tags::get_asset_tags))
        .route("/api/persons/names", get(routes::people::get_names_directory))

        // Static Asset Mounts
        .nest("/users", users_static_router)
        .nest("/thumbs", thumbs_router)
        .nest(
            "/api/shortcuts",
            Router::new()
                .route("/upload", post(routes::upload::upload_raw_binary))
                .route("/albums", get(routes::albums::get_folder_suggestions)),
        )
        .layer(Extension(pool.clone()))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.server_host, config.server_port)
        .parse()
        .expect("Invalid server address host/port configuration");

    let initial_ram = services::engine_coordinator::get_process_rss_mb();
    info!("------------------------------------------------------------");
    info!("Server running on http://{} | Initial Idle RAM: {:.2} MB", addr, initial_ram);
    info!("------------------------------------------------------------");

    services::backup::BackupService::start_scheduler(
        pool.clone(),
        config.storage_root.clone(),
        config.b2.clone(),
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}