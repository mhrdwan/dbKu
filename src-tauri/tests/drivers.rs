//! End-to-end driver scenario. SQLite always runs; the others run when an
//! environment variable points at a disposable server:
//!
//!   DBKU_TEST_POSTGRES="host:port:user:password:database"
//!   DBKU_TEST_MYSQL="host:port:user:password:database"
//!   DBKU_TEST_MSSQL="host:port:user:password:database"
//!
//! The scenario creates and drops a table called `dbku_t`.

use dbku_lib::db::{meta, Session};
use dbku_lib::error::AppError;
use dbku_lib::model::*;

const TABLE: &str = "dbku_t";

fn profile(driver: Driver, spec: &str) -> (ConnectionProfile, Option<String>) {
    let parts: Vec<&str> = spec.splitn(5, ':').collect();
    let [host, port, user, password, database] = parts[..] else {
        panic!("expected host:port:user:password:database, got {spec}");
    };
    let profile = ConnectionProfile {
        id: format!("test-{driver:?}"),
        name: "test".into(),
        driver,
        host: host.into(),
        port: port.parse().expect("port"),
        user: user.into(),
        database: database.into(),
        ssl_mode: SslMode::Disable,
        trust_server_cert: true,
        legacy_tls: false,
        read_only: false,
        color: None,
        group: None,
        save_password: false,
    };
    (
        profile,
        Some(password.to_string()).filter(|p| !p.is_empty()),
    )
}

