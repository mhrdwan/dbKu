//! Dump → drop → restore round trips, CSV import and SQL files, against
//! SQLite always and Postgres/MySQL when DBKU_TEST_POSTGRES / DBKU_TEST_MYSQL
//! are set (see tests/drivers.rs for the format). Tables are named dbku_x_*.

use dbku_lib::db::{meta, Session};
use dbku_lib::model::*;
use dbku_lib::transfer::csv_import::{import_csv, ColumnMapping, CsvImportOptions};
use dbku_lib::transfer::dump::{dump_to_file, DumpOptions};
use dbku_lib::transfer::script::run_file;
use dbku_lib::transfer::Silent;

fn sqlite_profile(path: &std::path::Path) -> ConnectionProfile {
    ConnectionProfile {
        id: "t".into(),
        name: "t".into(),
        driver: Driver::Sqlite,
        host: String::new(),
        port: 0,
        user: String::new(),
        database: path.to_string_lossy().into_owned(),
        ssl_mode: SslMode::Disable,
        trust_server_cert: false,
        legacy_tls: false,
        read_only: false,
        color: None,
        group: None,
        save_password: false,
    }
}

fn server_profile(driver: Driver, spec: &str) -> (ConnectionProfile, Option<String>) {
    let p: Vec<&str> = spec.splitn(5, ':').collect();
    let mut profile = sqlite_profile(std::path::Path::new(""));
    profile.driver = driver;
    profile.host = p[0].into();
    profile.port = p[1].parse().unwrap();
    profile.user = p[2].into();
    profile.database = p[4].into();
    (profile, Some(p[3].to_string()).filter(|s| !s.is_empty()))
}

struct Fixture<'a> {
    s: &'a Session,
    ctx: QueryContext,
}

impl Fixture<'_> {
    async fn exec(&self, sql: &str) -> ExecOutcome {
        let out = self
            .s
            .execute(&self.ctx, sql, &ExecOptions::default())
            .await
            .expect("execute");
        assert!(out.error.is_none(), "`{sql}` failed: {:?}", out.error);
        out
    }

    async fn rows(&self, sql: &str) -> Vec<Vec<Cell>> {
        self.exec(sql)
            .await
            .results
            .into_iter()
            .next()
            .unwrap_or_default()
            .rows
    }

    fn table(&self, name: &str) -> TableRef {
        TableRef {
            database: self.ctx.database.clone(),
            schema: self.ctx.schema.clone(),
            table: name.into(),
        }
    }
}

const TABLES: [&str; 3] = ["dbku_x_child", "dbku_x_auto", "dbku_x_parent"];

async fn create_fixture(f: &Fixture<'_>, driver: Driver) {
    f.exec("DROP VIEW IF EXISTS dbku_x_a_v").await;
    f.exec("DROP VIEW IF EXISTS dbku_x_v").await;
    for t in TABLES {
        f.exec(&format!("DROP TABLE IF EXISTS {t}")).await;
    }
    let (auto, bin, bin_value) = match driver {
        Driver::Postgres => ("id SERIAL PRIMARY KEY", "BYTEA", "'\\x00ff10'"),
        Driver::Mysql => (
            "id INT AUTO_INCREMENT PRIMARY KEY",
            "VARBINARY(10)",
            "X'00FF10'",
        ),
        _ => ("id INTEGER PRIMARY KEY", "BLOB", "X'00FF10'"),
    };
    let backslash = if driver == Driver::Mysql {
        r"'a\\b'"
    } else {
        r"'a\b'"
    };
    f.exec("CREATE TABLE dbku_x_parent (id INT PRIMARY KEY, name VARCHAR(50) NOT NULL)")
        .await;
    f.exec(&format!(
        "CREATE TABLE dbku_x_child (id INT PRIMARY KEY, parent_id INT, note VARCHAR(100), amount DECIMAL(10,2), \
         bin {bin}, FOREIGN KEY (parent_id) REFERENCES dbku_x_parent (id))"
    ))
    .await;
    f.exec("CREATE INDEX dbku_x_child_parent ON dbku_x_child (parent_id)")
        .await;
    f.exec(&format!("CREATE TABLE dbku_x_auto ({auto}, v VARCHAR(20))"))
        .await;
    f.exec("INSERT INTO dbku_x_parent VALUES (1, 'héllo — 世界'), (2, 'it''s')")
        .await;
    f.exec(&format!(
        "INSERT INTO dbku_x_child VALUES (1, 1, {backslash}, 12.50, {bin_value}), (2, 2, NULL, -0.01, NULL), \
         (3, 1, 'line1\nline2', 0, NULL)"
    ))
    .await;
    f.exec("INSERT INTO dbku_x_auto (v) VALUES ('a'), ('b'), ('c')")
        .await;
    f.exec("CREATE VIEW dbku_x_v AS SELECT c.id, p.name FROM dbku_x_child c JOIN dbku_x_parent p ON p.id = c.parent_id")
        .await;
    // Sorts before the view it reads from: dumps must order views by dependency.
    f.exec("CREATE VIEW dbku_x_a_v AS SELECT id FROM dbku_x_v")
        .await;
}

