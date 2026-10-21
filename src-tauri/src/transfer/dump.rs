//! SQL dump: schema and data as a script that recreates them.
//!
//! Order: tables, then their data, then indexes / foreign keys / sequence
//! positions, then views (which may reference any table). Rows stream from
//! the database straight into the file.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ddl::{dump_name, object_ddl, schema_prelude, ObjectDdl};
use super::{sql_literal, DumpValue, Progress};
use crate::db::{bulk, meta, Session};
use crate::error::{AppError, AppResult};
use crate::model::{DbObject, TableRef};
use crate::sql::builder::{qualified_table, quote_ident};
use crate::sql::Dialect;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DumpOptions {
    pub structure: bool,
    pub data: bool,
    /// Emit DROP … IF EXISTS before each CREATE.
    pub drop_existing: bool,
    pub rows_per_insert: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DumpSummary {
    pub path: String,
    pub objects: usize,
    pub rows: u64,
}

/// Write the dump to `path` via a temporary file, so a failed or cancelled
/// dump never leaves a half-written script behind.
pub async fn dump_to_file(
    s: &Session,
    database: Option<&str>,
    schema: Option<&str>,
    objects: &[DbObject],
    opts: &DumpOptions,
    path: &Path,
    progress: &dyn Progress,
) -> AppResult<DumpSummary> {
    // A unique temp name never clobbers an unrelated file next to the target.
    let tmp = path.with_extension(format!("{}.partial", uuid::Uuid::new_v4().simple()));
    let result = async {
        let mut out = BufWriter::new(File::create(&tmp)?);
        let rows = write_dump(s, database, schema, objects, opts, &mut out, progress).await?;
        out.flush()?;
        Ok::<u64, AppError>(rows)
    }
    .await;
    match result {
        Ok(rows) => {
            std::fs::rename(&tmp, path)?;
            Ok(DumpSummary {
                path: path.display().to_string(),
                objects: objects.len(),
                rows,
            })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub async fn write_dump<W: Write + Send>(
    s: &Session,
    database: Option<&str>,
    schema: Option<&str>,
    objects: &[DbObject],
    opts: &DumpOptions,
    out: &mut W,
    progress: &dyn Progress,
) -> AppResult<u64> {
    let d = s.dialect();
    let tref = |o: &DbObject| TableRef {
        database: database.map(String::from),
        schema: schema.map(String::from),
        table: o.name.clone(),
    };

    writeln!(out, "-- dbKu SQL dump")?;
    writeln!(
        out,
        "-- Source: {}{}",
        database.unwrap_or("(default)"),
        schema.map(|s| format!(" / {s}")).unwrap_or_default()
    )?;
    writeln!(
        out,
        "-- Created: {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z")
    )?;
    writeln!(out)?;
    for line in prologue(d) {
        statement(out, d, line)?;
    }
    let prelude = schema_prelude(s, database, schema).await?;
    for line in &prelude.setup {
        statement(out, d, line)?;
    }

    // DDL is read up front so table creation never waits on data.
    let mut tables: Vec<(&DbObject, ObjectDdl)> = Vec::new();
    let mut views: Vec<(&DbObject, ObjectDdl)> = Vec::new();
    for o in objects {
        check(progress)?;
        let ddl = object_ddl(s, &tref(o), o).await?;
        if is_table(o) {
            tables.push((o, ddl));
        } else {
            views.push((o, ddl));
        }
    }
    let views = order_views(views);

    if opts.structure {
        if opts.drop_existing {
            writeln!(out, "\n-- Drop what is about to be recreated")?;
            for (_, ddl) in views.iter().rev() {
                statement(out, d, &ddl.drop)?;
            }
            for (_, ddl) in &tables {
                for pre in &ddl.pre_drop {
                    statement(out, d, pre)?;
                }
                statement(out, d, &ddl.drop)?;
            }
            for drop in &prelude.drop_types {
                statement(out, d, drop)?;
            }
        }
        for create in &prelude.types {
            statement(out, d, create)?;
        }
        for (o, ddl) in &tables {
            writeln!(out, "\n-- Table {}", o.name)?;
            for create in &ddl.create {
                statement(out, d, create)?;
            }
        }
    }

    let mut total_rows = 0;
    if opts.data {
        let table_count = tables.len() as u64;
        for (i, (o, ddl)) in tables.iter().enumerate() {
            progress.report(i as u64, Some(table_count), &o.name);
            total_rows += dump_rows(s, &tref(o), ddl, opts, out, progress).await?;
        }
        progress.report(table_count, Some(table_count), "");
    }

    if opts.structure {
        writeln!(out, "\n-- Indexes, foreign keys and counters")?;
        for (_, ddl) in &tables {
            for after in &ddl.after {
                statement(out, d, after)?;
            }
        }
        for (o, ddl) in &views {
            writeln!(out, "\n-- View {}", o.name)?;
            for create in &ddl.create {
                statement(out, d, create)?;
            }
        }
    }

    for line in epilogue(d) {
        statement(out, d, line)?;
    }
    Ok(total_rows)
}

/// Views that read other views must come after them. Dependencies are
/// found by name in each view's definition; on a cycle the rest keep their
/// original order.
fn order_views(views: Vec<(&DbObject, ObjectDdl)>) -> Vec<(&DbObject, ObjectDdl)> {
    let n = views.len();
    let text: Vec<String> = views
        .iter()
        .map(|(_, d)| d.create.join("\n").to_lowercase())
        .collect();
    let names: Vec<String> = views.iter().map(|(o, _)| o.name.to_lowercase()).collect();
    let deps: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| j != i && mentions(&text[i], &names[j]))
                .collect()
        })
        .collect();
    let mut placed = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let ready = (0..n).find(|&i| !placed[i] && deps[i].iter().all(|&j| placed[j]));
        let next = ready.unwrap_or_else(|| (0..n).find(|&i| !placed[i]).unwrap_or(0));
        placed[next] = true;
        order.push(next);
    }
    let mut slots: Vec<Option<(&DbObject, ObjectDdl)>> = views.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// `name` appears in `text` as a whole identifier.
