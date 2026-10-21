//! Schema statements for SQL dumps.
//!
//! Each object yields statements to create it and statements that must wait
//! until every table exists and holds its data (foreign keys, indexes,
//! sequence positions). Names are written without the database part so a
//! dump restores into whichever database is current.

use crate::db::{meta, Session};
use crate::error::AppResult;
use crate::model::{DbObject, QueryContext, TableRef};
use crate::sql::builder::{qualified_table, quote_ident, quote_literal};
use crate::sql::Dialect;

#[derive(Debug, Default)]
pub struct ObjectDdl {
    pub drop: String,
    pub create: Vec<String>,
    /// Run after all tables are created and filled.
    pub after: Vec<String>,
    /// Postgres: the table has an identity/serial column (inserts need
    /// OVERRIDING SYSTEM VALUE). SQL Server: needs IDENTITY_INSERT.
    pub has_identity: bool,
    /// Columns the database computes itself; they can't be inserted.
    pub skip_columns: Vec<String>,
    /// SQL Server: drop foreign keys pointing at this table before DROP.
    pub pre_drop: Vec<String>,
}

/// Schema-level statements: run first (create schema, search path) and the
/// types tables depend on (Postgres enums), with their drops.
#[derive(Debug, Default)]
pub struct SchemaPrelude {
    pub setup: Vec<String>,
    pub types: Vec<String>,
    pub drop_types: Vec<String>,
}

pub async fn schema_prelude(
    s: &Session,
    database: Option<&str>,
    schema: Option<&str>,
) -> AppResult<SchemaPrelude> {
    let Some(schema) = schema.filter(|sc| !sc.is_empty()) else {
        return Ok(SchemaPrelude::default());
    };
    let d = s.dialect();
    let mut prelude = SchemaPrelude::default();
    match d {
        Dialect::Postgres => {
            if schema != "public" {
                prelude.setup.push(format!(
                    "CREATE SCHEMA IF NOT EXISTS {}",
                    quote_ident(d, schema)
                ));
            }
            // View and default definitions are written relative to this path.
            prelude.setup.push(format!(
                "SET search_path TO {}, public",
                quote_ident(d, schema)
            ));
            let c = QueryContext {
                database: database.map(String::from),
                schema: Some(schema.to_string()),
            };
            let enums = s
                .query(&c, &format!(
                    "SELECT format('CREATE TYPE %I.%I AS ENUM (%s)', n.nspname, t.typname, \
                       string_agg(quote_literal(e.enumlabel), ', ' ORDER BY e.enumsortorder)), \
                     format('DROP TYPE IF EXISTS %I.%I CASCADE', n.nspname, t.typname) \
                     FROM pg_type t JOIN pg_enum e ON e.enumtypid = t.oid JOIN pg_namespace n ON n.oid = t.typnamespace \
                     WHERE n.nspname = {} GROUP BY n.nspname, t.typname ORDER BY t.typname",
                    quote_literal(d, schema)
                ))
                .await?;
            prelude.types = first_cells(&enums.rows, 0);
            prelude.drop_types = first_cells(&enums.rows, 1);
        }
        Dialect::Mssql if !schema.eq_ignore_ascii_case("dbo") => {
            let create = quote_literal(d, &format!("CREATE SCHEMA {}", quote_ident(d, schema)));
            prelude.setup.push(format!(
                "IF SCHEMA_ID({}) IS NULL EXEC({create})",
                quote_literal(d, schema)
            ));
        }
        _ => {}
    }
    Ok(prelude)
}

/// The name used inside the dump (no database part).
pub fn dump_name(d: Dialect, t: &TableRef) -> String {
    let local = TableRef {
        database: None,
        schema: t.schema.clone(),
        table: t.table.clone(),
    };
    qualified_table(d, &local)
}