async fn snapshot(f: &Fixture<'_>) -> Vec<Vec<Vec<Cell>>> {
    let mut all = Vec::new();
    for t in [
        "dbku_x_parent",
        "dbku_x_child",
        "dbku_x_auto",
        "dbku_x_v",
        "dbku_x_a_v",
    ] {
        all.push(f.rows(&format!("SELECT * FROM {t} ORDER BY id")).await);
    }
    all
}

async fn round_trip(s: &Session, ctx: QueryContext, driver: Driver) {
    let f = Fixture {
        s,
        ctx: ctx.clone(),
    };
    create_fixture(&f, driver).await;
    let before = snapshot(&f).await;
    assert_eq!(
        before[1][0][2].as_deref(),
        Some(r"a\b"),
        "fixture stores a real backslash"
    );

    let db = ctx.database.clone().unwrap_or_default();
    let objects: Vec<DbObject> = meta::objects(s, &db, ctx.schema.as_deref())
        .await
        .unwrap()
        .into_iter()
        .filter(|o| o.name.starts_with("dbku_x_"))
        .collect();
    assert_eq!(objects.len(), 5, "{objects:?}");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dump.sql");
    let opts = DumpOptions {
        structure: true,
        data: true,
        drop_existing: true,
        rows_per_insert: 2,
    };
    let summary = dump_to_file(
        s,
        ctx.database.as_deref(),
        ctx.schema.as_deref(),
        &objects,
        &opts,
        &path,
        &Silent,
    )
    .await
    .expect("dump");
    assert_eq!(summary.rows, 8);
    assert!(
        !dir.path().join("dump.sql.partial").exists(),
        "temp file is renamed away"
    );

    // Wipe everything, then restore from the file alone.
    f.exec("DROP VIEW dbku_x_a_v").await;
    f.exec("DROP VIEW dbku_x_v").await;
    for t in TABLES {
        f.exec(&format!("DROP TABLE {t}")).await;
    }
    let outcome = run_file(s, &ctx, &path, &Silent).await.unwrap();
    let script = std::fs::read_to_string(&path).unwrap();
    assert!(
        outcome.error.is_none(),
        "restore failed: {:?}\n--- dump ---\n{script}",
        outcome.error
    );

    assert_eq!(
        snapshot(&f).await,
        before,
        "restored data matches\n--- dump ---\n{script}"
    );
    // Counters continue after the restored maximum.
    f.exec("INSERT INTO dbku_x_auto (v) VALUES ('d')").await;
    let next = f.rows("SELECT id FROM dbku_x_auto WHERE v = 'd'").await;
    assert_eq!(next[0][0].as_deref(), Some("4"), "auto-increment continues");
    // The foreign key came back.
    let bad = s
        .execute(
            &ctx,
            "INSERT INTO dbku_x_child (id, parent_id) VALUES (99, 42)",
            &ExecOptions::default(),
        )
        .await
        .unwrap();
    if driver != Driver::Sqlite {
        assert!(bad.error.is_some(), "foreign key is enforced after restore");
    }
    let idx = meta::indexes(s, &f.table("dbku_x_child")).await.unwrap();
    assert!(
        idx.iter().any(|i| i.columns == ["parent_id"]),
        "index restored: {idx:?}"
    );

    csv_import(&f).await;
    scripts(&f).await;

    f.exec("DROP VIEW dbku_x_a_v").await;
    f.exec("DROP VIEW dbku_x_v").await;
    for t in TABLES {
        f.exec(&format!("DROP TABLE IF EXISTS {t}")).await;
    }
}

