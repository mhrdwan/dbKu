//! Statement execution shared by the sqlx drivers (Postgres, MySQL, SQLite).
//!
//! Statements run one at a time with `raw_sql`, which uses each database's
//! text protocol, so every value can be read back as text regardless of type.

use std::time::Instant;

use futures_util::TryStreamExt;
use sqlx::{Column, Database, Either, Executor, Row, TypeInfo};

use crate::model::{Cell, ColumnMeta, ExecOutcome, ResultSet, StatementError};
use crate::sql::split::Statement;
use crate::transfer::DumpValue;

pub trait TextRow: Database {
    fn cell(row: &Self::Row, index: usize) -> Cell;
    /// The value for re-insertion (dumps): numbers unquoted, binary as bytes.
    fn dump_cell(row: &Self::Row, index: usize) -> DumpValue;
    fn rows_affected(result: &Self::QueryResult) -> u64;
}

fn type_name_of<R: Row>(row: &R, index: usize) -> String {
    row.columns()
        .get(index)
        .map(|c| c.type_info().name().to_ascii_uppercase())
        .unwrap_or_default()
}

impl TextRow for sqlx::Postgres {
    fn cell(row: &Self::Row, index: usize) -> Cell {
        row.try_get_unchecked::<Option<String>, _>(index)
            .unwrap_or_else(|e| Some(format!("<{e}>")))
    }
    fn dump_cell(row: &Self::Row, index: usize) -> DumpValue {
        // Text protocol: bytea arrives as `\x…` text, which is also valid input.
        let Ok(Some(text)) = row.try_get_unchecked::<Option<String>, _>(index) else {
            return DumpValue::Null;
        };
        match type_name_of(row, index).as_str() {
            "INT2" | "INT4" | "INT8" | "FLOAT4" | "FLOAT8" | "NUMERIC" | "OID" => {
                DumpValue::numeric(text)
            }
            _ => DumpValue::Text(text),
        }
    }
    fn rows_affected(result: &Self::QueryResult) -> u64 {
        result.rows_affected()
    }
}

impl TextRow for sqlx::MySql {
    fn cell(row: &Self::Row, index: usize) -> Cell {
        match row.try_get_unchecked::<Option<String>, _>(index) {
            Ok(v) => v,
            Err(_) => super::value::text_or_hex(
                row.try_get_unchecked::<Option<Vec<u8>>, _>(index)
                    .ok()
                    .flatten(),
            ),
        }
    }
    fn dump_cell(row: &Self::Row, index: usize) -> DumpValue {
        const NUMERIC: &[&str] = &[
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "INT",
            "BIGINT",
            "DECIMAL",
            "FLOAT",
            "DOUBLE",
            "YEAR",
        ];
        let type_name = type_name_of(row, index);
        let numeric = NUMERIC
            .iter()
            .any(|t| type_name.split(' ').next() == Some(*t));
        match row.try_get_unchecked::<Option<String>, _>(index) {
            Ok(None) => DumpValue::Null,
            Ok(Some(text)) if numeric => DumpValue::numeric(text),
            Ok(Some(text)) => DumpValue::Text(text),
            Err(_) => match row.try_get_unchecked::<Option<Vec<u8>>, _>(index) {
                Ok(Some(bytes)) => DumpValue::Bytes(bytes),
                _ => DumpValue::Null,
            },
        }
    }
    fn rows_affected(result: &Self::QueryResult) -> u64 {
        result.rows_affected()
    }
}

impl TextRow for sqlx::Sqlite {
    fn cell(row: &Self::Row, index: usize) -> Cell {
        use sqlx::ValueRef;
        let Ok(raw) = row.try_get_raw(index) else {
            return None;
        };
        if raw.is_null() {
            return None;
        }
        let storage = raw.type_info().name().to_string();
        let text = match storage.as_str() {
            "INTEGER" => row
                .try_get_unchecked::<i64, _>(index)
                .map(|v| v.to_string()),
            "REAL" => row
                .try_get_unchecked::<f64, _>(index)
                .map(|v| v.to_string()),
            "BLOB" => row
                .try_get_unchecked::<Vec<u8>, _>(index)
                .map(|b| super::value::hex_blob(&b)),
            _ => row.try_get_unchecked::<String, _>(index),
        };
        Some(text.unwrap_or_else(|e| format!("<{e}>")))
    }
    fn dump_cell(row: &Self::Row, index: usize) -> DumpValue {
        use sqlx::ValueRef;
        let Ok(raw) = row.try_get_raw(index) else {
            return DumpValue::Null;
        };
        if raw.is_null() {
            return DumpValue::Null;
        }
        let storage = raw.type_info().name().to_string();
        let value = match storage.as_str() {
            "INTEGER" => row
                .try_get_unchecked::<i64, _>(index)
                .map(|v| DumpValue::Number(v.to_string())),
            "REAL" => row
                .try_get_unchecked::<f64, _>(index)
                .map(|v| DumpValue::numeric(v.to_string())),
            "BLOB" => row
                .try_get_unchecked::<Vec<u8>, _>(index)
                .map(DumpValue::Bytes),
            _ => row
                .try_get_unchecked::<String, _>(index)
                .map(DumpValue::Text),
        };
        value.unwrap_or(DumpValue::Null)
    }
    fn rows_affected(result: &Self::QueryResult) -> u64 {
        result.rows_affected()
    }
}

