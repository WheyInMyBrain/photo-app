mod config;
mod db;
mod domain;
mod error;
mod routes;
mod services;
mod middleware;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue},
    routing::{get, post},
    Router,
};
use config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::{
    services::ServeDir,
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use services::face_engine::FaceEngine;
use services::queue::{ProcessJob, QueueService};
use services::tag_engine::TagEngine;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub config: Config,
    pub job_sender: mpsc::Sender<ProcessJob>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::init();

    tokio::fs::create_dir_all(&config.storage_root.join("db")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("originals/public")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("originals/private")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("thumbs")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("thumbs/faces")).await?;
    tokio::fs::create_dir_all(&config.storage_root.join("models")).await?;

    let pool = db::init_db_pool(&config.db_url).await?;

    let models_dir = config.storage_root.join("models");
    let face_engine = Arc::new(FaceEngine::init(&models_dir).map_err(|e| e.to_string())?);
    let tag_engine = Arc::new(TagEngine::init(&models_dir).map_err(|e| e.to_string())?);

    let (tx, rx) = mpsc::channel::<ProcessJob>(100);

    QueueService::start_worker(
        rx,
        pool.clone(),
        config.storage_root.join("thumbs"),
        face_engine,
        tag_engine,
    );

    let state = AppState {
        db: pool,
        config: config.clone(),
        job_sender: tx,
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

        // Upload
        .route("/api/upload", post(routes::upload::upload_photo))
        .route("/api/upload/inspect", post(routes::upload::inspect_link))
        .route("/api/upload/commit", post(routes::upload::commit_link_download))

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
                .layer(axum::middleware::from_fn(
                    crate::middleware::api_key::require_api_key,
                )),
        )
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.server_port));
    info!("Application running on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}