async fn csv_import(f: &Fixture<'_>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("in.csv");
    std::fs::write(
        &path,
        "\u{feff}name;code\n\"Dewi; \"\"D\"\"\";10\n\nEka\n;30\n\"multi\nline\";40\n",
    )
    .unwrap();
    let opts = CsvImportOptions {
        delimiter: ";".into(),
        has_header: true,
        empty_as_null: true,
        truncate_first: true,
        mapping: vec![
            ColumnMapping {
                column: "parent_id".into(),
                source: 1,
            },
            ColumnMapping {
                column: "note".into(),
                source: 0,
            },
        ],
        rows_per_batch: Some(2),
    };
    // "Eka" has only one field: the whole import must roll back.
    let table = f.table("dbku_x_child");
    let err = import_csv(f.s, &table, &path, &opts, &Silent)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Row 2"), "{err}");
    assert_eq!(
        f.rows("SELECT COUNT(*) FROM dbku_x_child").await[0][0].as_deref(),
        Some("3"),
        "nothing changed"
    );

    std::fs::write(
        &path,
        "\u{feff}id;note;amount\n10;\"Dewi; \"\"D\"\"\";1.5\n11;;\n12;\"multi\nline\";2\n13;héllo;3\n",
    )
    .unwrap();
    let opts = CsvImportOptions {
        mapping: vec![
            ColumnMapping {
                column: "id".into(),
                source: 0,
            },
            ColumnMapping {
                column: "note".into(),
                source: 1,
            },
            ColumnMapping {
                column: "amount".into(),
                source: 2,
            },
        ],
        truncate_first: false,
        ..opts
    };
    let n = import_csv(f.s, &table, &path, &opts, &Silent)
        .await
        .unwrap();
    assert_eq!(n, 4);
    let rows = f
        .rows("SELECT id, note, amount FROM dbku_x_child WHERE id >= 10 ORDER BY id")
        .await;
    assert_eq!(rows[0][1].as_deref(), Some("Dewi; \"D\""));
    assert_eq!(rows[1][1], None, "empty field became NULL");
    assert_eq!(rows[2][1].as_deref(), Some("multi\nline"));
    assert_eq!(rows[3][1].as_deref(), Some("héllo"));
}

async fn scripts(f: &Fixture<'_>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.sql");
    std::fs::write(
        &path,
        "INSERT INTO dbku_x_parent VALUES (50, 'x');\nUPDATE dbku_x_parent SET name = 'y' WHERE id = 50;\n\
         INSERT INTO dbku_x_missing VALUES (1);\nINSERT INTO dbku_x_parent VALUES (51, 'never');\n",
    )
    .unwrap();
    let out = run_file(f.s, &f.ctx, &path, &Silent).await.unwrap();
    assert_eq!(out.total, 4);
    assert_eq!(out.executed, 2);
    assert_eq!(out.error.as_ref().map(|e| e.index), Some(2));
    assert_eq!(
        f.rows("SELECT name FROM dbku_x_parent WHERE id = 50").await[0][0].as_deref(),
        Some("y")
    );
    assert!(
        f.rows("SELECT * FROM dbku_x_parent WHERE id = 51")
            .await
            .is_empty(),
        "stopped at the error"
    );
    f.exec("DELETE FROM dbku_x_parent WHERE id = 50").await;
}

#[tokio::test]
async fn sqlite_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(sqlite_profile(&dir.path().join("a.db")), None)
        .await
        .unwrap();
    let ctx = QueryContext {
        database: Some("main".into()),
        schema: None,
    };
    round_trip(&s, ctx, Driver::Sqlite).await;
}

#[tokio::test]
async fn read_only_refuses_imports_and_writing_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ro.db");
    let rw = Session::open(sqlite_profile(&path), None).await.unwrap();
    rw.execute(
        &QueryContext::default(),
        "CREATE TABLE t (a INT)",
        &ExecOptions::default(),
    )
    .await
    .unwrap();
    rw.close().await;
    let mut p = sqlite_profile(&path);
    p.read_only = true;
    let ro = Session::open(p, None).await.unwrap();
    let script = dir.path().join("w.sql");
    std::fs::write(&script, "SELECT 1; DELETE FROM t;").unwrap();
    let err = run_file(&ro, &QueryContext::default(), &script, &Silent)
        .await
        .unwrap_err();
    assert!(
        matches!(err, dbku_lib::error::AppError::ReadOnly(_)),
        "{err}"
    );
    let mut batches = std::iter::once(Ok("INSERT INTO t VALUES (1)".to_string()));
    let err = ro.apply_batches(None, &mut batches).await.unwrap_err();
    assert!(matches!(err, dbku_lib::error::AppError::ReadOnly(_)));
}