/// First words of statements that produce rows; used to fetch column names
/// for empty results.
const ROW_RETURNING: &[&str] = &[
    "SELECT", "WITH", "VALUES", "TABLE", "SHOW", "PRAGMA", "EXPLAIN",
];

pub struct Run {
    pub outcome: ExecOutcome,
    /// A result was cut short and its unread rows are still on the wire:
    /// the caller must close this connection instead of reusing it.
    pub needs_reset: bool,
}

/// How a cut-short result is abandoned.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Abandon {
    /// Network databases keep streaming; the connection must be closed.
    ResetConnection,
    /// SQLite is in-process: dropping the stream just resets the statement.
    Free,
}

pub async fn run_statements<DB>(
    conn: &mut DB::Connection,
    statements: &[Statement],
    max_rows: usize,
    abandon: Abandon,
) -> Run
where
    DB: TextRow,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
{
    let started = Instant::now();
    let mut outcome = ExecOutcome::default();
    let mut needs_reset = false;
    for (index, stmt) in statements.iter().enumerate() {
        match run_one::<DB>(conn, stmt, max_rows).await {
            Ok((sets, truncated)) => {
                outcome.results.extend(sets);
                if truncated && abandon == Abandon::ResetConnection {
                    // Unread rows would otherwise be drained from the socket
                    // on next use; closing the connection is much cheaper.
                    needs_reset = true;
                    let skipped = statements.len() - index - 1;
                    let rest = if skipped > 0 {
                        format!(", and the remaining {skipped} statement(s) did not run")
                    } else {
                        String::new()
                    };
                    outcome.notices.push(format!(
                        "The result was cut at {max_rows} rows. dbKu closed the connection to \
                         stop the transfer, so any open transaction on it was rolled back{rest}."
                    ));
                    break;
                }
            }
            Err(e) => {
                outcome.error = Some(StatementError {
                    index,
                    statement: stmt.text.clone(),
                    message: crate::error::AppError::from(e).to_string(),
                });
                break;
            }
        }
    }
    outcome.elapsed_ms = elapsed_ms(started);
    Run {
        outcome,
        needs_reset,
    }
}

async fn run_one<DB>(
    conn: &mut DB::Connection,
    stmt: &Statement,
    max_rows: usize,
) -> Result<(Vec<ResultSet>, bool), sqlx::Error>
where
    DB: TextRow,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
{
    let started = Instant::now();
    let mut sets = Vec::new();
    let mut current: Option<ResultSet> = None;
    let mut truncated = false;
    {
        let mut stream = sqlx::raw_sql(&stmt.text).fetch_many(&mut *conn);
        while let Some(item) = stream.try_next().await? {
            match item {
                Either::Right(row) => {
                    let set = current.get_or_insert_with(|| ResultSet {
                        columns: columns_of::<DB>(row.columns()),
                        ..ResultSet::default()
                    });
                    if set.rows.len() >= max_rows {
                        set.truncated = true;
                        truncated = true;
                        break;
                    }
                    set.rows
                        .push((0..row.len()).map(|i| DB::cell(&row, i)).collect());
                }
                Either::Left(result) => {
                    let mut set = current.take().unwrap_or_default();
                    set.rows_affected = Some(DB::rows_affected(&result));
                    sets.push(set);
                }
            }
        }
    }
    sets.extend(current);

    let returns_rows = stmt
        .first_word()
        .is_some_and(|w| ROW_RETURNING.contains(&w));
    if returns_rows && sets.iter().all(|s| s.columns.is_empty()) && !truncated {
        // Empty result: ask the database for the column names anyway.
        if let Ok(described) = (&mut *conn).describe(&stmt.text).await {
            let columns = columns_of::<DB>(described.columns());
            match sets.first_mut() {
                Some(first) => first.columns = columns,
                None => sets.push(ResultSet {
                    columns,
                    ..ResultSet::default()
                }),
            }
        }
    }

    let ms = elapsed_ms(started);
    for set in &mut sets {
        set.statement = stmt.text.clone();
        set.elapsed_ms = ms;
    }
    Ok((sets, truncated))
}

fn columns_of<DB: Database>(columns: &[DB::Column]) -> Vec<ColumnMeta> {
    columns
        .iter()
        .map(|c| ColumnMeta {
            name: c.name().to_string(),
            type_name: c.type_info().name().to_string(),
        })
        .collect()
}

pub fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Run grid edits atomically; every UPDATE/DELETE must hit exactly one row.
pub async fn apply_in_tx<DB>(
    pool: &sqlx::Pool<DB>,
    statements: &[(String, bool)],
) -> crate::error::AppResult<u64>
where
    DB: TextRow,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
{
    let mut tx = pool.begin().await?;
    let mut total = 0;
    for (sql, must_match_one) in statements {
        let result = (&mut *tx).execute(sqlx::raw_sql(sql)).await?;
        let affected = DB::rows_affected(&result);
        super::check_single_row(sql, affected, *must_match_one)?;
        total += affected;
    }
    tx.commit().await?;
    Ok(total)
}
