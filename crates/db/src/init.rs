use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePoolOptions,
    SqliteSynchronous,
};
use sqlx::{Connection, SqlitePool};
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

pub async fn init_db_pool(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    // -------------------------------------------------------------------------
    // Step 1: Run migrations on a single dedicated connection
    // -------------------------------------------------------------------------
    info!("Connecting single runner for database migrations...");
    let migration_options = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(60))
        .foreign_keys(false);

    let mut migration_conn = SqliteConnection::connect_with(&migration_options).await?;

    info!("Applying database migrations safely...");
    sqlx::migrate!("./migrations")
        .run(&mut migration_conn)
        .await
        .map_err(|e| {
            tracing::error!("Failed to apply migrations: {e}");
            sqlx::Error::Migrate(Box::new(e))
        })?;

    migration_conn.close().await?;

    // -------------------------------------------------------------------------
    // Step 2: Initialize production pool with full runtime pragmas
    // -------------------------------------------------------------------------
    let pool_options = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        // 1. Give SQLite up to 30s to wait out concurrent write locks before throwing SQLITE_BUSY
        .busy_timeout(Duration::from_secs(30))
        .pragma("cache_size", "-64000")        // 64 MB page cache
        .pragma("mmap_size", "268435456")      // 256 MB memory mapping
        .pragma("temp_store", "memory")        // In-memory temp tables
        .pragma("wal_autocheckpoint", "1000"); // Auto-prune WAL at 1,000 pages (~4MB)

    let pool = SqlitePoolOptions::new()
        // 2. Bump max connections so background workers cannot starve web readers
        .max_connections(25)
        // 3. Keep 3 warm connections alive to avoid latency spikes on cold requests
        .min_connections(3)
        // 4. Increase acquire timeout to match busy_timeout so requests don't bail at 10s
        .acquire_timeout(Duration::from_secs(30))
        // 5. Recycle idle connections cleanly
        .idle_timeout(Duration::from_secs(60))
        .connect_with(pool_options)
        .await?;

    info!("Database initialized with expanded connection pool (max 25) & WAL pragmas.");
    Ok(pool)
}