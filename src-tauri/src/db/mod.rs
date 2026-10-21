pub mod bulk;
pub mod meta;
pub mod mssql;
mod mssql_bulk;
pub mod mysql;
pub mod postgres;
pub mod run;
mod session_bulk;
pub mod sqlite;
pub mod tab_sessions;
pub mod value;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{AppError, AppResult};
use crate::model::{
    ConnectionProfile, ExecOptions, ExecOutcome, QueryContext, ResultSet, RowChange, ServerInfo,
    TableQuery, TableRef,
};
use crate::sql::builder::{build_change, build_count, build_select, SelectSpec};
use crate::sql::split::{split, Statement};
use crate::sql::{guard, Dialect};

pub const DEFAULT_MAX_ROWS: usize = 5_000;
const HARD_MAX_ROWS: usize = 200_000;

// Boxed variants: their connect options are hundreds of bytes.
pub enum Backend {
    Postgres(Box<postgres::PgBackend>),
    Mysql(Box<mysql::MySqlBackend>),
    Sqlite(sqlite::SqliteBackend),
    Mssql(Box<mssql::MssqlBackend>),
}

/// An open connection plus the profile it was opened with.
pub struct Session {
    pub profile: ConnectionProfile,
    backend: Backend,
}

impl Session {
    pub async fn open(profile: ConnectionProfile, password: Option<&str>) -> AppResult<Self> {
        use crate::model::Driver;
        let backend = match profile.driver {
            Driver::Postgres => Backend::Postgres(Box::new(
                postgres::PgBackend::connect(&profile, password).await?,
            )),
            Driver::Mysql => Backend::Mysql(Box::new(
                mysql::MySqlBackend::connect(&profile, password).await?,
            )),
            Driver::Sqlite => Backend::Sqlite(sqlite::SqliteBackend::connect(&profile).await?),
            Driver::Mssql => Backend::Mssql(Box::new(
                mssql::MssqlBackend::connect(&profile, password).await?,
            )),
        };
        Ok(Self { profile, backend })
    }

    pub fn dialect(&self) -> Dialect {
        self.profile.driver.into()
    }

    pub fn default_database(&self) -> Option<String> {
        match &self.backend {
            Backend::Postgres(b) => Some(b.default_db().to_string()),
            Backend::Mysql(b) => b.default_db().map(String::from),
            Backend::Sqlite(_) => Some("main".to_string()),
            Backend::Mssql(b) => b.default_db().map(String::from),
        }
    }

    pub async fn server_info(&self) -> AppResult<ServerInfo> {
        let sql = match self.dialect() {
            Dialect::Postgres => "SELECT version()",
            Dialect::Mysql => "SELECT CONCAT('MySQL ', VERSION())",
            Dialect::Sqlite => "SELECT 'SQLite ' || sqlite_version()",
            Dialect::Mssql => "SELECT @@VERSION",
        };
        let version = self
            .query_value(&QueryContext::default(), sql)
            .await?
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        Ok(ServerInfo {
            version,
            default_database: self.default_database(),
        })
    }

    /// Run SQL typed by the user. In read-only mode it must pass the guard.
    pub async fn execute(
        &self,
        ctx: &QueryContext,
        sql: &str,
        opts: &ExecOptions,
    ) -> AppResult<ExecOutcome> {
        let statements = split(sql, self.dialect());
        if self.profile.read_only {
            guard::check_read_only(&statements, self.dialect()).map_err(AppError::ReadOnly)?;
        }
        let max_rows = opts
            .max_rows
            .unwrap_or(DEFAULT_MAX_ROWS)
            .clamp(1, HARD_MAX_ROWS);
        let target = Target::Editor {
            run_id: opts.run_id.as_deref(),
            tab: opts.session.as_deref(),
        };
        self.run(ctx, &statements, max_rows, target).await
    }

