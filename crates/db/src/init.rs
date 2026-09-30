use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
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
        .busy_timeout(Duration::from_secs(30))
        .foreign_keys(false); // Crucial: avoid FK lock conflicts during DDL migrations

    let mut migration_conn = SqliteConnection::connect_with(&migration_options).await?;

    info!("Applying database migrations safely...");
    sqlx::migrate!("./migrations")
        .run(&mut migration_conn)
        .await
        .map_err(|e| {
            tracing::error!("Failed to apply migrations: {e}");
            sqlx::Error::Migrate(Box::new(e))
        })?;

    // Close the migration runner connection explicitly before starting the pool
    migration_conn.close().await?;

    // -------------------------------------------------------------------------
    // Step 2: Initialize production pool with full runtime pragmas
    // -------------------------------------------------------------------------
    let pool_options = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(false) // Migration step already verified/created it
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        .pragma("cache_size", "-64000")        // 64 MB page cache
        .pragma("mmap_size", "268435456")      // 256 MB memory mapping
        .pragma("temp_store", "memory")        // Memory-backed temp tables
        .pragma("wal_autocheckpoint", "1000"); // Auto-prune WAL at 1,000 pages (~4MB)

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(pool_options)
        .await?;

    info!("Database initialized with optimized WAL pragmas & auto-checkpointing.");
    Ok(pool)
}