pub async fn object_ddl(s: &Session, t: &TableRef, object: &DbObject) -> AppResult<ObjectDdl> {
    let is_view = object.kind != "table" && object.kind != "foreign_table";
    match s.dialect() {
        Dialect::Mysql => mysql(s, t, is_view).await,
        Dialect::Sqlite => sqlite(s, t, is_view).await,
        Dialect::Postgres => postgres(s, t, object).await,
        Dialect::Mssql => mssql(s, t, is_view).await,
    }
}

fn ctx(t: &TableRef) -> QueryContext {
    QueryContext {
        database: t.database.clone(),
        schema: t.schema.clone(),
    }
}

fn first_cells(rows: &[Vec<Option<String>>], col: usize) -> Vec<String> {
    rows.iter()
        .filter_map(|r| r.get(col).cloned().flatten())
        .collect()
}

async fn mysql(s: &Session, t: &TableRef, is_view: bool) -> AppResult<ObjectDdl> {
    let d = Dialect::Mysql;
    let name = dump_name(d, t);
    let (keyword, show) = if is_view {
        ("VIEW", "SHOW CREATE VIEW")
    } else {
        ("TABLE", "SHOW CREATE TABLE")
    };
    let set = s
        .query(&ctx(t), &format!("{show} {}", qualified_table(d, t)))
        .await?;
    let create = first_cells(&set.rows, 1)
        .into_iter()
        .next()
        .unwrap_or_default();
    let generated = s
        .query(&ctx(t), &format!(
            "SELECT column_name FROM information_schema.columns WHERE table_schema = {} AND table_name = {} \
             AND extra IN ('VIRTUAL GENERATED', 'STORED GENERATED')",
            quote_literal(d, t.database.as_deref().unwrap_or_default()),
            quote_literal(d, &t.table)
        ))
        .await?;
    let mut create = strip_definer(&create);
    if is_view {
        // SHOW CREATE VIEW qualifies names with the source database; drop
        // that so the view reads from whichever database it is restored into.
        if let Some(db) = t.database.as_deref() {
            create = create.replace(&format!("{}.", quote_ident(d, db)), "");
        }
    }
    Ok(ObjectDdl {
        drop: format!("DROP {keyword} IF EXISTS {name}"),
        create: vec![create],
        skip_columns: first_cells(&generated.rows, 0),
        ..ObjectDdl::default()
    })
}

/// `CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`%` SQL SECURITY DEFINER VIEW`
/// would fail on a server without that account.
fn strip_definer(sql: &str) -> String {
    let Some(start) = sql.find("DEFINER=") else {
        return sql.to_string();
    };
    let rest = &sql[start + "DEFINER=".len()..];
    // `user`@`host` — skip the two backquoted parts and the @ between them.
    let mut end = 0;
    let mut quotes = 0;
    for (i, c) in rest.char_indices() {
        if c == '`' {
            quotes += 1;
        }
        if quotes == 4 {
            end = i + 1;
            break;
        }
    }
    if end == 0 {
        return sql.to_string();
    }
    format!("{}{}", &sql[..start], rest[end..].trim_start())
}

async fn sqlite(s: &Session, t: &TableRef, is_view: bool) -> AppResult<ObjectDdl> {
    let d = Dialect::Sqlite;
    let schema = t.database.as_deref().unwrap_or("main");
    let sql = format!(
        "SELECT type, sql FROM {}.sqlite_master WHERE tbl_name = {} AND sql IS NOT NULL \
         ORDER BY type NOT IN ('table', 'view'), name",
        quote_ident(d, schema),
        quote_literal(d, &t.table)
    );
    let set = s.query(&ctx(t), &sql).await?;
    let mut ddl = ObjectDdl {
        drop: format!(
            "DROP {} IF EXISTS {}",
            if is_view { "VIEW" } else { "TABLE" },
            dump_name(d, t)
        ),
        ..ObjectDdl::default()
    };
    for row in &set.rows {
        let kind = row.first().cloned().flatten().unwrap_or_default();
        let text = row.get(1).cloned().flatten().unwrap_or_default();
        if kind == "table" || kind == "view" {
            ddl.create.push(text);
        } else {
            ddl.after.push(text); // indexes and triggers: faster after the data
        }
    }
    Ok(ddl)
}