    /// Release a query tab's dedicated connection.
    pub async fn close_tab(&self, tab: &str) {
        match &self.backend {
            Backend::Postgres(b) => b.close_tab(tab).await,
            Backend::Mysql(b) => b.close_tab(tab).await,
            // SQLite has one connection; SQL Server one shared editor client.
            Backend::Sqlite(_) | Backend::Mssql(_) => {}
        }
    }

    /// Guarded like `execute`, but on the metadata lane so table browsing
    /// never queues behind a long-running editor query.
    async fn browse(&self, ctx: &QueryContext, sql: &str, max_rows: usize) -> AppResult<ResultSet> {
        let statements = split(sql, self.dialect());
        if self.profile.read_only {
            guard::check_read_only(&statements, self.dialect()).map_err(AppError::ReadOnly)?;
        }
        let outcome = self.run(ctx, &statements, max_rows, Target::Meta).await?;
        if let Some(err) = outcome.error {
            return Err(AppError::Database(err.message));
        }
        Ok(outcome.results.into_iter().next().unwrap_or_default())
    }

    pub async fn cancel(&self, run_id: &str) -> AppResult<bool> {
        match &self.backend {
            Backend::Postgres(b) => b.cancel(run_id).await,
            Backend::Mysql(b) => b.cancel(run_id).await,
            Backend::Sqlite(_) | Backend::Mssql(_) => Ok(false),
        }
    }

    async fn run(
        &self,
        ctx: &QueryContext,
        statements: &[Statement],
        max_rows: usize,
        target: Target<'_>,
    ) -> AppResult<ExecOutcome> {
        let (run_id, tab, lane) = match target {
            Target::Editor { run_id, tab } => (run_id, tab, mssql::Lane::User),
            Target::Meta => (None, None, mssql::Lane::Meta),
        };
        match &self.backend {
            Backend::Postgres(b) => b.run(ctx, statements, max_rows, run_id, tab).await,
            Backend::Mysql(b) => b.run(ctx, statements, max_rows, run_id, tab).await,
            Backend::Sqlite(b) => b.run(statements, max_rows).await,
            Backend::Mssql(b) => b.run(ctx, statements, max_rows, lane).await,
        }
    }

    /// Internal metadata query: one statement, first result set, errors as `Err`.
    pub(crate) async fn query(&self, ctx: &QueryContext, sql: &str) -> AppResult<ResultSet> {
        let statement = Statement {
            text: sql.to_string(),
            words: Vec::new(),
            has_executable_comment: false,
        };
        let outcome = self
            .run(
                ctx,
                std::slice::from_ref(&statement),
                HARD_MAX_ROWS,
                Target::Meta,
            )
            .await?;
        if let Some(err) = outcome.error {
            return Err(AppError::Database(err.message));
        }
        Ok(outcome
            .results
            .into_iter()
            .find(|r| !r.columns.is_empty())
            .unwrap_or_default())
    }

    pub(crate) async fn query_value(
        &self,
        ctx: &QueryContext,
        sql: &str,
    ) -> AppResult<Option<String>> {
        let set = self.query(ctx, sql).await?;
        Ok(set
            .rows
            .into_iter()
            .next()
            .and_then(|r| r.into_iter().next().flatten()))
    }

    pub async fn fetch_table(&self, q: &TableQuery) -> AppResult<ResultSet> {
        let columns = self.search_columns(q).await?;
        let spec = select_spec(q, &columns);
        let sql = build_select(self.dialect(), &spec);
        self.browse(&context_for(&q.table), &sql, spec.limit as usize)
            .await
    }

    pub async fn count_rows(&self, q: &TableQuery) -> AppResult<u64> {
        let columns = self.search_columns(q).await?;
        let sql = build_count(self.dialect(), &select_spec(q, &columns));
        let value = self
            .browse(&context_for(&q.table), &sql, 1)
            .await?
            .rows
            .into_iter()
            .next()
            .and_then(|r| r.into_iter().next().flatten())
            .unwrap_or_default();
        value
            .trim()
            .parse()
            .map_err(|_| AppError::Database(format!("unexpected count `{value}`")))
    }

