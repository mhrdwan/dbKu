//! Schema introspection. Every query returns its columns in a fixed order
//! so the row mapping below is shared by all databases. Names are embedded
//! as escaped literals (see `sql::builder::quote_literal`).

use super::Session;
use crate::error::AppResult;
use crate::model::{Cell, ColumnInfo, DbObject, IndexInfo, QueryContext, SearchHit, TableRef};
use crate::sql::builder::{qualified_table, quote_ident, quote_literal};
use crate::sql::Dialect;

const SEARCH_LIMIT: u32 = 500;

fn ctx(database: Option<&str>, schema: Option<&str>) -> QueryContext {
    QueryContext {
        database: database.map(String::from),
        schema: schema.map(String::from),
    }
}

fn text(row: &[Cell], i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

fn opt(row: &[Cell], i: usize) -> Option<String> {
    row.get(i).cloned().flatten()
}

fn flag(row: &[Cell], i: usize) -> bool {
    matches!(
        row.get(i).cloned().flatten().as_deref().map(str::trim),
        Some("1" | "t" | "true")
    )
}

async fn first_column(s: &Session, c: &QueryContext, sql: &str) -> AppResult<Vec<String>> {
    let set = s.query(c, sql).await?;
    Ok(set.rows.iter().map(|r| text(r, 0)).collect())
}

pub async fn databases(s: &Session) -> AppResult<Vec<String>> {
    let sql = match s.dialect() {
        Dialect::Postgres => {
            "SELECT datname FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname"
        }
        Dialect::Mysql => "SELECT schema_name FROM information_schema.schemata ORDER BY schema_name",
        Dialect::Sqlite => "SELECT name FROM pragma_database_list ORDER BY seq",
        Dialect::Mssql => {
            "SELECT name FROM sys.databases WHERE HAS_DBACCESS(name) = 1 AND state = 0 ORDER BY name"
        }
    };
    first_column(s, &QueryContext::default(), sql).await
}

/// Schemas inside a database. MySQL and SQLite have none (the database is
/// the namespace), so they return an empty list.
pub async fn schemas(s: &Session, database: &str) -> AppResult<Vec<String>> {
    let sql = match s.dialect() {
        Dialect::Postgres => {
            "SELECT nspname FROM pg_namespace WHERE nspname !~ '^pg_' \
             AND nspname <> 'information_schema' ORDER BY nspname = 'public' DESC, nspname"
        }
        Dialect::Mssql => {
            "SELECT name FROM sys.schemas WHERE schema_id NOT BETWEEN 16384 AND 16399 \
             AND name NOT IN ('sys', 'INFORMATION_SCHEMA', 'guest') ORDER BY name"
        }
        Dialect::Mysql | Dialect::Sqlite => return Ok(Vec::new()),
    };
    first_column(s, &ctx(Some(database), None), sql).await
}

pub async fn objects(
    s: &Session,
    database: &str,
    schema: Option<&str>,
) -> AppResult<Vec<DbObject>> {
    let d = s.dialect();
    let sql = match d {
        Dialect::Postgres => format!(
            "SELECT c.relname, CASE c.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view' \
             WHEN 'f' THEN 'foreign_table' ELSE 'table' END \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {} AND c.relkind IN ('r','p','v','m','f') AND NOT c.relispartition \
             ORDER BY c.relname",
            quote_literal(d, schema.unwrap_or("public"))
        ),
        Dialect::Mysql => format!(
            "SELECT table_name, CASE WHEN table_type = 'VIEW' THEN 'view' ELSE 'table' END \
             FROM information_schema.tables WHERE table_schema = {} ORDER BY table_name",
            quote_literal(d, database)
        ),
        Dialect::Sqlite => format!(
            "SELECT name, type FROM {}.sqlite_master WHERE type IN ('table', 'view') \
             AND name NOT LIKE 'sqlite!_%' ESCAPE '!' ORDER BY name",
            quote_ident(d, database)
        ),
        Dialect::Mssql => format!(
            "SELECT o.name, CASE WHEN o.type = 'V' THEN 'view' ELSE 'table' END \
             FROM sys.objects o JOIN sys.schemas s ON s.schema_id = o.schema_id \
             WHERE s.name = {} AND o.type IN ('U', 'V') AND o.is_ms_shipped = 0 ORDER BY o.name",
            quote_literal(d, schema.unwrap_or("dbo"))
        ),
    };
    let set = s.query(&ctx(Some(database), schema), &sql).await?;
    Ok(set
        .rows
        .iter()
        .map(|r| DbObject {
            name: text(r, 0),
            kind: text(r, 1),
        })
        .collect())
}

pub async fn columns(s: &Session, t: &TableRef) -> AppResult<Vec<ColumnInfo>> {
    let d = s.dialect();
    let table_lit = quote_literal(d, &t.table);
    let sql = match d {
        Dialect::Postgres => format!(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), \
             CASE WHEN a.attnotnull THEN 0 ELSE 1 END, pg_get_expr(ad.adbin, ad.adrelid), \
             CASE WHEN EXISTS (SELECT 1 FROM pg_index i WHERE i.indrelid = a.attrelid \
               AND i.indisprimary AND a.attnum = ANY(i.indkey)) THEN 1 ELSE 0 END, \
             CASE WHEN a.attidentity <> '' OR pg_get_expr(ad.adbin, ad.adrelid) LIKE 'nextval(%' \
               THEN 1 ELSE 0 END, \
             col_description(a.attrelid, a.attnum) \
             FROM pg_attribute a LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
             WHERE a.attrelid = {}::regclass AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
            quote_literal(d, &qualified_table(d, t))
        ),
        Dialect::Mysql => format!(
            "SELECT column_name, column_type, CASE WHEN is_nullable = 'YES' THEN 1 ELSE 0 END, \
             column_default, CASE WHEN column_key = 'PRI' THEN 1 ELSE 0 END, \
             CASE WHEN extra LIKE '%auto_increment%' THEN 1 ELSE 0 END, NULLIF(column_comment, '') \
             FROM information_schema.columns WHERE table_schema = {} AND table_name = {table_lit} \
             ORDER BY ordinal_position",
            quote_literal(d, t.database.as_deref().unwrap_or_default())
        ),
        Dialect::Sqlite => format!(
            "SELECT name, type, CASE WHEN \"notnull\" THEN 0 ELSE 1 END, dflt_value, \
             CASE WHEN pk > 0 THEN 1 ELSE 0 END, \
             CASE WHEN pk = 1 AND upper(type) = 'INTEGER' THEN 1 ELSE 0 END, NULL \
             FROM pragma_table_info({table_lit}, {}) ORDER BY cid",
            quote_literal(d, t.database.as_deref().unwrap_or("main"))
        ),
        Dialect::Mssql => format!(
            "SELECT c.name, TYPE_NAME(c.user_type_id) + CASE \
               WHEN TYPE_NAME(c.user_type_id) IN ('varchar','char','varbinary','binary') \
                 THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length AS varchar(10)) END + ')' \
               WHEN TYPE_NAME(c.user_type_id) IN ('nvarchar','nchar') \
                 THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length / 2 AS varchar(10)) END + ')' \
               WHEN TYPE_NAME(c.user_type_id) IN ('decimal','numeric') \
                 THEN '(' + CAST(c.precision AS varchar(5)) + ',' + CAST(c.scale AS varchar(5)) + ')' \
               ELSE '' END, \
             CASE WHEN c.is_nullable = 1 THEN 1 ELSE 0 END, dc.definition, \
             CASE WHEN EXISTS (SELECT 1 FROM sys.index_columns ic JOIN sys.indexes i \
               ON i.object_id = ic.object_id AND i.index_id = ic.index_id \
               WHERE i.is_primary_key = 1 AND ic.object_id = c.object_id AND ic.column_id = c.column_id) \
               THEN 1 ELSE 0 END, \
             CASE WHEN c.is_identity = 1 THEN 1 ELSE 0 END, CAST(ep.value AS nvarchar(max)) \
             FROM sys.columns c \
             LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id \
             LEFT JOIN sys.extended_properties ep ON ep.major_id = c.object_id \
               AND ep.minor_id = c.column_id AND ep.name = 'MS_Description' AND ep.class = 1 \
             WHERE c.object_id = OBJECT_ID({}) ORDER BY c.column_id",
            quote_literal(d, &qualified_table(d, t))
        ),
    };
    let set = s
        .query(&ctx(t.database.as_deref(), t.schema.as_deref()), &sql)
        .await?;
    Ok(set
        .rows
        .iter()
        .map(|r| ColumnInfo {
            name: text(r, 0),
            data_type: text(r, 1),
            nullable: flag(r, 2),
            default_value: opt(r, 3),
            is_primary_key: flag(r, 4),
            is_auto_increment: flag(r, 5),
            comment: opt(r, 6),
        })
        .collect())
}