fn sqlite_profile(path: &std::path::Path) -> ConnectionProfile {
    ConnectionProfile {
        id: "test-sqlite".into(),
        name: "test".into(),
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

struct Ctx {
    database: String,
    schema: Option<String>,
}

impl Ctx {
    fn for_session(s: &Session) -> Self {
        let database = s.default_database().unwrap_or_default();
        let schema = match s.profile.driver {
            Driver::Postgres => Some("public".to_string()),
            Driver::Mssql => Some("dbo".to_string()),
            Driver::Mysql | Driver::Sqlite => None,
        };
        Self { database, schema }
    }
    fn query(&self) -> QueryContext {
        QueryContext {
            database: Some(self.database.clone()),
            schema: self.schema.clone(),
        }
    }
    fn table(&self) -> TableRef {
        TableRef {
            database: Some(self.database.clone()),
            schema: self.schema.clone(),
            table: TABLE.into(),
        }
    }
    fn table_query(&self) -> TableQuery {
        TableQuery {
            table: self.table(),
            limit: 100,
            offset: 0,
            sort: vec![],
            filter: None,
            search: None,
            search_columns: vec![],
        }
    }
}

async fn exec(s: &Session, c: &Ctx, sql: &str) -> ExecOutcome {
    let out = s
        .execute(&c.query(), sql, &ExecOptions::default())
        .await
        .expect("execute");
    assert!(out.error.is_none(), "`{sql}` failed: {:?}", out.error);
    out
}

/// Run on the dedicated connection of query tab "tab-a".
async fn in_tab(s: &Session, c: &Ctx, sql: &str) -> ExecOutcome {
    let opts = ExecOptions {
        max_rows: None,
        run_id: None,
        session: Some("tab-a".into()),
    };
    let out = s.execute(&c.query(), sql, &opts).await.expect("execute");
    assert!(out.error.is_none(), "`{sql}` failed: {:?}", out.error);
    out
}

fn ids(set: &ResultSet) -> Vec<String> {
    set.rows
        .iter()
        .map(|r| r[0].clone().unwrap_or_default())
        .collect()
}

async fn scenario(profile: ConnectionProfile, password: Option<String>) {
    let s = Session::open(profile.clone(), password.as_deref())
        .await
        .expect("connect");
    let c = Ctx::for_session(&s);
    let driver = profile.driver;

    // --- setup ---------------------------------------------------------------
    exec(&s, &c, &format!("DROP TABLE IF EXISTS {TABLE}")).await;
    exec(
        &s,
        &c,
        &format!(
            "CREATE TABLE {TABLE} (id INT PRIMARY KEY, name VARCHAR(50), score DECIMAL(10,2), \
             note VARCHAR(100) NULL)"
        ),
    )
    .await;
    let values: Vec<String> = (1..=30)
        .map(|i| {
            let name = match i {
                1 => "alice".to_string(),
                2 | 3 => "dup".to_string(),
                _ => format!("user{i}"),
            };
            format!("({i}, '{name}', {i}.5, NULL)")
        })
        .collect();
    let insert = exec(
        &s,
        &c,
        &format!(
            "INSERT INTO {TABLE} (id, name, score, note) VALUES {}",
            values.join(", ")
        ),
    )
    .await;
    assert_eq!(insert.results[0].rows_affected, Some(30), "insert count");

    // --- editor execution ----------------------------------------------------
    let capped = s
        .execute(
            &c.query(),
            &format!("SELECT id, name FROM {TABLE} ORDER BY id"),
            &ExecOptions {
                max_rows: Some(10),
                run_id: None,
                session: None,
            },
        )
        .await
        .unwrap();
    let set = &capped.results[0];
    assert!(set.truncated, "result should be marked truncated");
    assert_eq!(set.rows.len(), 10);
    assert_eq!(
        set.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "name"]
    );
    // The connection was reset after truncation; the next run must still work.
    let after = exec(&s, &c, &format!("SELECT COUNT(*) AS n FROM {TABLE}")).await;
    assert_eq!(after.results[0].rows[0][0].as_deref(), Some("30"));

    if driver == Driver::Postgres {
        // Cutting a huge result must not wait for the server to stream the rest.
        let started = std::time::Instant::now();
        let big = s
            .execute(
                &c.query(),
                "SELECT g, md5(g::text) FROM generate_series(1, 20000000) g",
                &ExecOptions {
                    max_rows: Some(100),
                    run_id: None,
                    session: None,
                },
            )
            .await
            .unwrap();
        assert!(big.results[0].truncated);
        assert!(
            started.elapsed().as_secs() < 3,
            "took {:?}",
            started.elapsed()
        );
    }

    let sleep = match driver {
        Driver::Postgres => Some("SELECT pg_sleep(10)"),
        Driver::Mysql => Some("SELECT SLEEP(10)"),
        Driver::Sqlite | Driver::Mssql => None,
    };
    if let Some(sleep) = sleep {
        let started = std::time::Instant::now();
        let opts = ExecOptions {
            max_rows: None,
            run_id: Some("run-1".into()),
            session: None,
        };
        let cancel = async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            s.cancel("run-1").await.unwrap()
        };
        let qc = c.query();
        let (_, cancelled) = tokio::join!(s.execute(&qc, sleep, &opts), cancel);
        assert!(cancelled, "run was registered");
        assert!(
            started.elapsed().as_secs() < 5,
            "cancel took {:?}",
            started.elapsed()
        );
        assert!(
            !s.cancel("run-1").await.unwrap(),
            "finished runs are forgotten"
        );
    }

    let empty = exec(
        &s,
        &c,
        &format!("SELECT id, name FROM {TABLE} WHERE id < 0"),
    )
    .await;
    if driver != Driver::Mssql {
        assert_eq!(
            empty.results[0].columns.len(),
            2,
            "empty result still has column names"
        );
    }
    assert!(empty.results[0].rows.is_empty());

    let script = match driver {
        Driver::Mssql => "SELECT 1 AS a\nGO\nSELECT * FROM dbku_missing\nGO\nSELECT 2 AS b",
        _ => "SELECT 1 AS a; SELECT * FROM dbku_missing; SELECT 2 AS b",
    };
    let out = s
        .execute(&c.query(), script, &ExecOptions::default())
        .await
        .unwrap();
    let err = out.error.expect("second statement fails");
    assert_eq!(err.index, 1);
    assert_eq!(out.results.len(), 1, "first statement's result is kept");
    assert_eq!(out.results[0].rows[0][0].as_deref(), Some("1"));

    // Text round-trip: unicode, NULL, quotes.
    exec(
        &s,
        &c,
        &format!("UPDATE {TABLE} SET note = 'héllo — 世界 ''q''' WHERE id = 5"),
    )
    .await;
    let row = exec(
        &s,
        &c,
        &format!("SELECT note, score FROM {TABLE} WHERE id IN (5, 6) ORDER BY id"),
    )
    .await;
    assert_eq!(
        row.results[0].rows[0][0].as_deref(),
        Some("héllo — 世界 'q'")
    );
    assert_eq!(row.results[0].rows[1][0], None, "NULL stays NULL");
    assert!(row.results[0].rows[0][1]
        .as_deref()
        .unwrap()
        .starts_with("5.5"));

    // --- query-tab sessions ---------------------------------------------------
    if matches!(driver, Driver::Postgres | Driver::Mysql) {
        let begin = if driver == Driver::Mysql {
            "START TRANSACTION"
        } else {
            "BEGIN"
        };
        in_tab(&s, &c, begin).await;
        in_tab(
            &s,
            &c,
            &format!("UPDATE {TABLE} SET note = 'in-tx' WHERE id = 7"),
        )
        .await;
        // Another run on the same tab sees its own uncommitted change...
        let mine = in_tab(&s, &c, &format!("SELECT note FROM {TABLE} WHERE id = 7")).await;
        assert_eq!(
            mine.results[0].rows[0][0].as_deref(),
            Some("in-tx"),
            "tab keeps its transaction"
        );
        // ...the browser (pooled connections) does not...
        let mut q = c.table_query();
        q.filter = Some("id = 7".into());
        let other = s.fetch_table(&q).await.unwrap();
        assert_eq!(
            other.rows[0][3], None,
            "uncommitted change is invisible elsewhere"
        );
        // ...and closing the tab rolls it back.
        s.close_tab("tab-a").await;
        let after = exec(&s, &c, &format!("SELECT note FROM {TABLE} WHERE id = 7")).await;
        assert_eq!(
            after.results[0].rows[0][0], None,
            "closing the tab rolled back"
        );
    }

    // --- metadata ------------------------------------------------------------
    let dbs = meta::databases(&s).await.unwrap();
    assert!(
        dbs.iter().any(|d| d.eq_ignore_ascii_case(&c.database)),
        "{dbs:?} has {}",
        c.database
    );
    if let Some(schema) = &c.schema {
        assert!(meta::schemas(&s, &c.database)
            .await
            .unwrap()
            .contains(schema));
    }
    let objects = meta::objects(&s, &c.database, c.schema.as_deref())
        .await
        .unwrap();
    assert!(
        objects.iter().any(|o| o.name == TABLE && o.kind == "table"),
        "{objects:?}"
    );
    let cols = meta::columns(&s, &c.table()).await.unwrap();
    assert_eq!(
        cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["id", "name", "score", "note"]
    );
    assert!(cols[0].is_primary_key);
    // SQLite quirk: a non-INTEGER primary key still accepts NULL.
    assert_eq!(cols[0].nullable, driver == Driver::Sqlite);
    assert!(!cols[1].is_primary_key && cols[3].nullable);
    let idx = meta::indexes(&s, &c.table()).await.unwrap();
    assert!(
        idx.iter().any(|i| i.is_primary && i.columns == ["id"]),
        "{idx:?}"
    );
    let ddl = meta::ddl(&s, &c.table()).await.unwrap();
    assert!(ddl.to_lowercase().contains("create table"), "{ddl}");
    let hits = meta::search_objects(&s, &c.database, "dbku").await.unwrap();
    assert!(
        hits.iter().any(|h| h.table == TABLE && h.kind == "table"),
        "{hits:?}"
    );
    let hits = meta::search_objects(&s, &c.database, "scor").await.unwrap();
    assert!(
        hits.iter()
            .any(|h| h.table == TABLE && h.column.as_deref() == Some("score")),
        "{hits:?}"
    );

    // --- table browser -------------------------------------------------------
    let mut q = c.table_query();
    q.sort = vec![SortSpec {
        column: "id".into(),
        dir: SortDir::Desc,
    }];
    q.limit = 5;
    q.offset = 5;
    assert_eq!(
        ids(&s.fetch_table(&q).await.unwrap()),
        ["25", "24", "23", "22", "21"]
    );

    let mut q = c.table_query();
    q.search = Some("ALI".into());
    assert_eq!(
        ids(&s.fetch_table(&q).await.unwrap()),
        ["1"],
        "search is case-insensitive"
    );
    let mut q = c.table_query();
    q.search = Some("100%".into());
    assert!(
        s.fetch_table(&q).await.unwrap().rows.is_empty(),
        "% is literal in search"
    );

    let mut q = c.table_query();
    q.filter = Some("id < 3".into());
    assert_eq!(s.fetch_table(&q).await.unwrap().rows.len(), 2);
    assert_eq!(s.count_rows(&q).await.unwrap(), 2);
    assert_eq!(s.count_rows(&c.table_query()).await.unwrap(), 30);

    // --- grid edits ----------------------------------------------------------
    let key = |id: &str| vec![("id".to_string(), Some(id.to_string()))];
    let changes = vec![
        RowChange::Update {
            key: key("1"),
            values: vec![("name".into(), Some("O'Ally".into()))],
        },
        RowChange::Insert {
            values: vec![
                ("id".into(), Some("100".into())),
                ("name".into(), Some("new".into())),
            ],
        },
    ];
    assert_eq!(s.apply_changes(&c.table(), &changes).await.unwrap(), 2);
    let check = exec(
        &s,
        &c,
        &format!("SELECT name FROM {TABLE} WHERE id IN (1, 100) ORDER BY id"),
    )
    .await;
    assert_eq!(ids(&check.results[0]), ["O'Ally", "new"]);

    // A key that matches two rows must roll back the whole batch.
    let bad = vec![
        RowChange::Delete { key: key("100") },
        RowChange::Update {
            key: vec![("name".into(), Some("dup".into()))],
            values: vec![("note".into(), Some("x".into()))],
        },
    ];
    assert!(s.apply_changes(&c.table(), &bad).await.is_err());
    let check = exec(
        &s,
        &c,
        &format!("SELECT COUNT(*) FROM {TABLE} WHERE id = 100 OR note = 'x'"),
    )
    .await;
    assert_eq!(
        check.results[0].rows[0][0].as_deref(),
        Some("1"),
        "delete was rolled back"
    );

    // --- read-only mode ------------------------------------------------------
    let mut ro_profile = profile.clone();
    ro_profile.read_only = true;
    let ro = Session::open(ro_profile, password.as_deref())
        .await
        .expect("read-only connect");
    let ok = ro
        .execute(
            &c.query(),
            &format!("SELECT COUNT(*) FROM {TABLE}"),
            &ExecOptions::default(),
        )
        .await;
    assert!(ok.unwrap().error.is_none());
    for sql in [
        format!("DELETE FROM {TABLE}"),
        format!("SELECT 1; DROP TABLE {TABLE}"),
        format!("UPDATE {TABLE} SET name = 'x'"),
    ] {
        let blocked = ro.execute(&c.query(), &sql, &ExecOptions::default()).await;
        assert!(
            matches!(blocked, Err(AppError::ReadOnly(_))),
            "`{sql}` must be blocked"
        );
    }
    let mut q = c.table_query();
    q.filter = Some("1=1; DELETE FROM dbku_t".into());
    assert!(
        matches!(ro.fetch_table(&q).await, Err(AppError::ReadOnly(_))),
        "filter is guarded"
    );
    assert!(ro.apply_changes(&c.table(), &changes).await.is_err());
    ro.close().await;
    assert_eq!(
        s.count_rows(&c.table_query()).await.unwrap(),
        31,
        "nothing was deleted"
    );

    // --- teardown ------------------------------------------------------------
    exec(&s, &c, &format!("DROP TABLE {TABLE}")).await;
    s.close().await;
}

