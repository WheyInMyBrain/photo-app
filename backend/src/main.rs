mod config;
mod db;
mod domain;
mod error;
mod middleware;
mod routes;
mod services;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue},
    routing::{get, post},
    Router,
};
use config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Notify, broadcast};
use tower_http::{
    services::ServeDir,
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use media_processing::{FaceEngine, MediaEngine, TagEngine};
use services::cluster_cache::ClusterCacheManager;
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::init();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tokio::fs::create_dir_all(&config.storage_root.join("db")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("originals/public")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("originals/private")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("temp_chunks")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("thumbs")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("thumbs/faces")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("models")).await?;

    let pool = db::init_db_pool(&config.db_url).await?;

    TrashPurgerService::start(pool.clone(), config.storage_root.clone());

    // 1. Initialize ONNX models once inside the media_processing crate
    let models_dir = config.storage_root.join("models");
    let face_engine = Arc::new(FaceEngine::init(&models_dir).map_err(|e| e.to_string())?);
    let tag_engine = Arc::new(TagEngine::init(&models_dir).map_err(|e| e.to_string())?);

    // 2. Wrap them into the unified MediaEngine
    let media_engine = Arc::new(MediaEngine::new(face_engine, tag_engine));

    // 3. Load centroids using ClusterCacheManager directly
    let cluster_cache = ClusterCacheManager::load_initial(&pool).await?;

    let queue_notify = Arc::new(Notify::new());

    let (tx_events, _) = broadcast::channel::<WsMediaEvent>(100);

    // 4. Start worker with unified MediaEngine & the RAM cluster cache
    QueueService::start_worker(
        pool.clone(),
        config.storage_root.join("thumbs"),
        media_engine,
        cluster_cache,
        config.worker_concurrency,
        queue_notify.clone(),
        tx_events.clone(),
    );

    let state = AppState {
        db: pool,
        config: config.clone(),
        queue_notify,
        tx_events,
    };

    let thumbs_router = Router::new()
        .fallback_service(ServeDir::new(config.storage_root.join("thumbs")))
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
        .route("/api/assets/{id}/delete", post(routes::media::toggle_soft_delete))
        .route("/api/assets/{id}/purge", post(routes::media::hard_delete_asset))
        .route("/api/assets/batch/delete", post(routes::media::batch_toggle_soft_delete))
        .route("/api/assets/batch/purge", post(routes::media::batch_purge_assets))

        // Upload
        .route("/api/upload", post(routes::upload::upload_photo))
        .route("/api/upload/chunk", post(routes::upload::upload_chunk))
        .route("/api/upload/chunk/finalize", post(routes::upload::finalize_chunk))
        .route("/api/upload/inspect", post(routes::upload::inspect_link))
        .route("/api/upload/commit", post(routes::upload::commit_link_download))

        // Event
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

        // Tag Metadata
        .route("/api/assets/{id}/tags", get(routes::tags::get_asset_tags))
        .route("/api/persons/names", get(routes::people::get_names_directory))

        .nest("/thumbs", thumbs_router)
        .nest(
            "/api/shortcuts",
            Router::new()
                .route("/upload", post(routes::upload::upload_raw_binary))
                .route("/albums", get(routes::albums::get_folder_suggestions))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    crate::middleware::api_key::require_api_key,
                )),
        )
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.server_host, config.server_port)
        .parse()
        .expect("Invalid server address host/port configuration");

    info!("Application running on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}