pub async fn indexes(s: &Session, t: &TableRef) -> AppResult<Vec<IndexInfo>> {
    let d = s.dialect();
    let table_lit = quote_literal(d, &t.table);
    let sql = match d {
        Dialect::Postgres => format!(
            "SELECT i.relname, array_to_string(ARRAY(SELECT pg_get_indexdef(x.indexrelid, k + 1, true) \
               FROM generate_subscripts(x.indkey, 1) AS k ORDER BY k), ','), \
             CASE WHEN x.indisunique THEN 1 ELSE 0 END, CASE WHEN x.indisprimary THEN 1 ELSE 0 END \
             FROM pg_index x JOIN pg_class i ON i.oid = x.indexrelid \
             WHERE x.indrelid = {}::regclass ORDER BY x.indisprimary DESC, i.relname",
            quote_literal(d, &qualified_table(d, t))
        ),
        Dialect::Mysql => format!(
            "SELECT index_name, GROUP_CONCAT(column_name ORDER BY seq_in_index SEPARATOR ','), \
             CASE WHEN MIN(non_unique) = 0 THEN 1 ELSE 0 END, \
             CASE WHEN index_name = 'PRIMARY' THEN 1 ELSE 0 END \
             FROM information_schema.statistics WHERE table_schema = {} AND table_name = {table_lit} \
             GROUP BY index_name ORDER BY index_name = 'PRIMARY' DESC, index_name",
            quote_literal(d, t.database.as_deref().unwrap_or_default())
        ),
        Dialect::Sqlite => {
            let schema = quote_literal(d, t.database.as_deref().unwrap_or("main"));
            format!(
                "SELECT il.name, (SELECT group_concat(ii.name, ',') FROM pragma_index_info(il.name, {schema}) ii), \
                 il.\"unique\", CASE WHEN il.origin = 'pk' THEN 1 ELSE 0 END \
                 FROM pragma_index_list({table_lit}, {schema}) il ORDER BY il.name"
            )
        }
        Dialect::Mssql => format!(
            "SELECT i.name, STUFF((SELECT ',' + c.name FROM sys.index_columns ic \
               JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
               WHERE ic.object_id = i.object_id AND ic.index_id = i.index_id AND ic.is_included_column = 0 \
               ORDER BY ic.key_ordinal FOR XML PATH('')), 1, 1, ''), \
             CASE WHEN i.is_unique = 1 THEN 1 ELSE 0 END, CASE WHEN i.is_primary_key = 1 THEN 1 ELSE 0 END \
             FROM sys.indexes i WHERE i.object_id = OBJECT_ID({}) AND i.type > 0 \
             ORDER BY i.is_primary_key DESC, i.name",
            quote_literal(d, &qualified_table(d, t))
        ),
    };
    let set = s
        .query(&ctx(t.database.as_deref(), t.schema.as_deref()), &sql)
        .await?;
    Ok(set
        .rows
        .iter()
        .map(|r| IndexInfo {
            name: text(r, 0),
            columns: text(r, 1)
                .split(',')
                .filter(|c| !c.is_empty())
                .map(String::from)
                .collect(),
            is_unique: flag(r, 2),
            is_primary: flag(r, 3),
        })
        .collect())
}