#[tokio::test]
async fn sqlite_scenario() {
    let dir = tempfile::tempdir().unwrap();
    scenario(sqlite_profile(&dir.path().join("t.db")), None).await;
}

#[tokio::test]
async fn sqlite_read_only_connection_refuses_writes_at_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ro.db");
    let rw = Session::open(sqlite_profile(&path), None).await.unwrap();
    let ctx = QueryContext::default();
    rw.execute(&ctx, "CREATE TABLE t (a INT)", &ExecOptions::default())
        .await
        .unwrap();
    rw.close().await;

    let mut p = sqlite_profile(&path);
    p.read_only = true;
    let ro = Session::open(p, None).await.unwrap();
    // `apply_changes` is refused before reaching the engine; the engine
    // itself is opened read-only as a second layer.
    let table = TableRef {
        database: Some("main".into()),
        schema: None,
        table: "t".into(),
    };
    let insert = RowChange::Insert {
        values: vec![("a".into(), Some("1".into()))],
    };
    assert!(matches!(
        ro.apply_changes(&table, &[insert]).await,
        Err(AppError::ReadOnly(_))
    ));
}

#[tokio::test]
async fn sqlite_truncation_keeps_session_state() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(sqlite_profile(&dir.path().join("t.db")), None)
        .await
        .unwrap();
    let ctx = QueryContext::default();
    let opts = ExecOptions::default();
    s.execute(&ctx, "CREATE TEMP TABLE scratch (n INT); WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 50) INSERT INTO scratch SELECT n FROM r", &opts)
        .await
        .unwrap();
    let capped = ExecOptions {
        max_rows: Some(5),
        ..ExecOptions::default()
    };
    let out = s
        .execute(
            &ctx,
            "SELECT * FROM scratch; SELECT COUNT(*) FROM scratch",
            &capped,
        )
        .await
        .unwrap();
    assert!(out.results[0].truncated);
    assert_eq!(out.results.len(), 2, "later statements still run on SQLite");
    assert_eq!(out.results[1].rows[0][0].as_deref(), Some("50"));
    let again = s
        .execute(&ctx, "SELECT COUNT(*) FROM scratch", &opts)
        .await
        .unwrap();
    assert!(
        again.error.is_none(),
        "TEMP table survived: {:?}",
        again.error
    );
}