async fn postgres(s: &Session, t: &TableRef, object: &DbObject) -> AppResult<ObjectDdl> {
    let d = Dialect::Postgres;
    let name = dump_name(d, t);
    let regclass = format!("{}::regclass", quote_literal(d, &qualified_table(d, t)));
    let c = ctx(t);
    if object.kind == "view" || object.kind == "materialized_view" {
        let def = s
            .query_value(&c, &format!("SELECT pg_get_viewdef({regclass}, true)"))
            .await?
            .unwrap_or_default();
        let keyword = if object.kind == "view" {
            "VIEW"
        } else {
            "MATERIALIZED VIEW"
        };
        return Ok(ObjectDdl {
            drop: format!("DROP {keyword} IF EXISTS {name} CASCADE"),
            create: vec![format!(
                "CREATE {keyword} {name} AS\n{}",
                def.trim_end().trim_end_matches(';')
            )],
            ..ObjectDdl::default()
        });
    }

    // name, type, not null, default, identity ('', 'a', 'd'), generated ('', 's')
    let cols = s
        .query(&c, &format!(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull::int, \
             pg_get_expr(ad.adbin, ad.adrelid), a.attidentity::text, a.attgenerated::text \
             FROM pg_attribute a LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
             WHERE a.attrelid = {regclass} AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum"
        ))
        .await?;
    let mut lines = Vec::new();
    let mut counters = Vec::new();
    let mut skip_columns = Vec::new();
    for row in &cols.rows {
        let get = |i: usize| row.get(i).cloned().flatten().unwrap_or_default();
        let (col, ty, not_null, default, identity) =
            (get(0), get(1), get(2) == "1", get(3), get(4));
        if get(5) == "s" {
            lines.push(format!(
                "  {} {ty} GENERATED ALWAYS AS ({default}) STORED",
                quote_ident(d, &col)
            ));
            skip_columns.push(col);
            continue;
        }
        let serial = default.starts_with("nextval(");
        let mut line = format!("  {} ", quote_ident(d, &col));
        match (identity.as_str(), serial, ty.as_str()) {
            ("a", _, _) => line.push_str(&format!("{ty} GENERATED ALWAYS AS IDENTITY")),
            ("d", _, _) => line.push_str(&format!("{ty} GENERATED BY DEFAULT AS IDENTITY")),
            (_, true, "integer") => line.push_str("serial"),
            (_, true, "bigint") => line.push_str("bigserial"),
            (_, true, "smallint") => line.push_str("smallserial"),
            _ => line.push_str(&ty),
        }
        let counter = !identity.is_empty() || serial;
        if counter {
            counters.push(col.clone());
        } else if !default.is_empty() {
            line.push_str(&format!(" DEFAULT {default}"));
        }
        if not_null && !counter {
            line.push_str(" NOT NULL");
        }
        lines.push(line);
    }
    // Primary key, unique, check and exclusion constraints stay inline.
    let inline = s
        .query(&c, &format!(
            "SELECT 'CONSTRAINT ' || quote_ident(conname) || ' ' || pg_get_constraintdef(oid) FROM pg_constraint \
             WHERE conrelid = {regclass} AND contype IN ('p', 'u', 'c', 'x') ORDER BY contype, conname"
        ))
        .await?;
    lines.extend(
        first_cells(&inline.rows, 0)
            .into_iter()
            .map(|l| format!("  {l}")),
    );

    let mut after: Vec<String> = Vec::new();
    let indexes = s
        .query(&c, &format!(
            "SELECT pg_get_indexdef(indexrelid) FROM pg_index WHERE indrelid = {regclass} AND indexrelid NOT IN \
             (SELECT conindid FROM pg_constraint WHERE conrelid = {regclass}) ORDER BY indexrelid"
        ))
        .await?;
    after.extend(first_cells(&indexes.rows, 0));
    let fks = s
        .query(&c, &format!(
            "SELECT 'ALTER TABLE {} ADD CONSTRAINT ' || quote_ident(conname) || ' ' || pg_get_constraintdef(oid) \
             FROM pg_constraint WHERE conrelid = {regclass} AND contype = 'f' ORDER BY conname",
            name.replace('\'', "''")
        ))
        .await?;
    after.extend(first_cells(&fks.rows, 0));
    // Continue counters after the highest value restored.
    for col in &counters {
        let q = quote_ident(d, col);
        after.push(format!(
            "SELECT setval(pg_get_serial_sequence({}, {}), COALESCE(MAX({q}), 1), MAX({q}) IS NOT NULL) FROM {name}",
            quote_literal(d, &name),
            quote_literal(d, col)
        ));
    }
    Ok(ObjectDdl {
        drop: format!("DROP TABLE IF EXISTS {name} CASCADE"),
        create: vec![format!("CREATE TABLE {name} (\n{}\n)", lines.join(",\n"))],
        after,
        has_identity: !counters.is_empty(),
        skip_columns,
        ..ObjectDdl::default()
    })
}

