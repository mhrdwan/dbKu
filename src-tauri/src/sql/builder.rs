//! SQL text generation for the table browser and grid edits.
//!
//! Grid edits are rendered as literal SQL (not bind parameters) on purpose:
//! the UI shows the exact statements before they run, and untyped literals
//! let every database coerce the text into the column's own type.

use super::Dialect;
use crate::model::{Cell, RowChange, SortDir, SortSpec, TableRef};

pub fn quote_ident(d: Dialect, name: &str) -> String {
    match d {
        Dialect::Postgres | Dialect::Sqlite => format!("\"{}\"", name.replace('"', "\"\"")),
        Dialect::Mysql => format!("`{}`", name.replace('`', "``")),
        Dialect::Mssql => format!("[{}]", name.replace(']', "]]")),
    }
}

pub fn quote_literal(d: Dialect, value: &str) -> String {
    match d {
        Dialect::Postgres | Dialect::Sqlite => format!("'{}'", value.replace('\'', "''")),
        Dialect::Mysql => {
            // Default MySQL sql_mode treats backslash as an escape character.
            let escaped = value
                .replace('\\', "\\\\")
                .replace('\'', "''")
                .replace('\0', "\\0");
            format!("'{escaped}'")
        }
        Dialect::Mssql => format!("N'{}'", value.replace('\'', "''")),
    }
}

fn literal_or_null(d: Dialect, cell: &Cell) -> String {
    cell.as_deref()
        .map_or_else(|| "NULL".to_string(), |v| quote_literal(d, v))
}

/// Fully qualified table name. Postgres pools are per-database, so the
/// database is never part of a Postgres name.
pub fn qualified_table(d: Dialect, t: &TableRef) -> String {
    let parts: Vec<&str> = match d {
        Dialect::Postgres => vec![t.schema.as_deref(), Some(&t.table)],
        Dialect::Mysql => vec![t.database.as_deref(), Some(&t.table)],
        // SQLite: `database` is the attached database name (`main`, ...).
        Dialect::Sqlite => vec![
            t.database.as_deref().filter(|s| *s != "main"),
            Some(&t.table),
        ],
        Dialect::Mssql => vec![t.database.as_deref(), t.schema.as_deref(), Some(&t.table)],
    }
    .into_iter()
    .flatten()
    .filter(|p| !p.is_empty())
    .collect();
    parts
        .iter()
        .map(|p| quote_ident(d, p))
        .collect::<Vec<_>>()
        .join(".")
}

pub struct SelectSpec<'a> {
    pub table: &'a TableRef,
    pub sort: &'a [SortSpec],
    pub filter: Option<&'a str>,
    pub search: Option<&'a str>,
    pub search_columns: &'a [String],
    pub limit: u32,
    pub offset: u64,
}

pub fn build_select(d: Dialect, spec: &SelectSpec) -> String {
    let mut sql = format!("SELECT * FROM {}", qualified_table(d, spec.table));
    if let Some(clause) = where_clause(d, spec) {
        sql.push_str(" WHERE ");
        sql.push_str(&clause);
    }
    let order = order_by(d, spec.sort);
    match d {
        Dialect::Mssql => {
            // OFFSET/FETCH needs an ORDER BY; `(SELECT NULL)` keeps natural order.
            let order = order.unwrap_or_else(|| "(SELECT NULL)".to_string());
            sql.push_str(&format!(
                " ORDER BY {order} OFFSET {} ROWS FETCH NEXT {} ROWS ONLY",
                spec.offset, spec.limit
            ));
        }
        _ => {
            if let Some(order) = order {
                sql.push_str(&format!(" ORDER BY {order}"));
            }
            sql.push_str(&format!(" LIMIT {} OFFSET {}", spec.limit, spec.offset));
        }
    }
    sql
}

pub fn build_count(d: Dialect, spec: &SelectSpec) -> String {
    let mut sql = format!("SELECT COUNT(*) FROM {}", qualified_table(d, spec.table));
    if let Some(clause) = where_clause(d, spec) {
        sql.push_str(" WHERE ");
        sql.push_str(&clause);
    }
    sql
}

fn where_clause(d: Dialect, spec: &SelectSpec) -> Option<String> {
    let filter = spec
        .filter
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(|f| format!("({f})"));
    let search = spec
        .search
        .filter(|s| !s.is_empty() && !spec.search_columns.is_empty())
        .map(|s| search_predicate(d, s, spec.search_columns));
    match (filter, search) {
        (Some(f), Some(s)) => Some(format!("{f} AND {s}")),
        (f, s) => f.or(s),
    }
}

