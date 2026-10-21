//! Types shared between the backend and the UI (all camelCase over IPC).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Driver {
    Postgres,
    Mysql,
    Sqlite,
    Mssql,
}

impl Driver {
    pub fn default_port(self) -> u16 {
        match self {
            Driver::Postgres => 5432,
            Driver::Mysql => 3306,
            Driver::Sqlite => 0,
            Driver::Mssql => 1433,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

/// A saved connection. The password is never part of this struct: it lives
/// in the OS keychain, keyed by `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub driver: Driver,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    /// Default database. For SQLite this is the file path.
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub ssl_mode: SslMode,
    /// SQL Server: accept self-signed server certificates.
    #[serde(default)]
    pub trust_server_cert: bool,
    /// SQL Server: allow TLS 1.0 for old servers (2008, 2012 without updates).
    #[serde(default)]
    pub legacy_tls: bool,
    /// Refuse every statement that could change data or schema.
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub save_password: bool,
}

/// What the connection dialog sends: a profile plus an optional password.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
    #[serde(flatten)]
    pub profile: ConnectionProfile,
    #[serde(default)]
    pub password: Option<String>,
}

/// Where a statement runs: which database, and which schema to resolve
/// unqualified names against.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryContext {
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableRef {
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    pub table: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub version: String,
    pub default_database: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbObject {
    pub name: String,
    /// "table" | "view" | "materialized_view" | "foreign_table"
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub is_primary_key: bool,
    pub is_auto_increment: bool,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    pub name: String,
    pub type_name: String,
}

pub type Cell = Option<String>;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultSet {
    pub statement: String,
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Cell>>,
    pub rows_affected: Option<u64>,
    /// More rows existed than the row limit allowed.
    pub truncated: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecOutcome {
    pub results: Vec<ResultSet>,
    /// Non-fatal information for the user (e.g. statements skipped).
    pub notices: Vec<String>,
    /// The statement that failed (0-based) and why. Results of the
    /// statements before it are still in `results`.
    pub error: Option<StatementError>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementError {
    pub index: usize,
    pub statement: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecOptions {
    pub max_rows: Option<usize>,
    /// Lets the UI cancel this run with `cancel_query`.
    pub run_id: Option<String>,
    /// Query-tab key: runs with the same key share one dedicated connection
    /// (transactions and session settings carry over). Released by
    /// `close_session`.
    pub session: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    pub column: String,
    pub dir: SortDir,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableQuery {
    #[serde(flatten)]
    pub table: TableRef,
    pub limit: u32,
    #[serde(default)]
    pub offset: u64,
    #[serde(default)]
    pub sort: Vec<SortSpec>,
    /// Raw WHERE clause typed by the user (without the WHERE keyword).
    #[serde(default)]
    pub filter: Option<String>,
    /// Free text matched against every column.
    #[serde(default)]
    pub search: Option<String>,
    /// Columns used for `search`; when empty all columns are searched.
    #[serde(default)]
    pub search_columns: Vec<String>,
}

/// One pending grid edit, identified by the row's primary-key values.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RowChange {
    #[serde(rename_all = "camelCase")]
    Update {
        key: Vec<(String, Cell)>,
        values: Vec<(String, Cell)>,
    },
    #[serde(rename_all = "camelCase")]
    Insert { values: Vec<(String, Cell)> },
    #[serde(rename_all = "camelCase")]
    Delete { key: Vec<(String, Cell)> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub schema: Option<String>,
    pub table: String,
    pub column: Option<String>,
    pub kind: String,
}

/// Result of running a SQL file.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptOutcome {
    pub total: usize,
    pub executed: usize,
    pub rows_affected: u64,
    pub error: Option<StatementError>,
    pub cancelled: bool,
}