fn mentions(text: &str, name: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

async fn dump_rows<W: Write + Send>(
    s: &Session,
    t: &TableRef,
    ddl: &ObjectDdl,
    opts: &DumpOptions,
    out: &mut W,
    progress: &dyn Progress,
) -> AppResult<u64> {
    let d = s.dialect();
    let columns: Vec<String> = meta::columns(s, t)
        .await?
        .into_iter()
        .map(|c| c.name)
        .filter(|c| !ddl.skip_columns.contains(c))
        .collect();
    if columns.is_empty() {
        return Ok(0);
    }
    let list = columns
        .iter()
        .map(|c| quote_ident(d, c))
        .collect::<Vec<_>>()
        .join(", ");
    let name = dump_name(d, t);
    let overriding = if d == Dialect::Postgres && ddl.has_identity {
        "OVERRIDING SYSTEM VALUE "
    } else {
        ""
    };
    let prefix = format!("INSERT INTO {name} ({list}) {overriding}VALUES");
    // SQL Server allows at most 1000 rows in one VALUES list.
    let per_insert = opts
        .rows_per_insert
        .clamp(1, if d == Dialect::Mssql { 1000 } else { 10_000 });
    let identity_insert = d == Dialect::Mssql && ddl.has_identity;

    writeln!(out, "\n-- Data for {}", t.table)?;
    if identity_insert {
        statement(out, d, &format!("SET IDENTITY_INSERT {name} ON"))?;
    }
    let mut batch: Vec<String> = Vec::with_capacity(per_insert);
    let mut rows: u64 = 0;
    let select = format!("SELECT {list} FROM {}", qualified_table(d, t));
    {
        let mut on_row = |values: Vec<DumpValue>| -> AppResult<()> {
            let tuple = values
                .iter()
                .map(|v| sql_literal(d, v))
                .collect::<Vec<_>>()
                .join(", ");
            batch.push(format!("({tuple})"));
            rows += 1;
            if batch.len() >= per_insert {
                flush(out, d, &prefix, &mut batch)?;
                if rows % 10_000 < per_insert as u64 {
                    progress.report(rows, None, &format!("{} — {} rows", t.table, rows));
                }
                check(progress)?;
            }
            Ok(())
        };
        s.stream_select(t.database.as_deref(), &select, &mut on_row)
            .await?;
    }
    flush(out, d, &prefix, &mut batch)?;
    if identity_insert {
        statement(out, d, &format!("SET IDENTITY_INSERT {name} OFF"))?;
    }
    Ok(rows)
}

fn flush<W: Write>(
    out: &mut W,
    d: Dialect,
    prefix: &str,
    batch: &mut Vec<String>,
) -> std::io::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    write!(out, "{prefix}\n  {}", batch.join(",\n  "))?;
    batch.clear();
    end_statement(out, d)
}