/// `(CAST(a AS text) LIKE '%x%' ESCAPE '!' OR ...)` — case-insensitive where
/// the database makes that cheap.
pub fn search_predicate(d: Dialect, term: &str, columns: &[String]) -> String {
    let pattern = quote_literal(d, &format!("%{}%", escape_like(d, term)));
    let tests: Vec<String> = columns
        .iter()
        .map(|c| {
            let col = quote_ident(d, c);
            match d {
                Dialect::Postgres => format!("CAST({col} AS TEXT) ILIKE {pattern} ESCAPE '!'"),
                Dialect::Mysql => format!("CAST({col} AS CHAR) LIKE {pattern} ESCAPE '!'"),
                Dialect::Sqlite => format!("CAST({col} AS TEXT) LIKE {pattern} ESCAPE '!'"),
                Dialect::Mssql => {
                    format!("CAST({col} AS NVARCHAR(MAX)) LIKE {pattern} ESCAPE '!'")
                }
            }
        })
        .collect();
    format!("({})", tests.join(" OR "))
}

fn escape_like(d: Dialect, term: &str) -> String {
    term.chars()
        .fold(String::with_capacity(term.len()), |mut out, c| {
            let special = matches!(c, '!' | '%' | '_') || (d == Dialect::Mssql && c == '[');
            if special {
                out.push('!');
            }
            out.push(c);
            out
        })
}

fn order_by(d: Dialect, sort: &[SortSpec]) -> Option<String> {
    if sort.is_empty() {
        return None;
    }
    let parts: Vec<String> = sort
        .iter()
        .map(|s| {
            let dir = match s.dir {
                SortDir::Asc => "ASC",
                SortDir::Desc => "DESC",
            };
            format!("{} {dir}", quote_ident(d, &s.column))
        })
        .collect();
    Some(parts.join(", "))
}

pub fn build_change(d: Dialect, table: &TableRef, change: &RowChange) -> Result<String, String> {
    let name = qualified_table(d, table);
    match change {
        RowChange::Update { key, values } => {
            if values.is_empty() {
                return Err("update has no changed columns".into());
            }
            let set: Vec<String> = values
                .iter()
                .map(|(c, v)| format!("{} = {}", quote_ident(d, c), literal_or_null(d, v)))
                .collect();
            Ok(format!(
                "UPDATE {name} SET {} WHERE {}",
                set.join(", "),
                key_match(d, key)?
            ))
        }
        RowChange::Insert { values } => {
            if values.is_empty() {
                return Ok(match d {
                    Dialect::Mysql => format!("INSERT INTO {name} () VALUES ()"),
                    _ => format!("INSERT INTO {name} DEFAULT VALUES"),
                });
            }
            let cols: Vec<String> = values.iter().map(|(c, _)| quote_ident(d, c)).collect();
            let vals: Vec<String> = values.iter().map(|(_, v)| literal_or_null(d, v)).collect();
            Ok(format!(
                "INSERT INTO {name} ({}) VALUES ({})",
                cols.join(", "),
                vals.join(", ")
            ))
        }
        RowChange::Delete { key } => Ok(format!("DELETE FROM {name} WHERE {}", key_match(d, key)?)),
    }
}

