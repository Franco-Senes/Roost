use std::path::Path;
use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

/// Envoltorio sobre el pool de SQLite en modo WAL (`AGENTS.md` §2).
#[derive(Clone, Debug)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Abre o crea la base SQLite en `path`, activa WAL y corre las migraciones.
    pub async fn connect_file(path: &Path) -> Result<Self, sqlx::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;

        Self::migrate(pool).await
    }

    /// Crea una base SQLite temporal en memoria para tests de integración.
    pub async fn connect_in_memory() -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        Self::migrate(pool).await
    }

    async fn migrate(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Devuelve el pool interno de `sqlx`.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
