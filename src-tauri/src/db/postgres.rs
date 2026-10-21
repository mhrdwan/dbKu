use std::collections::HashMap;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions, PgSslMode};
use sqlx::{Connection, Executor};
use tokio::sync::Mutex;

use super::run::{run_statements, Abandon, Run};
use super::tab_sessions::TabSessions;
use super::RunRegistry;
use crate::error::AppResult;
use crate::model::{ConnectionProfile, ExecOutcome, QueryContext, SslMode};
use crate::sql::builder::quote_ident;
use crate::sql::split::Statement;
use crate::sql::Dialect;

/// Postgres connections are bound to one database, so we keep one small
/// pool per database (for browsing and metadata) plus one dedicated
/// connection per query tab.
pub struct PgBackend {
    base: PgConnectOptions,
    default_db: String,
    pools: Mutex<HashMap<String, PgPool>>,
    tabs: TabSessions<PgConnection>,
    running: RunRegistry<(String, i32)>,
}

impl PgBackend {
    pub async fn connect(profile: &ConnectionProfile, password: Option<&str>) -> AppResult<Self> {
        let mut base = PgConnectOptions::new()
            .host(&profile.host)
            .port(if profile.port == 0 {
                5432
            } else {
                profile.port
            })
            .username(&profile.user)
            .application_name("dbKu")
            .ssl_mode(match profile.ssl_mode {
                SslMode::Disable => PgSslMode::Disable,
                SslMode::Prefer => PgSslMode::Prefer,
                SslMode::Require => PgSslMode::Require,
            });
        if let Some(pw) = password {
            base = base.password(pw);
        }
        if profile.read_only {
            // Server-side safety net under the statement guard.
            base = base.options([("default_transaction_read_only", "on")]);
        }
        let default_db = match profile.database.trim() {
            "" => "postgres".to_string(),
            db => db.to_string(),
        };
        let backend = Self {
            base,
            default_db: default_db.clone(),
            pools: Mutex::new(HashMap::new()),
            tabs: TabSessions::default(),
            running: RunRegistry::default(),
        };
        backend.pool(Some(&default_db)).await?;
        Ok(backend)
    }

    pub fn default_db(&self) -> &str {
        &self.default_db
    }

    fn database_or_default(&self, database: Option<&str>) -> String {
        database
            .filter(|d| !d.is_empty())
            .unwrap_or(&self.default_db)
            .to_string()
    }

    pub(super) async fn pool_for(&self, database: Option<&str>) -> AppResult<PgPool> {
        self.pool(database).await
    }

    async fn pool(&self, database: Option<&str>) -> AppResult<PgPool> {
        let db = self.database_or_default(database);
        if let Some(pool) = self.pools.lock().await.get(&db) {
            return Ok(pool.clone());
        }
        // Connect without holding the lock, so one slow database doesn't
        // stall every other request.
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .min_connections(0)
            .acquire_timeout(Duration::from_secs(20))
            .idle_timeout(Duration::from_secs(300))
            .connect_with(self.base.clone().database(&db))
            .await?;
        Ok(self.pools.lock().await.entry(db).or_insert(pool).clone())
    }

    /// Run user SQL. With a `tab` key it runs on that tab's own connection,
    /// so session state (transactions, SET, temp tables) carries over.
    pub async fn run(
        &self,
        ctx: &QueryContext,
        statements: &[Statement],
        max_rows: usize,
        run_id: Option<&str>,
        tab: Option<&str>,
    ) -> AppResult<ExecOutcome> {
        let db = self.database_or_default(ctx.database.as_deref());
        match tab {
            Some(key) => {
                let mut conn = match self.tabs.take(key, &db).await {
                    Some(conn) => conn,
                    None => PgConnection::connect_with(&self.base.clone().database(&db)).await?,
                };
                let run = self
                    .run_on(&mut conn, &db, ctx, statements, max_rows, run_id)
                    .await?;
                if !run.needs_reset {
                    self.tabs.put(key, &db, conn).await;
                }
                Ok(run.outcome)
            }
            None => {
                let pool = self.pool(Some(&db)).await?;
                let mut conn = pool.acquire().await?;
                let run = self
                    .run_on(&mut conn, &db, ctx, statements, max_rows, run_id)
                    .await?;
                if run.needs_reset {
                    conn.close_on_drop();
                }
                Ok(run.outcome)
            }
        }
    }

    async fn run_on(
        &self,
        conn: &mut PgConnection,
        db: &str,
        ctx: &QueryContext,
        statements: &[Statement],
        max_rows: usize,
        run_id: Option<&str>,
    ) -> AppResult<Run> {
        let search_path = match ctx.schema.as_deref().filter(|s| !s.is_empty()) {
            Some(s) => format!(
                "SET search_path TO {}, public",
                quote_ident(Dialect::Postgres, s)
            ),
            None => "RESET search_path".to_string(),
        };
        conn.execute(sqlx::raw_sql(&search_path)).await?;
        let _guard = match run_id {
            Some(id) => {
                let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                    .fetch_one(&mut *conn)
                    .await?;
                Some(self.running.register(id, (db.to_string(), pid)))
            }
            None => None,
        };
        Ok(
            run_statements::<sqlx::Postgres>(conn, statements, max_rows, Abandon::ResetConnection)
                .await,
        )
    }

    /// Cancel over a fresh connection: the pool may be busy with the very
    /// queries the user wants to stop.
    pub async fn cancel(&self, run_id: &str) -> AppResult<bool> {
        let Some((db, pid)) = self.running.get(run_id) else {
            return Ok(false);
        };
        let mut conn = PgConnection::connect_with(&self.base.clone().database(&db)).await?;
        sqlx::query("SELECT pg_cancel_backend($1)")
            .bind(pid)
            .execute(&mut conn)
            .await?;
        let _ = conn.close().await;
        Ok(true)
    }

    pub async fn close_tab(&self, key: &str) {
        if let Some(conn) = self.tabs.remove(key).await {
            let _ = conn.close().await;
        }
    }

    /// Run grid edits atomically; every UPDATE/DELETE must hit exactly one row.
    pub async fn apply(
        &self,
        database: Option<&str>,
        statements: &[(String, bool)],
    ) -> AppResult<u64> {
        let pool = self.pool(database).await?;
        super::run::apply_in_tx(&pool, statements).await
    }

    /// Stop running queries, then close everything.
    pub async fn close(&self) {
        for id in self.running.ids() {
            let _ = self.cancel(&id).await;
        }
        for conn in self.tabs.drain().await {
            let _ = conn.close().await;
        }
        let pools: Vec<PgPool> = self.pools.lock().await.drain().map(|(_, p)| p).collect();
        for pool in pools {
            pool.close().await;
        }
    }
}