#[tokio::test]
async fn sqlite_failed_script_leaves_no_open_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(sqlite_profile(&dir.path().join("tx.db")), None)
        .await
        .unwrap();
    let f = Fixture {
        s: &s,
        ctx: QueryContext::default(),
    };
    f.exec("CREATE TABLE t (a INT)").await;
    let script = dir.path().join("s.sql");
    std::fs::write(
        &script,
        "PRAGMA foreign_keys = OFF; BEGIN; INSERT INTO t VALUES (1); INSERT INTO missing VALUES (1);",
    )
    .unwrap();
    let out = run_file(&s, &QueryContext::default(), &script, &Silent)
        .await
        .unwrap();
    assert!(out.error.is_some());
    // The half-done script was rolled back and normal work goes on.
    f.exec("INSERT INTO t VALUES (2)").await;
    assert_eq!(
        f.rows("SELECT a FROM t").await,
        vec![vec![Some("2".to_string())]]
    );
    assert_eq!(
        f.rows("PRAGMA foreign_keys").await[0][0].as_deref(),
        Some("1"),
        "foreign keys back on"
    );
}

/// Postgres: a schema other than public restores on its own, enums included.
async fn postgres_other_schema(s: &Session, db: &str) {
    let ctx = QueryContext {
        database: Some(db.to_string()),
        schema: Some("dbku_x_s".into()),
    };
    let f = Fixture {
        s,
        ctx: ctx.clone(),
    };
    f.exec("DROP SCHEMA IF EXISTS dbku_x_s CASCADE").await;
    f.exec("CREATE SCHEMA dbku_x_s").await;
    f.exec("CREATE TYPE dbku_x_s.mood AS ENUM ('sad', 'ok', 'happy')")
        .await;
    f.exec("CREATE TABLE dbku_x_s.t (id INT PRIMARY KEY, m dbku_x_s.mood)")
        .await;
    f.exec("INSERT INTO dbku_x_s.t VALUES (1, 'happy')").await;
    f.exec("CREATE VIEW dbku_x_s.v AS SELECT id FROM dbku_x_s.t")
        .await;
    let objects = meta::objects(s, db, Some("dbku_x_s")).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.sql");
    let opts = DumpOptions {
        structure: true,
        data: true,
        drop_existing: true,
        rows_per_insert: 10,
    };
    dump_to_file(
        s,
        Some(db),
        Some("dbku_x_s"),
        &objects,
        &opts,
        &path,
        &Silent,
    )
    .await
    .unwrap();
    f.exec("DROP SCHEMA dbku_x_s CASCADE").await;
    // Restore from a plain session (default search_path).
    let plain = QueryContext {
        database: Some(db.to_string()),
        schema: None,
    };
    let out = run_file(s, &plain, &path, &Silent).await.unwrap();
    let script = std::fs::read_to_string(&path).unwrap();
    assert!(
        out.error.is_none(),
        "restore failed: {:?}\n{script}",
        out.error
    );
    let rows = f
        .rows("SELECT m::text FROM dbku_x_s.t JOIN dbku_x_s.v USING (id)")
        .await;
    assert_eq!(rows[0][0].as_deref(), Some("happy"));
    f.exec("DROP SCHEMA dbku_x_s CASCADE").await;
}

