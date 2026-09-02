use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::env;

pub type Db = SqlitePool;

pub mod models;
pub mod queries;

/// Create a SQLite connection pool.
/// Defaults to sqlite:kehrkraft.db when DATABASE_URL is unset.
/// For sqlite::memory:, restrict to a single connection so the DB persists.
pub async fn connect_pool() -> Result<SqlitePool, sqlx::Error> {
    let mut url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:kehrkraft.db".to_string());

    let mut opts = SqlitePoolOptions::new();
    if url.starts_with("sqlite::memory:") {
        opts = opts.max_connections(1);
    } else {
        opts = opts.max_connections(5);
        // sqlx >= 0.9 opens file DBs read-write without creating them by default;
        // request creation unless the URL already pins a mode.
        if !url.contains("mode=") {
            url.push_str("?mode=rwc");
        }
    }

    let pool = opts.connect(&url).await?;

    // Best-effort: enable foreign keys for connections from this pool.
    let _ = sqlx::query("PRAGMA foreign_keys = ON;")
        .execute(&pool)
        .await;

    Ok(pool)
}

/// Apply embedded migrations from ./migrations at startup.
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!().run(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteRow;
    use sqlx::Row;

    #[tokio::test]
    async fn migrations_apply_on_memory_db() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("apply migrations");

        // Sanity: tables exist
        let count: i64 = sqlx::query("SELECT COUNT(*) FROM sqlite_master WHERE type='table'")
            .try_map(|row: SqliteRow| row.try_get::<i64, _>(0))
            .fetch_one(&pool)
            .await
            .expect("query sqlite_master");

        assert!(count >= 3, "expected tables to be created");
    }
}