fn key_match(d: Dialect, key: &[(String, Cell)]) -> Result<String, String> {
    if key.is_empty() {
        return Err("row has no primary key; refusing to touch every row".into());
    }
    let parts: Vec<String> = key
        .iter()
        .map(|(c, v)| match v {
            Some(v) => format!("{} = {}", quote_ident(d, c), quote_literal(d, v)),
            None => format!("{} IS NULL", quote_ident(d, c)),
        })
        .collect();
    Ok(parts.join(" AND "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(db: Option<&str>, schema: Option<&str>, table: &str) -> TableRef {
        TableRef {
            database: db.map(String::from),
            schema: schema.map(String::from),
            table: table.into(),
        }
    }

    #[test]
    fn quotes_identifiers_per_dialect_and_escapes_the_quote_char() {
        assert_eq!(quote_ident(Dialect::Postgres, r#"we"ird"#), r#""we""ird""#);
        assert_eq!(quote_ident(Dialect::Mysql, "a`b"), "`a``b`");
        assert_eq!(quote_ident(Dialect::Mssql, "a]b"), "[a]]b]");
    }

    #[test]
    fn quotes_literals_per_dialect() {
        assert_eq!(quote_literal(Dialect::Postgres, "it's"), "'it''s'");
        assert_eq!(quote_literal(Dialect::Mysql, r"a\'b"), r"'a\\''b'");
        assert_eq!(quote_literal(Dialect::Mssql, "x'y"), "N'x''y'");
    }

    #[test]
    fn mysql_literal_cannot_break_out_with_trailing_backslash() {
        // Without backslash escaping, `\` + `'` would close nothing and the
        // rest of the input would become SQL.
        let lit = quote_literal(Dialect::Mysql, r"evil\");
        assert_eq!(lit, r"'evil\\'");
    }

    #[test]
    fn qualifies_tables() {
        assert_eq!(
            qualified_table(Dialect::Postgres, &t(Some("db"), Some("public"), "users")),
            r#""public"."users""#
        );
        assert_eq!(
            qualified_table(Dialect::Mysql, &t(Some("shop"), None, "orders")),
            "`shop`.`orders`"
        );
        assert_eq!(
            qualified_table(Dialect::Sqlite, &t(Some("main"), None, "x")),
            r#""x""#
        );
        assert_eq!(
            qualified_table(Dialect::Sqlite, &t(Some("aux"), None, "x")),
            r#""aux"."x""#
        );
        assert_eq!(
            qualified_table(Dialect::Mssql, &t(Some("hotel"), Some("dbo"), "Guest")),
            "[hotel].[dbo].[Guest]"
        );
    }

    fn spec<'a>(table: &'a TableRef, sort: &'a [SortSpec], cols: &'a [String]) -> SelectSpec<'a> {
        SelectSpec {
            table,
            sort,
            filter: Some("age > 30"),
            search: Some("50%_off"),
            search_columns: cols,
            limit: 100,
            offset: 200,
        }
    }

    #[test]
    fn builds_paged_select_with_filter_search_and_sort() {
        let table = t(None, Some("public"), "users");
        let sort = [SortSpec {
            column: "name".into(),
            dir: SortDir::Desc,
        }];
        let cols = vec!["name".to_string(), "email".to_string()];
        let sql = build_select(Dialect::Postgres, &spec(&table, &sort, &cols));
        assert_eq!(
            sql,
            "SELECT * FROM \"public\".\"users\" WHERE (age > 30) AND \
             (CAST(\"name\" AS TEXT) ILIKE '%50!%!_off%' ESCAPE '!' OR \
             CAST(\"email\" AS TEXT) ILIKE '%50!%!_off%' ESCAPE '!') \
             ORDER BY \"name\" DESC LIMIT 100 OFFSET 200"
        );
    }

    #[test]
    fn mssql_paging_always_has_order_by() {
        let table = t(Some("db"), Some("dbo"), "t");
        let s = SelectSpec {
            table: &table,
            sort: &[],
            filter: None,
            search: None,
            search_columns: &[],
            limit: 50,
            offset: 0,
        };
        assert_eq!(
            build_select(Dialect::Mssql, &s),
            "SELECT * FROM [db].[dbo].[t] ORDER BY (SELECT NULL) OFFSET 0 ROWS FETCH NEXT 50 ROWS ONLY"
        );
        assert_eq!(
            build_count(Dialect::Mssql, &s),
            "SELECT COUNT(*) FROM [db].[dbo].[t]"
        );
    }

    #[test]
    fn mssql_escapes_like_brackets() {
        let p = search_predicate(Dialect::Mssql, "[a]", &["c".to_string()]);
        assert!(p.contains("'%![a]%'"), "{p}");
    }

    #[test]
    fn builds_update_insert_delete() {
        let table = t(None, Some("public"), "users");
        let key = vec![("id".to_string(), Some("7".to_string()))];
        let update = RowChange::Update {
            key: key.clone(),
            values: vec![("name".into(), Some("O'Neil".into())), ("bio".into(), None)],
        };
        assert_eq!(
            build_change(Dialect::Postgres, &table, &update).unwrap(),
            r#"UPDATE "public"."users" SET "name" = 'O''Neil', "bio" = NULL WHERE "id" = '7'"#
        );
        let insert = RowChange::Insert {
            values: vec![("name".into(), Some("a".into()))],
        };
        assert_eq!(
            build_change(Dialect::Postgres, &table, &insert).unwrap(),
            r#"INSERT INTO "public"."users" ("name") VALUES ('a')"#
        );
        let delete = RowChange::Delete { key };
        assert_eq!(
            build_change(Dialect::Postgres, &table, &delete).unwrap(),
            r#"DELETE FROM "public"."users" WHERE "id" = '7'"#
        );
    }

    #[test]
    fn refuses_keyless_update_or_delete() {
        let table = t(None, None, "t");
        let delete = RowChange::Delete { key: vec![] };
        assert!(build_change(Dialect::Sqlite, &table, &delete).is_err());
        let update = RowChange::Update {
            key: vec![],
            values: vec![("a".into(), None)],
        };
        assert!(build_change(Dialect::Sqlite, &table, &update).is_err());
    }

    #[test]
    fn null_key_uses_is_null() {
        let table = t(None, None, "t");
        let delete = RowChange::Delete {
            key: vec![("k".into(), None)],
        };
        assert_eq!(
            build_change(Dialect::Sqlite, &table, &delete).unwrap(),
            r#"DELETE FROM "t" WHERE "k" IS NULL"#
        );
    }

    #[test]
    fn empty_insert_uses_defaults() {
        let table = t(Some("db"), None, "t");
        let insert = RowChange::Insert { values: vec![] };
        assert_eq!(
            build_change(Dialect::Mysql, &table, &insert).unwrap(),
            "INSERT INTO `db`.`t` () VALUES ()"
        );
    }
}
