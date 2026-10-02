// photo-app/backend/src/main.rs

mod config;
mod error;
mod middleware;
mod routes;
mod services;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue},
    routing::{delete, get, post},
    Extension, Router,
};
use config::Config;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};
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
use services::queue::{QueueChannels, QueueService};
use services::trash_purger::TrashPurgerService;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum WsMediaEvent {
    #[serde(rename = "asset_ready")]
    AssetReady {
        asset_id: String,
        thumb_path: String,
        folder_path: String,
    },
    #[serde(rename = "asset_failed")]
    AssetFailed {
        asset_id: String,
        error: String,
    },
    #[serde(rename = "album_updated")]
    AlbumUpdated {
        album_id: Option<String>,
        folder_path: String,
        asset_id: String,
    },
    #[serde(rename = "people_updated")]
    PeopleUpdated {
        user_id: String,
        new_people_count: usize,
        affected_person_ids: Vec<String>,
    },
    #[serde(rename = "ai_completed")]
    AiCompleted {
        asset_id: String,
        faces_detected: usize,
        tags_count: usize,
    },
}

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub config: Config,
    pub tx_events: broadcast::Sender<WsMediaEvent>,
    pub coordinator: EngineCoordinator,
    pub channels: QueueChannels,
    pub upload_pause_tx: watch::Sender<bool>,
    pub upload_pause_rx: watch::Receiver<bool>,
    pub active_upload_batches: Arc<AtomicUsize>,
}

impl AppState {
    pub fn pause_processing(&self) {
        let prev = self.active_upload_batches.fetch_add(1, Ordering::SeqCst);
        if prev == 0 {
            let _ = self.upload_pause_tx.send(true);
            info!("Upload batch active: background workers PAUSED");
        }
    }

    pub fn resume_processing_if_idle(&self) {
        let prev = self.active_upload_batches.fetch_sub(1, Ordering::SeqCst);
        if prev <= 1 {
            let _ = self.upload_pause_tx.send(false);
            info!("All upload batches complete: background workers RESUMED");
        }
    }
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
    let (tx_events, _) = broadcast::channel::<WsMediaEvent>(100);

    // =========================================================================
    // 2. Initialize Bounded In-Memory Pipeline Channels & Pause Latch
    // =========================================================================
    let (assemble_tx, assemble_rx) = mpsc::channel::<db::domain::DbJob>(500);
    let (thumb_tx, thumb_rx) = mpsc::channel::<db::domain::DbJob>(500);
    let (ai_tx, ai_rx) = mpsc::channel::<db::domain::DbJob>(500);

    let (upload_pause_tx, upload_pause_rx) = watch::channel(false);
    let active_upload_batches = Arc::new(AtomicUsize::new(0));

    let channels = QueueChannels {
        assemble_tx: assemble_tx.clone(),
        thumb_tx: thumb_tx.clone(),
        ai_tx: ai_tx.clone(),
    };

    let state = AppState {
        db: pool.clone(),
        config: config.clone(),
        tx_events: tx_events.clone(),
        coordinator: coordinator.clone(),
        channels: channels.clone(),
        upload_pause_tx,
        upload_pause_rx,
        active_upload_batches,
    };

    // =========================================================================
    // 3. Start Event-Driven Workers
    // =========================================================================
    QueueService::start_pipeline(
        state.clone(),
        config.storage_root.clone(),
        coordinator.clone(),
        config.worker_concurrency,
        tx_events.clone(),
        assemble_rx,
        thumb_rx,
        ai_rx,
    );