async fn mssql(s: &Session, t: &TableRef, is_view: bool) -> AppResult<ObjectDdl> {
    let d = Dialect::Mssql;
    let name = dump_name(d, t);
    let full = quote_literal(d, &qualified_table(d, t));
    let c = ctx(t);
    let kind = if is_view { "V" } else { "U" };
    let drop = format!(
        "IF OBJECT_ID({}, N'{kind}') IS NOT NULL DROP {} {name}",
        quote_literal(d, &name),
        if is_view { "VIEW" } else { "TABLE" }
    );
    if is_view {
        let def = s
            .query_value(&c, &format!("SELECT OBJECT_DEFINITION(OBJECT_ID({full}))"))
            .await?;
        return Ok(ObjectDdl {
            drop,
            create: vec![def.unwrap_or_default()],
            ..ObjectDdl::default()
        });
    }
    let columns = meta::columns(s, t).await?;
    let has_identity = columns.iter().any(|c| c.is_auto_increment);
    // rowversion/timestamp values are assigned by the server.
    let skip_columns = columns
        .iter()
        .filter(|c| matches!(c.data_type.as_str(), "timestamp" | "rowversion"))
        .map(|c| c.name.clone())
        .collect();
    let create = meta::reconstructed_ddl(d, &name, &columns, &[]);
    let fks = s.query(&c, &mssql_fk_sql(&full)).await?;
    let indexes = s.query(&c, &mssql_index_sql(&full, &name)).await?;
    let mut after = first_cells(&fks.rows, 0);
    after.splice(0..0, first_cells(&indexes.rows, 0));
    let pre_drop = vec![format!(
        "DECLARE @dbku_fk nvarchar(max) = N''; \
         SELECT @dbku_fk = @dbku_fk + N'ALTER TABLE ' + QUOTENAME(OBJECT_SCHEMA_NAME(parent_object_id)) + N'.' + \
         QUOTENAME(OBJECT_NAME(parent_object_id)) + N' DROP CONSTRAINT ' + QUOTENAME(name) + N'; ' \
         FROM sys.foreign_keys WHERE referenced_object_id = OBJECT_ID({}); EXEC sp_executesql @dbku_fk",
        quote_literal(d, &name)
    )];
    Ok(ObjectDdl {
        drop,
        pre_drop,
        create: vec![create.trim_end_matches(';').to_string()],
        after,
        has_identity,
        skip_columns,
    })
}