fn statement<W: Write>(out: &mut W, d: Dialect, sql: &str) -> std::io::Result<()> {
    let sql = sql.trim().trim_end_matches(';');
    if sql.is_empty() {
        return Ok(());
    }
    write!(out, "{sql}")?;
    end_statement(out, d)
}

/// `;` everywhere; SQL Server scripts also get `GO`, which CREATE VIEW needs
/// (it must start a batch) and which keeps batches small.
fn end_statement<W: Write>(out: &mut W, d: Dialect) -> std::io::Result<()> {
    if d == Dialect::Mssql {
        writeln!(out, ";\nGO")
    } else {
        writeln!(out, ";")
    }
}

fn prologue(d: Dialect) -> &'static [&'static str] {
    match d {
        Dialect::Mysql => &[
            "SET NAMES utf8mb4",
            "SET FOREIGN_KEY_CHECKS = 0",
            // Keep explicit 0 values in AUTO_INCREMENT columns.
            "SET SESSION sql_mode = CONCAT_WS(',', NULLIF(@@SESSION.sql_mode, ''), 'NO_AUTO_VALUE_ON_ZERO')",
        ],
        Dialect::Sqlite => &["PRAGMA foreign_keys = OFF", "BEGIN"],
        Dialect::Postgres => &["SET client_encoding = 'UTF8'", "BEGIN"],
        Dialect::Mssql => &["SET NOCOUNT ON"],
    }
}

fn epilogue(d: Dialect) -> &'static [&'static str] {
    match d {
        Dialect::Mysql => &["SET FOREIGN_KEY_CHECKS = 1"],
        Dialect::Sqlite => &["COMMIT", "PRAGMA foreign_keys = ON"],
        Dialect::Postgres => &["COMMIT"],
        Dialect::Mssql => &[],
    }
}

fn is_table(o: &DbObject) -> bool {
    o.kind == "table" || o.kind == "foreign_table"
}

fn check(progress: &dyn Progress) -> AppResult<()> {
    if progress.cancelled() {
        Err(bulk::cancelled())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(name: &str, sql: &str) -> (DbObject, ObjectDdl) {
        let ddl = ObjectDdl {
            create: vec![sql.into()],
            ..ObjectDdl::default()
        };
        (
            DbObject {
                name: name.into(),
                kind: "view".into(),
            },
            ddl,
        )
    }

    #[test]
    fn orders_views_by_what_they_read() {
        let owned = [
            view("a_v", "CREATE VIEW a_v AS SELECT * FROM \"base_v\""),
            view("base_v", "CREATE VIEW base_v AS SELECT * FROM t"),
            view("other", "CREATE VIEW other AS SELECT * FROM base_vx"),
        ];
        let input = owned
            .iter()
            .map(|(o, d)| {
                (
                    o,
                    ObjectDdl {
                        create: d.create.clone(),
                        ..ObjectDdl::default()
                    },
                )
            })
            .collect();
        let names: Vec<&str> = order_views(input)
            .iter()
            .map(|(o, _)| o.name.as_str())
            .collect();
        assert_eq!(names, ["base_v", "a_v", "other"]);
    }

    #[test]
    fn mentions_whole_identifiers_only() {
        assert!(mentions("from `shop_v` join", "shop_v"));
        assert!(!mentions("from shop_view", "shop_v"));
        assert!(!mentions("from xshop_v", "shop_v"));
    }
}
