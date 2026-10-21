//! Read-only guard. A connection marked read-only only lets through
//! statements whose first keyword is a known read and which contain no
//! keyword or function that writes. It is deliberately strict: a false
//! "blocked" is an annoyance, a false "allowed" could change production data.
//!
//! This is one of two layers — drivers also open read-only sessions where
//! the database supports it (see `db::*::connect`).

use super::split::Statement;
use super::Dialect;

const READ_FIRST_WORDS: &[&str] = &[
    "SELECT", "WITH", "SHOW", "EXPLAIN", "DESCRIBE", "DESC", "VALUES", "TABLE", "USE",
];

/// Words that write (or can write) wherever they appear in a statement.
const WRITE_WORDS: &[&str] = &[
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "UPSERT",
    "DROP",
    "ALTER",
    "CREATE",
    "TRUNCATE",
    "RENAME",
    "GRANT",
    "REVOKE",
    "DENY",
    "EXEC",
    "EXECUTE",
    "CALL",
    "INTO",
    "COPY",
    "LOCK",
    "ATTACH",
    "DETACH",
    "VACUUM",
    "REINDEX",
    "COMMIT",
    "ROLLBACK",
    "SAVEPOINT",
    // Functions with side effects, reachable from a plain SELECT.
    "SET_CONFIG",
    "SETVAL",
    "NEXTVAL",
    "PG_TERMINATE_BACKEND",
    "PG_CANCEL_BACKEND",
    "PG_RELOAD_CONF",
    "PG_ROTATE_LOGFILE",
    "PG_SWITCH_WAL",
    "PG_CREATE_RESTORE_POINT",
    "PG_PROMOTE",
    "PG_LOGICAL_EMIT_MESSAGE",
    "PG_CREATE_PHYSICAL_REPLICATION_SLOT",
    "PG_CREATE_LOGICAL_REPLICATION_SLOT",
    "PG_DROP_REPLICATION_SLOT",
    "PG_REPLICATION_SLOT_ADVANCE",
    "PG_STAT_RESET",
    "PG_STAT_RESET_SHARED",
    "PG_IMPORT_SYSTEM_COLLATIONS",
    "PG_FILE_WRITE",
    // query_to_xml & co. run an arbitrary query string.
    "QUERY_TO_XML",
    "QUERY_TO_XMLSCHEMA",
    "QUERY_TO_XML_AND_XMLSCHEMA",
    "CURSOR_TO_XML",
    "CURSOR_TO_XMLSCHEMA",
    "LO_IMPORT",
    "LO_EXPORT",
    "LO_UNLINK",
    "LO_CREATE",
    "LO_CREAT",
    "LO_FROM_BYTEA",
    "LO_PUT",
    "LO_TRUNCATE",
    "LO_TRUNCATE64",
    "LOWRITE",
    "BRIN_SUMMARIZE_NEW_VALUES",
    "BRIN_SUMMARIZE_RANGE",
    "BRIN_DESUMMARIZE_RANGE",
    "GIN_CLEAN_PENDING_LIST",
    "DBLINK",
    "DBLINK_EXEC",
    "LOAD_EXTENSION",
    "WRITEFILE",
    "OPENROWSET",
    "OPENQUERY",
    "OPENDATASOURCE",
    "XP_CMDSHELL",
];

/// T-SQL needs no `;` between statements, so any statement keyword can
/// follow a SELECT in the same batch. These are reserved words in T-SQL
/// (or rare as bare names) and none of them belongs in a read.
const MSSQL_WRITE_WORDS: &[&str] = &[
    "SET",
    "KILL",
    "BACKUP",
    "RESTORE",
    "DBCC",
    "SHUTDOWN",
    "RECONFIGURE",
    "CHECKPOINT",
    "WAITFOR",
    "WRITETEXT",
    "UPDATETEXT",
    "ENABLE",
    "DISABLE",
    "RECEIVE",
    "SEND",
    "CONVERSATION",
    "DIALOG",
    "SETUSER",
    "REVERT",
    "BULK",
    "BEGIN",
    "DECLARE",
    "OPEN",
];