fn mssql_fk_sql(full: &str) -> String {
    let cols = |side: &str| {
        format!(
            "STUFF((SELECT ', ' + QUOTENAME(c.name) FROM sys.foreign_key_columns fkc \
             JOIN sys.columns c ON c.object_id = fkc.{side}_object_id AND c.column_id = fkc.{side}_column_id \
             WHERE fkc.constraint_object_id = fk.object_id ORDER BY fkc.constraint_column_id FOR XML PATH('')), 1, 2, '')"
        )
    };
    let action = |col: &str, verb: &str| {
        format!(
            "CASE fk.{col} WHEN 1 THEN ' ON {verb} CASCADE' WHEN 2 THEN ' ON {verb} SET NULL' \
             WHEN 3 THEN ' ON {verb} SET DEFAULT' ELSE '' END"
        )
    };
    format!(
        "SELECT 'ALTER TABLE ' + QUOTENAME(OBJECT_SCHEMA_NAME(fk.parent_object_id)) + '.' + \
         QUOTENAME(OBJECT_NAME(fk.parent_object_id)) + ' ADD CONSTRAINT ' + QUOTENAME(fk.name) + \
         ' FOREIGN KEY (' + {} + ') REFERENCES ' + QUOTENAME(SCHEMA_NAME(rt.schema_id)) + '.' + \
         QUOTENAME(rt.name) + ' (' + {} + ')' + {} + {} \
         FROM sys.foreign_keys fk JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id \
         WHERE fk.parent_object_id = OBJECT_ID({full}) ORDER BY fk.name",
        cols("parent"),
        cols("referenced"),
        action("delete_referential_action", "DELETE"),
        action("update_referential_action", "UPDATE"),
    )
}

fn mssql_index_sql(full: &str, name: &str) -> String {
    let key_cols = |included: u8| {
        format!(
            "STUFF((SELECT ', ' + QUOTENAME(c.name) + CASE WHEN ic.is_descending_key = 1 AND {included} = 0 THEN ' DESC' ELSE '' END \
             FROM sys.index_columns ic JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
             WHERE ic.object_id = i.object_id AND ic.index_id = i.index_id AND ic.is_included_column = {included} \
             ORDER BY ic.key_ordinal, ic.index_column_id FOR XML PATH('')), 1, 2, '')"
        )
    };
    format!(
        "SELECT 'CREATE ' + CASE WHEN i.is_unique = 1 THEN 'UNIQUE ' ELSE '' END + \
         CASE WHEN i.type = 1 THEN 'CLUSTERED ' ELSE 'NONCLUSTERED ' END + 'INDEX ' + QUOTENAME(i.name) + \
         ' ON {} (' + {} + ')' + ISNULL(' INCLUDE (' + {} + ')', '') + \
         CASE WHEN i.has_filter = 1 THEN ' WHERE ' + i.filter_definition ELSE '' END \
         FROM sys.indexes i WHERE i.object_id = OBJECT_ID({full}) AND i.is_primary_key = 0 \
         AND i.type IN (1, 2) AND i.is_hypothetical = 0 ORDER BY i.name",
        name.replace('\'', "''"),
        key_cols(0),
        key_cols(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_mysql_definer() {
        let view = "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`%` SQL SECURITY DEFINER VIEW `v` AS select 1";
        assert_eq!(
            strip_definer(view),
            "CREATE ALGORITHM=UNDEFINED SQL SECURITY DEFINER VIEW `v` AS select 1"
        );
        assert_eq!(
            strip_definer("CREATE TABLE `t` (a int)"),
            "CREATE TABLE `t` (a int)"
        );
    }

    #[test]
    fn dump_names_drop_the_database() {
        let t = TableRef {
            database: Some("shop".into()),
            schema: Some("dbo".into()),
            table: "Order".into(),
        };
        assert_eq!(dump_name(Dialect::Mssql, &t), "[dbo].[Order]");
        assert_eq!(dump_name(Dialect::Mysql, &t), "`Order`");
    }
}
