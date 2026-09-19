use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

pub async fn init_db_pool(db_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        .pragma("cache_size", "-64000")              // 64 MB page cache
        .pragma("mmap_size", "268435456")            // 256 MB memory mapping
        .pragma("temp_store", "memory")             // Memory-backed temp tables
        .pragma("wal_autocheckpoint", "1000");       // Auto-prune WAL at 1,000 pages (~4MB)

    // 5 connections is the sweet spot for SQLite WAL mode:
    // Allows concurrent readers without saturating the file descriptor lock.
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await?;

    info!("Applying database migrations safely...");
    sqlx::migrate!("./migrations").run(&pool).await?;

    info!("Database initialized with optimized WAL pragmas & auto-checkpointing.");
    Ok(pool)
}