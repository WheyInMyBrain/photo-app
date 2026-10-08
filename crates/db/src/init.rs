use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use std::str::FromStr;
use std::time::Duration;
use tracing::{error, info};

#[derive(Clone, Debug)]
pub struct DbPools {
    /// Dedicated pool for web endpoints (Axum handlers, reads, keyset pagination)
    pub http: PgPool,
    /// Dedicated pool for background queues (transcoding, CLIP, face inference, batch inserts)
    pub worker: PgPool,
}

pub async fn init_db_pools(db_url: &str) -> Result<DbPools, sqlx::Error> {
    info!("Initializing PostgreSQL connection pools...");

    let connect_opts = PgConnectOptions::from_str(db_url)?
        // Tune TCP keepalives to prevent silent drops across Docker networks
        .extra_float_digits(2);

    // 1. Run migrations FIRST on a dedicated single-connection temporary handle
    //    This guarantees zero migration lock contention with runtime pools.
    info!("Running database migrations...");
    let migration_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(connect_opts.clone())
        .await?;

    sqlx::migrate!("./migrations")
        .run(&migration_pool)
        .await
        .map_err(|e| {
            error!("Failed applying PostgreSQL migrations: {e}");
            sqlx::Error::Migrate(Box::new(e))
        })?;

    // Close migration pool immediately to release the slot
    migration_pool.close().await;

    // 2. HTTP Pool: Prioritizes fast response times for web traffic
    //    - 15 connections is ample for Axum async workloads.
    //    - Strict 3s acquire timeout so the frontend fails fast rather than freezing.
    let http_pool = PgPoolOptions::new()
        .max_connections(15)
        .min_connections(4)
        .acquire_timeout(Duration::from_secs(3))
        .idle_timeout(Duration::from_secs(120))
        .max_lifetime(Duration::from_secs(1800))
        .connect_with(connect_opts.clone())
        .await?;

    // 3. Worker Pool: Tuned for batch pipelines on mechanical storage (HDD)
    //    - Keep max_connections low (3 to 5 max) to avoid disk head contention.
    //    - Generous acquire timeout (30s) so background jobs queue cleanly.
    let worker_pool = PgPoolOptions::new()
        .max_connections(4)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(120))
        .max_lifetime(Duration::from_secs(1800))
        .connect_with(connect_opts)
        .await?;

    info!(
        http_max = 15,
        worker_max = 4,
        "Database pools initialized successfully with strict HDD-tuned limits."
    );

    Ok(DbPools {
        http: http_pool,
        worker: worker_pool,
    })
}