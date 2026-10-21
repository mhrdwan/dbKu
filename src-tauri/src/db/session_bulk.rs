//! `Session` entry points for dumps, imports and scripts.

use sqlx::Executor;

use super::bulk::{exec_batches, exec_script, stream_rows};
use super::{Backend, Session};
use crate::error::{AppError, AppResult};
use crate::model::{QueryContext, ScriptOutcome};
use crate::sql::builder::quote_ident;
use crate::sql::split::split_iter;
use crate::sql::{guard, Dialect};
use crate::transfer::{DumpValue, Progress};

impl Session {
    /// Stream every row of an internal SELECT (used by dumps).
    pub async fn stream_select(
        &self,
        database: Option<&str>,
        sql: &str,
        on_row: &mut (dyn FnMut(Vec<DumpValue>) -> AppResult<()> + Send),
    ) -> AppResult<()> {
        match &self.backend {
            Backend::Postgres(b) => {
                let pool = b.pool_for(database).await?;
                let mut conn = pool.acquire().await?;
                let result = stream_rows::<sqlx::Postgres>(&mut conn, sql, on_row).await;
                if result.is_err() {
                    conn.close_on_drop();
                }
                result
            }
            Backend::Mysql(b) => {
                let mut conn = b.pool().acquire().await?;
                use_database(&mut conn, database).await?;
                let result = stream_rows::<sqlx::MySql>(&mut conn, sql, on_row).await;
                if result.is_err() {
                    conn.close_on_drop();
                }
                result
            }
            Backend::Sqlite(b) => {
                let mut conn = b.pool().acquire().await?;
                stream_rows::<sqlx::Sqlite>(&mut conn, sql, on_row).await
            }
            Backend::Mssql(b) => b.stream_rows(database, sql, on_row).await,
        }
    }

    /// Run write batches in one transaction (imports). Refused when read-only.
    pub async fn apply_batches(
        &self,
        database: Option<&str>,
        batches: &mut (dyn Iterator<Item = AppResult<String>> + Send),
    ) -> AppResult<u64> {
        if self.profile.read_only {
            return Err(AppError::ReadOnly("this connection is read-only".into()));
        }
        match &self.backend {
            Backend::Postgres(b) => exec_batches(&b.pool_for(database).await?, batches).await,
            Backend::Mysql(b) => {
                // Imports name tables with their database, so no USE is needed.
                exec_batches(b.pool(), batches).await
            }
            Backend::Sqlite(b) => exec_batches(b.pool(), batches).await,
            Backend::Mssql(b) => b.exec_batches(database, batches).await,
        }
    }

    /// Run a SQL file statement by statement. Read-only connections only run
    /// it when every statement passes the guard. The connection used is closed
    /// afterwards so whatever the script changed in the session doesn't leak.
    pub async fn run_script(
        &self,
        ctx: &QueryContext,
        sql: &str,
        progress: &dyn Progress,
    ) -> AppResult<ScriptOutcome> {
        let d = self.dialect();
        // First pass: count (and, when read-only, check) without keeping the
        // statements; the second pass splits again as it executes.
        let mut total = 0;
        for stmt in split_iter(sql, d) {
            if self.profile.read_only {
                guard::check_read_only(std::slice::from_ref(&stmt), d)
                    .map_err(AppError::ReadOnly)?;
            }
            total += 1;
        }
        let mut statements = split_iter(sql, d);
        match &self.backend {
            Backend::Postgres(b) => {
                let pool = b.pool_for(ctx.database.as_deref()).await?;
                let mut conn = pool.acquire().await?;
                conn.close_on_drop();
                if let Some(schema) = ctx.schema.as_deref().filter(|s| !s.is_empty()) {
                    let path = format!(
                        "SET search_path TO {}, public",
                        quote_ident(Dialect::Postgres, schema)
                    );
                    conn.execute(sqlx::raw_sql(&path)).await?;
                }
                Ok(
                    exec_script::<sqlx::Postgres>(&mut conn, &mut statements, total, progress)
                        .await,
                )
            }
            Backend::Mysql(b) => {
                let mut conn = b.pool().acquire().await?;
                conn.close_on_drop();
                use_database(&mut conn, ctx.database.as_deref()).await?;
                Ok(exec_script::<sqlx::MySql>(&mut conn, &mut statements, total, progress).await)
            }
            Backend::Sqlite(b) => {
                // SQLite has a single connection that we can't simply close:
                // undo whatever a failed or partial script left behind.
                let mut conn = b.pool().acquire().await?;
                let outcome =
                    exec_script::<sqlx::Sqlite>(&mut conn, &mut statements, total, progress).await;
                let _ = conn.execute("ROLLBACK").await; // fails harmlessly when nothing is open
                conn.execute("PRAGMA foreign_keys = ON").await?;
                Ok(outcome)
            }
            Backend::Mssql(b) => b.exec_script(ctx, &mut statements, total, progress).await,
        }
    }
}

async fn use_database(conn: &mut sqlx::MySqlConnection, database: Option<&str>) -> AppResult<()> {
    if let Some(db) = database.filter(|d| !d.is_empty()) {
        let sql = format!("USE {}", quote_ident(Dialect::Mysql, db));
        conn.execute(sqlx::raw_sql(&sql)).await?;
    }
    Ok(())
}