/// CREATE statement for a table or view. MySQL and SQLite store it; for
/// Postgres and SQL Server it is reconstructed from the catalog.
pub async fn ddl(s: &Session, t: &TableRef) -> AppResult<String> {
    let d = s.dialect();
    let c = ctx(t.database.as_deref(), t.schema.as_deref());
    let name = qualified_table(d, t);
    match d {
        Dialect::Mysql => {
            let set = s.query(&c, &format!("SHOW CREATE TABLE {name}")).await?;
            Ok(set.rows.first().map(|r| text(r, 1)).unwrap_or_default())
        }
        Dialect::Sqlite => {
            let sql = format!(
                "SELECT sql FROM {}.sqlite_master WHERE tbl_name = {} AND sql IS NOT NULL \
                 ORDER BY type NOT IN ('table', 'view'), name",
                quote_ident(d, t.database.as_deref().unwrap_or("main")),
                quote_literal(d, &t.table)
            );
            let parts = first_column(s, &c, &sql).await?;
            Ok(parts
                .iter()
                .map(|p| format!("{p};"))
                .collect::<Vec<_>>()
                .join("\n\n"))
        }
        Dialect::Postgres => postgres_ddl(s, t, &c, &name).await,
        Dialect::Mssql => {
            let view = s
                .query_value(
                    &c,
                    &format!(
                        "SELECT OBJECT_DEFINITION(OBJECT_ID({}))",
                        quote_literal(d, &name)
                    ),
                )
                .await?;
            match view {
                Some(def) => Ok(def),
                None => Ok(reconstructed_ddl(d, &name, &columns(s, t).await?, &[])),
            }
        }
    }
}