pub fn check_read_only(statements: &[Statement], dialect: Dialect) -> Result<(), String> {
    statements
        .iter()
        .try_for_each(|s| check_statement(s, dialect))
}

fn check_statement(stmt: &Statement, dialect: Dialect) -> Result<(), String> {
    if stmt.has_executable_comment {
        return Err("MySQL executable comments (/*! … */) are not allowed".into());
    }
    let Some(first) = stmt.first_word() else {
        return Err(format!("cannot verify statement `{}`", preview(&stmt.text)));
    };
    if !READ_FIRST_WORDS.contains(&first) {
        return Err(format!("`{first}` statements are not allowed"));
    }
    let extra: &[&str] = if dialect == Dialect::Mssql {
        MSSQL_WRITE_WORDS
    } else {
        &[]
    };
    let denied = |w: &&String| WRITE_WORDS.contains(&w.as_str()) || extra.contains(&w.as_str());
    if let Some(word) = stmt.words.iter().find(denied) {
        return Err(format!("`{word}` is not allowed"));
    }
    // `NEXT VALUE FOR seq` advances a sequence (`FETCH NEXT n ROWS` is fine).
    if stmt
        .words
        .windows(2)
        .any(|w| w[0] == "NEXT" && w[1] == "VALUE")
    {
        return Err("`NEXT VALUE FOR` changes a sequence".into());
    }
    Ok(())
}

