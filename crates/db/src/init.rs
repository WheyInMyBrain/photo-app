use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;
use tracing::info;

#[derive(Clone, Debug)]
pub struct DbPools {
    /// Dedicated pool for web endpoints (Axum handlers, reads, quick transactions)
    pub http: PgPool,
    /// Dedicated pool for background queues (transcoding, CLIP, face inference)
    pub worker: PgPool,
}

pub async fn init_db_pools(db_url: &str) -> Result<DbPools, sqlx::Error> {
    info!("Initializing PostgreSQL connection pools...");

    // 1. HTTP Pool: Fast acquires, lower max limit, never blocked by heavy jobs
    let http_pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(60))
        .max_lifetime(Duration::from_secs(1800))
        .connect(db_url)
        .await?;

    // 2. Worker Pool: Tuned for longer-running transactional tasks
    let worker_pool = PgPoolOptions::new()
        .max_connections(15)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(15))
        .idle_timeout(Duration::from_secs(60))
        .max_lifetime(Duration::from_secs(1800))
        .connect(db_url)
        .await?;

    // 3. Apply migrations once using the HTTP pool
    info!("Running database migrations...");
    sqlx::migrate!("./migrations")
        .run(&http_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed applying PostgreSQL migrations: {e}");
            sqlx::Error::Migrate(Box::new(e))
        })?;

    info!("Database initialized successfully with separate HTTP & Worker pools.");

    Ok(DbPools {
        http: http_pool,
        worker: worker_pool,
    })
}