    // =========================================================================
    // 4. WAL Startup Recovery: Refill channels from SQLite
    // =========================================================================
    {
        let pool = pool.clone();
        let a_tx = assemble_tx.clone();
        let t_tx = thumb_tx.clone();
        let ai_tx_init = ai_tx.clone();

        tokio::spawn(async move {
            if let Ok(jobs) = db::JobRepo::recover_uncompleted_jobs(&pool, "assemble").await {
                info!("WAL Startup Recovery: Refilling {} assemble jobs", jobs.len());
                for job in jobs {
                    let _ = a_tx.send(job).await;
                }
            }
            if let Ok(jobs) = db::JobRepo::recover_uncompleted_jobs(&pool, "thumbnail").await {
                info!("WAL Startup Recovery: Refilling {} thumbnail jobs", jobs.len());
                for job in jobs {
                    let _ = t_tx.send(job).await;
                }
            }
            if let Ok(jobs) = db::JobRepo::recover_uncompleted_jobs(&pool, "ai_enrichment").await {
                info!("WAL Startup Recovery: Refilling {} AI jobs", jobs.len());
                for job in jobs {
                    let _ = ai_tx_init.send(job).await;
                }
            }
        });
    }

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
        .route("/api/assets/{id}/poses", get(routes::media::get_asset_poses))

        // Upload Endpoints
        .route("/api/upload", post(routes::upload::upload_photo))
        .route("/api/upload/check", post(routes::upload::check_upload))
        .route(
            "/api/upload/chunk",
            post(routes::upload::upload_chunk).layer(DefaultBodyLimit::disable()),
        )
        .route("/api/upload/chunk/finalize", post(routes::upload::finalize_chunk))
        .route("/api/upload/inspect", post(routes::upload::inspect_link))
        .route("/api/upload/ingest", post(routes::upload::upload_ingest))
        .route("/api/upload/batch/start", post(routes::upload::start_batch))
        .route("/api/upload/batch/finish", post(routes::upload::finish_batch))

        // Event Stream
        .route("/api/events", get(routes::events::stream_events))

        // Folder Structure, Custom Albums & Collections
        .route(
            "/api/albums",
            get(routes::albums::list_albums).post(routes::albums::create_album),
        )
        .route(
            "/api/albums/{id}",
            get(routes::albums::get_album)
                .put(routes::albums::update_album)
                .delete(routes::albums::delete_album),
        )
        .route("/api/albums/{id}/assets", post(routes::albums::add_assets_to_album))
        .route("/api/albums/{id}/assets/remove", post(routes::albums::remove_assets_from_album))
        .route("/api/albums/{id}/remove-assets", post(routes::albums::remove_assets_from_album))
        .route(
            "/api/albums/{id}/cover",
            post(routes::albums::set_album_cover).put(routes::albums::set_album_cover),
        )
        .route(
            "/api/albums/{id}/reorder",
            post(routes::albums::reorder_album_assets).put(routes::albums::reorder_album_assets),
        )
        .route("/api/albums/{id}/delete", post(routes::albums::delete_album))
        .route("/api/albums/suggestions", get(routes::albums::get_folder_suggestions))

        // People & Faces
        .route("/api/smart-albums/people", get(routes::people::get_people_overview))
        .route("/api/assets/{id}/faces", get(routes::people::get_asset_faces))
        .route("/api/persons/{id}/name", post(routes::people::name_person))
        .route("/api/persons/{id}", delete(routes::people::delete_person))
        .route("/api/persons/{id}/delete", post(routes::people::delete_person))
        .route("/api/persons/merge", post(routes::people::merge_persons))
        .route("/api/persons/names", get(routes::people::get_names_directory))
        .route("/api/faces/{face_id}/reassign", post(routes::people::reassign_face))
        .route("/api/faces/{face_id}/verify", post(routes::people::verify_face))
        .route("/api/faces/{face_id}/unlink", post(routes::people::unlink_face))
        .route("/api/faces/{face_id}", delete(routes::people::delete_face))
        .route("/api/faces/{face_id}/delete", post(routes::people::delete_face))
        .route("/api/faces/{face_id}/split-new", post(routes::people::split_face_to_new_person))

        // Locations & Map data
        .route("/api/media/locations", get(routes::media::get_media_locations))

        // Tag Metadata
        .route("/api/assets/{id}/tags", get(routes::tags::get_asset_tags))

        // Static Asset Mounts
        .nest("/users", users_static_router)
        .nest("/thumbs", thumbs_router)
        .nest(
            "/api/shortcuts",
            Router::new()
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