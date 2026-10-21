use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

use super::run::{run_statements, Abandon};
use crate::error::{AppError, AppResult};
use crate::model::{ConnectionProfile, ExecOutcome};
use crate::sql::split::Statement;

pub struct SqliteBackend {
    pool: SqlitePool,
}

impl SqliteBackend {
    pub async fn connect(profile: &ConnectionProfile) -> AppResult<Self> {
        let path = profile.database.trim();
        if path.is_empty() {
            return Err(AppError::Invalid("SQLite needs a database file".into()));
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .read_only(profile.read_only)
            .create_if_missing(!profile.read_only)
            .busy_timeout(Duration::from_secs(5));
        // A single connection keeps session state (TEMP tables, ATTACH)
        // consistent between runs.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(0)
            .acquire_timeout(Duration::from_secs(30))
            .connect_with(opts)
            .await?;
        Ok(Self { pool })
    }

    pub(super) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn run(&self, statements: &[Statement], max_rows: usize) -> AppResult<ExecOutcome> {
        let mut conn = self.pool.acquire().await?;
        Ok(
            run_statements::<sqlx::Sqlite>(&mut conn, statements, max_rows, Abandon::Free)
                .await
                .outcome,
        )
    }

    pub async fn apply(&self, statements: &[(String, bool)]) -> AppResult<u64> {
        super::run::apply_in_tx(&self.pool, statements).await
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
