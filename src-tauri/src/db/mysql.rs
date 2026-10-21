use std::time::Duration;

use sqlx::mysql::{
    MySqlConnectOptions, MySqlConnection, MySqlPool, MySqlPoolOptions, MySqlSslMode,
};
use sqlx::{Connection, Executor};

use super::run::{run_statements, Abandon, Run};
use super::tab_sessions::TabSessions;
use super::RunRegistry;
use crate::error::AppResult;
use crate::model::{ConnectionProfile, ExecOutcome, QueryContext, SslMode};
use crate::sql::builder::quote_ident;
use crate::sql::split::Statement;
use crate::sql::Dialect;

/// Every session gets the same string rules: backslash escapes on and `"`
/// meaning a string. The SQL lexer (read-only guard, statement splitter) and
/// the literal quoting in `sql::builder` both rely on this, so a server whose
/// default sql_mode has NO_BACKSLASH_ESCAPES or ANSI_QUOTES can't make them
/// disagree with what the server parses. (`ANSI` is a combination mode that
/// includes ANSI_QUOTES, so it goes too.)
const NORMALIZE_SQL_MODE: &str = "SET SESSION sql_mode = TRIM(BOTH ',' FROM \
     REPLACE(REPLACE(REPLACE(CONCAT(',', @@SESSION.sql_mode, ','), \
     ',NO_BACKSLASH_ESCAPES,', ','), ',ANSI_QUOTES,', ','), ',ANSI,', ','))";

async fn prepare_session(conn: &mut MySqlConnection, read_only: bool) -> Result<(), sqlx::Error> {
    conn.execute(NORMALIZE_SQL_MODE).await?;
    if read_only {
        conn.execute("SET SESSION TRANSACTION READ ONLY").await?;
    }
    Ok(())
}

/// A pool for browsing and metadata, plus one dedicated connection per
/// query tab (see `tab_sessions`).
pub struct MySqlBackend {
    opts: MySqlConnectOptions,
    read_only: bool,
    pool: MySqlPool,
    default_db: Option<String>,
    tabs: TabSessions<MySqlConnection>,
    running: RunRegistry<u64>,
}

impl MySqlBackend {
    pub async fn connect(profile: &ConnectionProfile, password: Option<&str>) -> AppResult<Self> {
        let mut opts = MySqlConnectOptions::new()
            .host(&profile.host)
            .port(if profile.port == 0 {
                3306
            } else {
                profile.port
            })
            .username(&profile.user)
            .charset("utf8mb4")
            // Keep the server's time zone and most of its sql_mode: a GUI
            // should show what other clients see, not sqlx's defaults.
            .pipes_as_concat(false)
            .no_engine_substitution(false)
            .timezone(None)
            .ssl_mode(match profile.ssl_mode {
                SslMode::Disable => MySqlSslMode::Disabled,
                SslMode::Prefer => MySqlSslMode::Preferred,
                SslMode::Require => MySqlSslMode::Required,
            });
        if let Some(pw) = password {
            opts = opts.password(pw);
        }
        let default_db = Some(profile.database.trim().to_string()).filter(|d| !d.is_empty());
        if let Some(db) = &default_db {
            opts = opts.database(db);
        }
        let read_only = profile.read_only;
        let pool = MySqlPoolOptions::new()
            .max_connections(4)
            .min_connections(0)
            .acquire_timeout(Duration::from_secs(20))
            .idle_timeout(Duration::from_secs(300))
            .after_connect(move |conn, _| Box::pin(prepare_session(conn, read_only)))
            .connect_with(opts.clone())
            .await?;
        Ok(Self {
            opts,
            read_only,
            pool,
            default_db,
            tabs: TabSessions::default(),
            running: RunRegistry::default(),
        })
    }

    pub(super) fn pool(&self) -> &MySqlPool {
        &self.pool
    }

    pub fn default_db(&self) -> Option<&str> {
        self.default_db.as_deref()
    }

    async fn new_connection(&self) -> AppResult<MySqlConnection> {
        let mut conn = MySqlConnection::connect_with(&self.opts).await?;
        prepare_session(&mut conn, self.read_only).await?;
        Ok(conn)
    }

    /// Run user SQL. With a `tab` key it runs on that tab's own connection.
    /// MySQL switches databases with USE, so one connection serves them all.
    pub async fn run(
        &self,
        ctx: &QueryContext,
        statements: &[Statement],
        max_rows: usize,
        run_id: Option<&str>,
        tab: Option<&str>,
    ) -> AppResult<ExecOutcome> {
        match tab {
            Some(key) => {
                let mut conn = match self.tabs.take(key, "").await {
                    Some(conn) => conn,
                    None => self.new_connection().await?,
                };
                let run = self
                    .run_on(&mut conn, ctx, statements, max_rows, run_id)
                    .await?;
                if !run.needs_reset {
                    self.tabs.put(key, "", conn).await;
                }
                Ok(run.outcome)
            }
            None => {
                let mut conn = self.pool.acquire().await?;
                let run = self
                    .run_on(&mut conn, ctx, statements, max_rows, run_id)
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
        conn: &mut MySqlConnection,
        ctx: &QueryContext,
        statements: &[Statement],
        max_rows: usize,
        run_id: Option<&str>,
    ) -> AppResult<Run> {
        let db = ctx
            .database
            .as_deref()
            .or(self.default_db.as_deref())
            .filter(|d| !d.is_empty());
        if let Some(db) = db {
            let use_db = format!("USE {}", quote_ident(Dialect::Mysql, db));
            conn.execute(sqlx::raw_sql(&use_db)).await?;
        }
        let _guard = match run_id {
            Some(id) => {
                let thread: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()")
                    .fetch_one(&mut *conn)
                    .await?;
                Some(self.running.register(id, thread))
            }
            None => None,
        };
        Ok(
            run_statements::<sqlx::MySql>(conn, statements, max_rows, Abandon::ResetConnection)
                .await,
        )
    }

    /// Cancel over a fresh connection: the pool may be busy with the very
    /// queries the user wants to stop.
    pub async fn cancel(&self, run_id: &str) -> AppResult<bool> {
        let Some(thread) = self.running.get(run_id) else {
            return Ok(false);
        };
        let mut conn = MySqlConnection::connect_with(&self.opts).await?;
        conn.execute(sqlx::raw_sql(&format!("KILL QUERY {thread}")))
            .await?;
        let _ = conn.close().await;
        Ok(true)
    }

    pub async fn close_tab(&self, key: &str) {
        if let Some(conn) = self.tabs.remove(key).await {
            let _ = conn.close().await;
        }
    }

    pub async fn apply(&self, statements: &[(String, bool)]) -> AppResult<u64> {
        super::run::apply_in_tx(&self.pool, statements).await
    }

    /// Stop running queries, then close everything.
    pub async fn close(&self) {
        for id in self.running.ids() {
            let _ = self.cancel(&id).await;
        }
        for conn in self.tabs.drain().await {
            let _ = conn.close().await;
        }
        self.pool.close().await;
    }
}