async fn postgres_ddl(
    s: &Session,
    t: &TableRef,
    c: &QueryContext,
    name: &str,
) -> AppResult<String> {
    let d = Dialect::Postgres;
    let regclass = format!("{}::regclass", quote_literal(d, name));
    let kind = s
        .query_value(
            c,
            &format!("SELECT relkind::text FROM pg_class WHERE oid = {regclass}"),
        )
        .await?;
    if let Some(k @ ("v" | "m")) = kind.as_deref() {
        let def = s
            .query_value(c, &format!("SELECT pg_get_viewdef({regclass}, true)"))
            .await?;
        let keyword = if k == "m" {
            "MATERIALIZED VIEW"
        } else {
            "VIEW"
        };
        return Ok(format!(
            "CREATE {keyword} {name} AS\n{}",
            def.unwrap_or_default()
        ));
    }
    let constraints = s
        .query(c, &format!(
            "SELECT 'CONSTRAINT ' || quote_ident(conname) || ' ' || pg_get_constraintdef(oid) \
             FROM pg_constraint WHERE conrelid = {regclass} AND contype IN ('f', 'c', 'u', 'x') ORDER BY conname"
        ))
        .await?;
    let extra: Vec<String> = constraints.rows.iter().map(|r| text(r, 0)).collect();
    let mut out = reconstructed_ddl(d, name, &columns(s, t).await?, &extra);
    let indexes = first_column(s, c, &format!(
        "SELECT pg_get_indexdef(indexrelid) FROM pg_index WHERE indrelid = {regclass} \
         AND NOT indisprimary AND indexrelid NOT IN (SELECT conindid FROM pg_constraint WHERE conrelid = {regclass})"
    ))
    .await?;
    for index in indexes {
        out.push_str(&format!("\n{index};"));
    }
    Ok(out)
}

pub fn reconstructed_ddl(d: Dialect, name: &str, cols: &[ColumnInfo], extra: &[String]) -> String {
    let mut lines: Vec<String> = cols
        .iter()
        .map(|c| {
            let mut line = format!("  {} {}", quote_ident(d, &c.name), c.data_type);
            if d == Dialect::Mssql && c.is_auto_increment {
                line.push_str(" IDENTITY(1,1)");
            }
            if !c.nullable {
                line.push_str(" NOT NULL");
            }
            if let Some(def) = &c.default_value {
                line.push_str(&format!(" DEFAULT {def}"));
            }
            line
        })
        .collect();
    let pk: Vec<String> = cols
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| quote_ident(d, &c.name))
        .collect();
    if !pk.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", pk.join(", ")));
    }
    lines.extend(extra.iter().map(|e| format!("  {e}")));
    format!("CREATE TABLE {name} (\n{}\n);", lines.join(",\n"))
}

