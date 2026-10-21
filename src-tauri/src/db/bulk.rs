//! Bulk operations for the sqlx drivers: streaming rows out (dumps),
//! batched writes in one transaction (imports) and running scripts.

use futures_util::TryStreamExt;
use sqlx::{Executor, Row};

use super::run::TextRow;
use crate::error::{AppError, AppResult};
use crate::model::{ScriptOutcome, StatementError};
use crate::sql::split::Statement;
use crate::transfer::{DumpValue, Progress, CANCELLED};

/// Feeds every row of `sql` to `on_row`. When it returns an error the
/// stream stops early with unread rows, so the caller must not reuse the
/// connection (see `Abandon`).
pub async fn stream_rows<DB>(
    conn: &mut DB::Connection,
    sql: &str,
    on_row: &mut (dyn FnMut(Vec<DumpValue>) -> AppResult<()> + Send),
) -> AppResult<()>
where
    DB: TextRow,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
{
    let mut stream = sqlx::raw_sql(sql).fetch(&mut *conn);
    while let Some(row) = stream.try_next().await? {
        let values = (0..row.len()).map(|i| DB::dump_cell(&row, i)).collect();
        on_row(values)?;
    }
    Ok(())
}

/// Runs every batch inside one transaction; any error rolls everything back.
pub async fn exec_batches<DB>(
    pool: &sqlx::Pool<DB>,
    batches: &mut (dyn Iterator<Item = AppResult<String>> + Send),
) -> AppResult<u64>
where
    DB: TextRow,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
{
    let mut tx = pool.begin().await?;
    let mut total = 0;
    for batch in batches {
        let sql = batch?;
        let result = (&mut *tx).execute(sqlx::raw_sql(&sql)).await?;
        total += DB::rows_affected(&result);
    }
    tx.commit().await?;
    Ok(total)
}

/// Runs statements one by one, stopping at the first error (or a cancel).
/// Each statement commits on its own unless the script manages transactions.
pub async fn exec_script<DB>(
    conn: &mut DB::Connection,
    statements: &mut (dyn Iterator<Item = Statement> + Send),
    total: usize,
    progress: &dyn Progress,
) -> ScriptOutcome
where
    DB: TextRow,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
{
    let mut outcome = ScriptOutcome {
        total,
        ..ScriptOutcome::default()
    };
    for (index, stmt) in statements.enumerate() {
        if progress.cancelled() {
            outcome.cancelled = true;
            break;
        }
        match (&mut *conn).execute(sqlx::raw_sql(&stmt.text)).await {
            Ok(result) => {
                outcome.executed += 1;
                outcome.rows_affected += DB::rows_affected(&result);
            }
            Err(e) => {
                outcome.error = Some(StatementError {
                    index,
                    statement: stmt.text.clone(),
                    message: AppError::from(e).to_string(),
                });
                break;
            }
        }
        progress.report(index as u64 + 1, Some(total as u64), &label(&stmt));
    }
    outcome
}

pub fn label(stmt: &Statement) -> String {
    stmt.text
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn cancelled() -> AppError {
    AppError::Invalid(CANCELLED.to_string())
}