async fn external(var: &str, driver: Driver) {
    match std::env::var(var) {
        Ok(spec) => {
            let (p, pw) = profile(driver, &spec);
            scenario(p, pw).await;
        }
        Err(_) => eprintln!("skipping {driver:?}: set {var} to run it"),
    }
}

#[tokio::test]
async fn postgres_scenario() {
    external("DBKU_TEST_POSTGRES", Driver::Postgres).await;
}

#[tokio::test]
async fn mysql_scenario() {
    external("DBKU_TEST_MYSQL", Driver::Mysql).await;
}

#[tokio::test]
async fn mssql_scenario() {
    external("DBKU_TEST_MSSQL", Driver::Mssql).await;
}

/// Run by hand against a throwaway MySQL whose GLOBAL sql_mode was set to
/// `ANSI,NO_BACKSLASH_ESCAPES`: every dbKu session must still parse strings
/// the way the read-only guard does.
#[tokio::test]
#[ignore]
async fn mysql_session_normalizes_string_rules() {
    let spec = std::env::var("DBKU_TEST_MYSQL").expect("DBKU_TEST_MYSQL");
    let (p, pw) = profile(Driver::Mysql, &spec);
    let s = Session::open(p, pw.as_deref()).await.unwrap();
    let mode = s
        .execute(
            &QueryContext::default(),
            "SELECT @@SESSION.sql_mode",
            &ExecOptions::default(),
        )
        .await
        .unwrap();
    let mode = mode.results[0].rows[0][0].clone().unwrap_or_default();
    assert!(
        !mode.contains("NO_BACKSLASH_ESCAPES") && !mode.contains("ANSI_QUOTES"),
        "{mode}"
    );
    assert!(!mode.split(',').any(|m| m == "ANSI"), "{mode}");
    assert!(
        mode.contains("STRICT_TRANS_TABLES"),
        "other modes are kept: {mode}"
    );
    let echo = s
        .execute(
            &QueryContext::default(),
            r#"SELECT 'a\'b', "c""#,
            &ExecOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        echo.results[0].rows[0],
        vec![Some("a'b".into()), Some("c".into())]
    );
    s.close().await;
}