/// Tables, views and columns whose name contains `term`.
pub async fn search_objects(s: &Session, database: &str, term: &str) -> AppResult<Vec<SearchHit>> {
    let d = s.dialect();
    let escaped: String = term
        .chars()
        .flat_map(|c| {
            let special = matches!(c, '!' | '%' | '_') || (d == Dialect::Mssql && c == '[');
            special.then_some('!').into_iter().chain(std::iter::once(c))
        })
        .collect();
    let pat = quote_literal(d, &format!("%{escaped}%"));
    let sql = match d {
        Dialect::Postgres => format!(
            "SELECT * FROM (SELECT table_schema::text AS s, table_name::text AS t, NULL::text AS c, \
               CASE WHEN table_type = 'VIEW' THEN 'view' ELSE 'table' END AS k \
             FROM information_schema.tables WHERE table_name ILIKE {pat} ESCAPE '!' \
             UNION ALL SELECT table_schema::text, table_name::text, column_name::text, 'column' \
             FROM information_schema.columns WHERE column_name ILIKE {pat} ESCAPE '!') x \
             WHERE s NOT IN ('pg_catalog', 'information_schema') ORDER BY k = 'column', s, t, c LIMIT {SEARCH_LIMIT}"
        ),
        Dialect::Mysql => {
            let db = quote_literal(d, database);
            format!(
                "SELECT * FROM (SELECT NULL AS s, table_name AS t, NULL AS c, \
                   CASE WHEN table_type = 'VIEW' THEN 'view' ELSE 'table' END AS k \
                 FROM information_schema.tables WHERE table_schema = {db} AND table_name LIKE {pat} ESCAPE '!' \
                 UNION ALL SELECT NULL, table_name, column_name, 'column' FROM information_schema.columns \
                 WHERE table_schema = {db} AND column_name LIKE {pat} ESCAPE '!') x \
                 ORDER BY k = 'column', t, c LIMIT {SEARCH_LIMIT}"
            )
        }
        Dialect::Sqlite => {
            let master = format!("{}.sqlite_master", quote_ident(d, database));
            format!(
                "SELECT * FROM (SELECT NULL AS s, m.name AS t, NULL AS c, m.type AS k FROM {master} m \
                 WHERE m.type IN ('table', 'view') AND m.name LIKE {pat} ESCAPE '!' \
                 UNION ALL SELECT NULL, m.name, p.name, 'column' FROM {master} m \
                 JOIN pragma_table_info(m.name) p WHERE m.type IN ('table', 'view') AND p.name LIKE {pat} ESCAPE '!') \
                 ORDER BY k = 'column', t, c LIMIT {SEARCH_LIMIT}"
            )
        }
        Dialect::Mssql => format!(
            "SELECT TOP ({SEARCH_LIMIT}) * FROM (SELECT TABLE_SCHEMA AS s, TABLE_NAME AS t, NULL AS c, \
               CASE WHEN TABLE_TYPE = 'VIEW' THEN 'view' ELSE 'table' END AS k \
             FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_NAME LIKE {pat} ESCAPE '!' \
             UNION ALL SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, 'column' FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE COLUMN_NAME LIKE {pat} ESCAPE '!') x \
             ORDER BY CASE WHEN k = 'column' THEN 1 ELSE 0 END, s, t, c"
        ),
    };
    let set = s.query(&ctx(Some(database), None), &sql).await?;
    Ok(set
        .rows
        .iter()
        .map(|r| SearchHit {
            schema: opt(r, 0),
            table: text(r, 1),
            column: opt(r, 2),
            kind: text(r, 3),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstructs_create_table() {
        let cols = vec![
            ColumnInfo {
                name: "id".into(),
                data_type: "int".into(),
                nullable: false,
                default_value: None,
                is_primary_key: true,
                is_auto_increment: true,
                comment: None,
            },
            ColumnInfo {
                name: "name".into(),
                data_type: "nvarchar(50)".into(),
                nullable: true,
                default_value: Some("(N'x')".into()),
                is_primary_key: false,
                is_auto_increment: false,
                comment: None,
            },
        ];
        assert_eq!(
            reconstructed_ddl(Dialect::Mssql, "[dbo].[t]", &cols, &[]),
            "CREATE TABLE [dbo].[t] (\n  [id] int IDENTITY(1,1) NOT NULL,\n  [name] nvarchar(50) DEFAULT (N'x'),\n  PRIMARY KEY ([id])\n);"
        );
    }

    #[test]
    fn flag_accepts_db_specific_truthy_text() {
        let row = vec![Some("1".into()), Some("t".into()), Some("0".into()), None];
        assert!(flag(&row, 0) && flag(&row, 1));
        assert!(!flag(&row, 2) && !flag(&row, 3));
    }
}