fn preview(text: &str) -> String {
    let short: String = text.chars().take(40).collect();
    if short.len() < text.len() {
        format!("{short}…")
    } else {
        short
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::{split::split, Dialect};

    fn allowed(sql: &str, d: Dialect) -> bool {
        check_read_only(&split(sql, d), d).is_ok()
    }

    #[test]
    fn allows_plain_reads() {
        for sql in [
            "SELECT * FROM users WHERE id = 1",
            "with x as (select 1) select * from x",
            "SHOW TABLES",
            "EXPLAIN SELECT 1",
            "explain analyze select * from t",
            "DESCRIBE users",
            "select replace(name, 'a', 'b') from t",
            "select 'delete from x' as txt",
            r#"select "update" from t"#,
            "select * from inserts join updated_rows using (id)",
            "-- note: drop everything\nselect 1",
        ] {
            assert!(allowed(sql, Dialect::Postgres), "should allow: {sql}");
        }
        assert!(allowed("select [delete] from [insert]", Dialect::Mssql));
        assert!(allowed("select `drop` from t", Dialect::Mysql));
    }

    #[test]
    fn blocks_writes_and_ddl() {
        for sql in [
            "INSERT INTO t VALUES (1)",
            "update t set a = 1",
            "DELETE FROM t",
            "drop table t",
            "ALTER TABLE t ADD c int",
            "create table t (a int)",
            "TRUNCATE t",
            "GRANT ALL ON t TO bob",
            "MERGE INTO t USING s ON 1=1 WHEN MATCHED THEN DELETE",
            "REPLACE INTO t VALUES (1)",
            "SET default_transaction_read_only = off",
            "BEGIN",
            "COMMIT",
            "DO $$ begin delete from t; end $$",
            "CALL p()",
            "EXEC sp_who",
            "LOAD DATA INFILE 'x' INTO TABLE t",
            "PRAGMA journal_mode = WAL",
            "VACUUM",
        ] {
            assert!(!allowed(sql, Dialect::Postgres), "should block: {sql}");
        }
    }

    #[test]
    fn blocks_writes_hidden_inside_reads() {
        for sql in [
            "WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d",
            "EXPLAIN ANALYZE DELETE FROM t",
            "SELECT * INTO new_t FROM t",
            "SELECT * FROM t INTO OUTFILE '/tmp/x'",
            "select set_config('default_transaction_read_only', 'off', false)",
            "select nextval('seq')",
            "select pg_terminate_backend(123)",
            "select * from t for update",
            "SELECT * FROM OPENROWSET('SQLNCLI', 'x', 'delete from t')",
        ] {
            assert!(!allowed(sql, Dialect::Postgres), "should block: {sql}");
        }
    }

    #[test]
    fn blocks_a_write_anywhere_in_a_script() {
        assert!(!allowed(
            "select 1; delete from t; select 2",
            Dialect::Postgres
        ));
        assert!(!allowed("select 1\nGO\ndelete from t", Dialect::Mssql));
        assert!(!allowed("select 1; delete from t", Dialect::Mssql));
    }

    #[test]
    fn mysql_double_dash_without_space_is_not_a_comment() {
        // MySQL runs all three statements; `--1` is minus minus one.
        let sql = "SELECT 1 --1;SET SESSION TRANSACTION READ WRITE;DELETE FROM t";
        assert!(!allowed(sql, Dialect::Mysql));
        let filter = "SELECT * FROM t WHERE (1=1) AND 1 --1;DELETE FROM t;SELECT (1)";
        assert!(!allowed(filter, Dialect::Mysql));
        // A real comment still works.
        assert!(allowed("SELECT 1 -- delete everything\n", Dialect::Mysql));
    }

    #[test]
    fn mysql_executable_comments_are_refused() {
        for sql in [
            "SELECT 1 /*! ; DELETE FROM t */",
            "SELECT 1 /*!50000 INTO OUTFILE '/tmp/x' */",
            "SELECT 1 /*M! ; DELETE FROM t */",
            "SELECT /*!40001 SQL_NO_CACHE */ 1",
        ] {
            assert!(!allowed(sql, Dialect::Mysql), "should block: {sql}");
        }
        assert!(allowed("SELECT /* plain */ 1", Dialect::Mysql));
    }

    #[test]
    fn mssql_batch_statements_without_semicolons_are_refused() {
        for sql in [
            "SELECT 1; KILL 55",
            "SELECT 1 BACKUP DATABASE x TO DISK='f'",
            "SELECT 1; DBCC FREEPROCCACHE",
            "SELECT 1; SHUTDOWN",
            "SELECT 1 RESTORE DATABASE x FROM DISK='f'",
            "SELECT 1 RECONFIGURE",
            "SELECT 1 WAITFOR DELAY '01:00:00'",
            "SELECT 1 SET XACT_ABORT OFF",
            "SELECT NEXT VALUE FOR dbo.seq",
            "SELECT 1 RECEIVE TOP(1) * FROM q",
            "SELECT 1 WRITETEXT t.c @p 'x'",
            "SELECT 1 DISABLE TRIGGER tr ON t",
            "SELECT 1 CHECKPOINT",
        ] {
            assert!(!allowed(sql, Dialect::Mssql), "should block: {sql}");
        }
        // Paging syntax uses NEXT too and must keep working.
        assert!(allowed(
            "SELECT * FROM t ORDER BY (SELECT NULL) OFFSET 0 ROWS FETCH NEXT 50 ROWS ONLY",
            Dialect::Mssql
        ));
    }

    #[test]
    fn postgres_side_effect_functions_are_refused() {
        for sql in [
            "select query_to_xml('insert into t values(1)', true, true, '')",
            "select lo_from_bytea(0, 'x')",
            "select lo_put(1, 0, 'x')",
            "select lowrite(0, 'x')",
            "select pg_logical_emit_message(true, 'a', 'b')",
            "select pg_switch_wal()",
            "select brin_summarize_new_values('i')",
        ] {
            assert!(!allowed(sql, Dialect::Postgres), "should block: {sql}");
        }
    }

    #[test]
    fn mssql_nested_comments_match_the_server() {
        // T-SQL comments nest, so this whole thing is a comment on the server.
        assert!(allowed(
            "SELECT 1 /* a /* b */ DROP TABLE t */",
            Dialect::Mssql
        ));
    }

    #[test]
    fn rejects_statements_without_keywords() {
        assert!(!allowed("(1)", Dialect::Postgres));
    }

    #[test]
    fn empty_script_is_allowed() {
        assert!(allowed("  -- nothing\n", Dialect::Postgres));
    }

    #[test]
    fn error_names_the_offending_keyword() {
        let err = check_read_only(
            &split("select 1; drop table x", Dialect::Mysql),
            Dialect::Mysql,
        )
        .unwrap_err();
        assert!(err.contains("DROP"), "{err}");
    }
}