    async fn search_columns(&self, q: &TableQuery) -> AppResult<Vec<String>> {
        let wants_search = q.search.as_deref().is_some_and(|s| !s.is_empty());
        if !wants_search || !q.search_columns.is_empty() {
            return Ok(q.search_columns.clone());
        }
        let columns = meta::columns(self, &q.table).await?;
        Ok(columns.into_iter().map(|c| c.name).collect())
    }

    pub fn preview_changes(
        &self,
        table: &TableRef,
        changes: &[RowChange],
    ) -> AppResult<Vec<String>> {
        changes
            .iter()
            .map(|c| build_change(self.dialect(), table, c).map_err(AppError::Invalid))
            .collect()
    }

    pub async fn apply_changes(&self, table: &TableRef, changes: &[RowChange]) -> AppResult<u64> {
        if self.profile.read_only {
            return Err(AppError::ReadOnly("this connection is read-only".into()));
        }
        let statements: Vec<(String, bool)> = self
            .preview_changes(table, changes)?
            .into_iter()
            .zip(changes)
            .map(|(sql, change)| (sql, !matches!(change, RowChange::Insert { .. })))
            .collect();
        if statements.is_empty() {
            return Ok(0);
        }
        match &self.backend {
            Backend::Postgres(b) => b.apply(table.database.as_deref(), &statements).await,
            Backend::Mysql(b) => b.apply(&statements).await,
            Backend::Sqlite(b) => b.apply(&statements).await,
            Backend::Mssql(b) => b.apply(table.database.as_deref(), &statements).await,
        }
    }

    pub async fn close(&self) {
        match &self.backend {
            Backend::Postgres(b) => b.close().await,
            Backend::Mysql(b) => b.close().await,
            Backend::Sqlite(b) => b.close().await,
            Backend::Mssql(_) => {}
        }
    }
}

/// Where a run goes: the user's editor (cancellable, optionally on a tab's
/// own connection) or the metadata/browsing side.
#[derive(Clone, Copy)]
enum Target<'a> {
    Editor {
        run_id: Option<&'a str>,
        tab: Option<&'a str>,
    },
    Meta,
}

fn context_for(t: &TableRef) -> QueryContext {
    QueryContext {
        database: t.database.clone(),
        schema: t.schema.clone(),
    }
}

fn select_spec<'a>(q: &'a TableQuery, columns: &'a [String]) -> SelectSpec<'a> {
    SelectSpec {
        table: &q.table,
        sort: &q.sort,
        filter: q.filter.as_deref(),
        search: q.search.as_deref(),
        search_columns: columns,
        limit: q.limit.clamp(1, HARD_MAX_ROWS as u32),
        offset: q.offset,
    }
}

pub(crate) fn check_single_row(sql: &str, affected: u64, must_match_one: bool) -> AppResult<()> {
    if must_match_one && affected != 1 {
        return Err(AppError::Database(format!(
            "`{sql}` matched {affected} rows instead of 1 (the row changed or its key is not \
             unique). Nothing was saved."
        )));
    }
    Ok(())
}

/// Tracks in-flight runs so they can be cancelled from another call.
pub struct RunRegistry<T: Clone> {
    inner: Arc<Mutex<HashMap<String, T>>>,
}

impl<T: Clone> Default for RunRegistry<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<T: Clone> RunRegistry<T> {
    pub fn register(&self, id: &str, value: T) -> RunGuard<T> {
        if let Ok(mut map) = self.inner.lock() {
            map.insert(id.to_string(), value);
        }
        RunGuard {
            inner: Arc::clone(&self.inner),
            id: id.to_string(),
        }
    }

    pub fn ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn get(&self, id: &str) -> Option<T> {
        self.inner.lock().ok().and_then(|m| m.get(id).cloned())
    }
}

/// Removes the run from the registry when the run finishes (or panics).
pub struct RunGuard<T> {
    inner: Arc<Mutex<HashMap<String, T>>>,
    id: String,
}

impl<T> Drop for RunGuard<T> {
    fn drop(&mut self) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(&self.id);
        }
    }
}
