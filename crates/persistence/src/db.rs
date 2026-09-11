use easyjob_common::{Error, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::str::FromStr;

pub type DbPool = Pool<Sqlite>;

pub async fn init_pool(url: &str) -> Result<DbPool> {
    let options = SqliteConnectOptions::from_str(url)
        .map_err(|e| Error::Database(e.to_string()))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

    run_migrations(&pool).await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> Result<()> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}