/// MySQL: a dump restores into a different database, views included.
async fn mysql_other_database(s: &Session, db: &str) {
    let src = QueryContext {
        database: Some(db.to_string()),
        schema: None,
    };
    let f = Fixture {
        s,
        ctx: src.clone(),
    };
    f.exec("DROP VIEW IF EXISTS dbku_x_pv").await;
    f.exec("DROP TABLE IF EXISTS dbku_x_pt").await;
    f.exec("CREATE TABLE dbku_x_pt (id INT PRIMARY KEY)").await;
    f.exec("INSERT INTO dbku_x_pt VALUES (1), (2)").await;
    f.exec("CREATE VIEW dbku_x_pv AS SELECT id FROM dbku_x_pt")
        .await;
    let objects: Vec<DbObject> = meta::objects(s, db, None)
        .await
        .unwrap()
        .into_iter()
        .filter(|o| o.name.starts_with("dbku_x_p"))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.sql");
    let opts = DumpOptions {
        structure: true,
        data: true,
        drop_existing: true,
        rows_per_insert: 10,
    };
    dump_to_file(s, Some(db), None, &objects, &opts, &path, &Silent)
        .await
        .unwrap();
    f.exec("DROP DATABASE IF EXISTS dbku_x_copy").await;
    f.exec("CREATE DATABASE dbku_x_copy").await;
    let copy = QueryContext {
        database: Some("dbku_x_copy".into()),
        schema: None,
    };
    let out = run_file(s, &copy, &path, &Silent).await.unwrap();
    assert!(out.error.is_none(), "{:?}", out.error);
    // Empty the source: the copy's view must read the copy, not the source.
    f.exec("DELETE FROM dbku_x_pt").await;
    let rows = f.rows("SELECT COUNT(*) FROM dbku_x_copy.dbku_x_pv").await;
    let script = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        rows[0][0].as_deref(),
        Some("2"),
        "view in the copy reads the copy\n{script}"
    );
    f.exec("DROP DATABASE dbku_x_copy").await;
    f.exec("DROP VIEW dbku_x_pv").await;
    f.exec("DROP TABLE dbku_x_pt").await;
}

async fn external(var: &str, driver: Driver) {
    let Ok(spec) = std::env::var(var) else {
        eprintln!("skipping {driver:?}: set {var}");
        return;
    };
    let (p, pw) = server_profile(driver, &spec);
    let db = p.database.clone();
    let s = Session::open(p, pw.as_deref()).await.unwrap();
    let schema = (driver == Driver::Postgres).then(|| "public".to_string());
    round_trip(
        &s,
        QueryContext {
            database: Some(db.clone()),
            schema,
        },
        driver,
    )
    .await;
    match driver {
        Driver::Postgres => postgres_other_schema(&s, &db).await,
        Driver::Mysql => mysql_other_database(&s, &db).await,
        _ => {}
    }
    s.close().await;
}

#[tokio::test]
async fn postgres_round_trip() {
    external("DBKU_TEST_POSTGRES", Driver::Postgres).await;
}

#[tokio::test]
async fn mysql_round_trip() {
    external("DBKU_TEST_MYSQL", Driver::Mysql).await;
}

/// Manual check that dumps stream: run with `--ignored --nocapture` under
/// `/usr/bin/time -l` and look at the peak memory.
#[tokio::test]
#[ignore]
async fn postgres_large_dump_streams() {
    let Ok(spec) = std::env::var("DBKU_TEST_POSTGRES") else {
        return;
    };
    let (p, pw) = server_profile(Driver::Postgres, &spec);
    let db = p.database.clone();
    let s = Session::open(p, pw.as_deref()).await.unwrap();
    let ctx = QueryContext {
        database: Some(db.clone()),
        schema: Some("public".into()),
    };
    let f = Fixture {
        s: &s,
        ctx: ctx.clone(),
    };
    f.exec("DROP TABLE IF EXISTS dbku_x_big").await;
    f.exec("CREATE TABLE dbku_x_big AS SELECT g AS id, md5(g::text) || md5((g * 7)::text) AS payload, now() AS at FROM generate_series(1, 300000) g").await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.sql");
    let started = std::time::Instant::now();
    let opts = DumpOptions {
        structure: true,
        data: true,
        drop_existing: true,
        rows_per_insert: 500,
    };
    let objects = vec![DbObject {
        name: "dbku_x_big".into(),
        kind: "table".into(),
    }];
    let summary = dump_to_file(
        &s,
        Some(&db),
        Some("public"),
        &objects,
        &opts,
        &path,
        &Silent,
    )
    .await
    .unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    eprintln!(
        "dumped {} rows, {} MB in {:?}",
        summary.rows,
        size / 1_000_000,
        started.elapsed()
    );
    assert_eq!(summary.rows, 300_000);
    f.exec("DROP TABLE dbku_x_big").await;
}
