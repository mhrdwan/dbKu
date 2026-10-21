//! Bulk operations for SQL Server (see `bulk.rs` for the sqlx drivers).
//! Each runs on its own client, which is dropped afterwards.

use futures_util::TryStreamExt;
use tiberius::{ColumnData, QueryItem};

use super::bulk::label;
use super::mssql::{cell, Client, MssqlBackend};
use crate::error::{AppError, AppResult};
use crate::model::{QueryContext, ScriptOutcome, StatementError};
use crate::sql::split::Statement;
use crate::transfer::{DumpValue, Progress};

impl MssqlBackend {
    pub async fn stream_rows(
        &self,
        database: Option<&str>,
        sql: &str,
        on_row: &mut (dyn FnMut(Vec<DumpValue>) -> AppResult<()> + Send),
    ) -> AppResult<()> {
        let mut client = self.open_dedicated(database).await?;
        let mut stream = client.simple_query(sql).await?;
        while let Some(item) = stream.try_next().await? {
            if let QueryItem::Row(row) = item {
                on_row(row.cells().map(|(_, data)| dump_cell(data)).collect())?;
            }
        }
        Ok(())
    }

    /// All batches in one transaction; any error rolls everything back.
    pub async fn exec_batches(
        &self,
        database: Option<&str>,
        batches: &mut (dyn Iterator<Item = AppResult<String>> + Send),
    ) -> AppResult<u64> {
        let mut client = self.open_dedicated(database).await?;
        run(&mut client, "SET XACT_ABORT ON; BEGIN TRAN").await?;
        let mut total = 0;
        for batch in batches {
            let result = match batch {
                Ok(sql) => client.execute(sql, &[]).await.map_err(AppError::from),
                Err(e) => Err(e),
            };
            match result {
                Ok(r) => total += r.rows_affected().iter().sum::<u64>(),
                Err(e) => {
                    let _ = run(&mut client, "IF @@TRANCOUNT > 0 ROLLBACK TRAN").await;
                    return Err(e);
                }
            }
        }
        run(&mut client, "COMMIT TRAN").await?;
        Ok(total)
    }

    /// Runs `GO`-separated batches one by one, stopping at the first error.
    /// On a read-only connection every batch also runs inside a transaction
    /// that is always rolled back, as editor queries do.
    pub async fn exec_script(
        &self,
        ctx: &QueryContext,
        statements: &mut (dyn Iterator<Item = Statement> + Send),
        total: usize,
        progress: &dyn Progress,
    ) -> AppResult<ScriptOutcome> {
        let mut client = self.open_dedicated(ctx.database.as_deref()).await?;
        let read_only = self.read_only();
        let mut outcome = ScriptOutcome {
            total,
            ..ScriptOutcome::default()
        };
        for (index, stmt) in statements.enumerate() {
            if progress.cancelled() {
                outcome.cancelled = true;
                break;
            }
            let text = if read_only {
                format!("SET XACT_ABORT ON; BEGIN TRAN;\n{}", stmt.text)
            } else {
                stmt.text.clone()
            };
            let ran = run(&mut client, &text).await;
            if read_only {
                run(&mut client, "IF @@TRANCOUNT > 0 ROLLBACK TRAN").await?;
            }
            if let Err(e) = ran {
                outcome.error = Some(StatementError {
                    index,
                    statement: stmt.text.clone(),
                    message: e.to_string(),
                });
                break;
            }
            outcome.executed += 1;
            progress.report(index as u64 + 1, Some(total as u64), &label(&stmt));
        }
        Ok(outcome)
    }
}

async fn run(client: &mut Client, sql: &str) -> AppResult<()> {
    client.simple_query(sql).await?.into_results().await?;
    Ok(())
}

fn dump_cell(data: &ColumnData<'static>) -> DumpValue {
    match data {
        ColumnData::Binary(Some(bytes)) => DumpValue::Bytes(bytes.to_vec()),
        ColumnData::U8(_)
        | ColumnData::I16(_)
        | ColumnData::I32(_)
        | ColumnData::I64(_)
        | ColumnData::F32(_)
        | ColumnData::F64(_)
        | ColumnData::Bit(_)
        | ColumnData::Numeric(_) => cell(data).map_or(DumpValue::Null, DumpValue::numeric),
        _ => cell(data).map_or(DumpValue::Null, DumpValue::Text),
    }
}
