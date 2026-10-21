//! IPC contract: call the real commands with the exact JSON the UI sends
//! (see src/lib/api.ts), so a renamed argument or field fails here instead
//! of silently in the app.

use std::collections::HashMap;
use std::sync::Mutex;

use dbku_lib::commands::AppState;
use dbku_lib::error::AppResult;
use dbku_lib::store::{ConnectionStore, SecretStore};
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindow};

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, id: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn set(&self, id: &str, secret: &str) -> AppResult<()> {
        self.0.lock().unwrap().insert(id.into(), secret.into());
        Ok(())
    }
    fn delete(&self, id: &str) -> AppResult<()> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

struct Harness {
    webview: WebviewWindow<tauri::test::MockRuntime>,
    _dir: tempfile::TempDir,
}

impl Harness {
    fn new() -> (Self, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let store = ConnectionStore::new(
            dir.path().join("connections.json"),
            Box::new(MemorySecrets::default()),
        );
        let app = mock_builder()
            .invoke_handler(dbku_lib::handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        let saved = dbku_lib::saved::SavedQueryStore::new(dir.path().join("saved.json"));
        app.manage(AppState::new(store, saved));
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let db = dir.path().join("app.db");
        (Self { webview, _dir: dir }, db)
    }

    fn call(&self, cmd: &str, args: Value) -> Result<Value, Value> {
        let request = InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        };
        get_ipc_response(&self.webview, request).map(|body| body.deserialize::<Value>().unwrap())
    }

    fn ok(&self, cmd: &str, args: Value) -> Value {
        self.call(cmd, args)
            .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
    }
}

/// The profile shape ConnectionDialog.svelte sends.
fn sqlite_input(path: &std::path::Path, read_only: bool) -> Value {
    json!({
        "id": "", "name": "app", "driver": "sqlite", "host": "", "port": 0, "user": "",
        "database": path.to_string_lossy(), "sslMode": "prefer", "trustServerCert": false,
        "readOnly": read_only, "color": "#d64545", "group": null, "savePassword": true, "password": null
    })
}

#[test]
fn ui_payloads_round_trip_through_every_command() {
    let (h, db) = Harness::new();

    let saved = h.ok(
        "save_connection",
        json!({ "input": sqlite_input(&db, false) }),
    );
    let id = saved["id"].as_str().unwrap().to_string();
    assert!(!id.is_empty(), "a new connection gets an id");
    assert_eq!(saved["readOnly"], false);
    assert_eq!(h.ok("list_connections", json!({}))[0]["color"], "#d64545");

    let tested = h.ok(
        "test_connection",
        json!({ "input": sqlite_input(&db, false) }),
    );
    assert!(tested["version"].as_str().unwrap().starts_with("SQLite"));

    let info = h.ok("connect", json!({ "id": id, "password": null }));
    assert_eq!(info["defaultDatabase"], "main");
    assert_eq!(h.ok("list_databases", json!({ "id": id })), json!(["main"]));
    assert_eq!(
        h.ok("list_schemas", json!({ "id": id, "database": "main" })),
        json!([])
    );

    let ctx = json!({ "database": "main", "schema": null });
    let out = h.ok(
        "execute_query",
        json!({
            "id": id, "context": ctx,
            "sql": "CREATE TABLE t (a INTEGER PRIMARY KEY, b TEXT); \
                    INSERT INTO t VALUES (1, 'one'), (2, 'two'), (3, NULL); SELECT * FROM t ORDER BY a",
            "options": { "maxRows": 2, "runId": "run-1", "session": "tab-1" }
        }),
    );
    let last = out["results"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["rows"], json!([["1", "one"], ["2", "two"]]));
    assert_eq!(last["truncated"], true);
    assert_eq!(last["columns"][1]["name"], "b");
    assert!(out["error"].is_null());

    let table = json!({ "database": "main", "schema": null, "table": "t" });
    let objects = h.ok(
        "list_objects",
        json!({ "id": id, "database": "main", "schema": null }),
    );
    assert_eq!(objects, json!([{ "name": "t", "kind": "table" }]));
    let cols = h.ok("table_columns", json!({ "id": id, "table": table }));
    assert_eq!(cols[0]["isPrimaryKey"], true);
    assert_eq!(cols[1]["dataType"], "TEXT");
    assert!(h
        .ok("table_indexes", json!({ "id": id, "table": table }))
        .is_array());
    assert!(h
        .ok("table_ddl", json!({ "id": id, "table": table }))
        .as_str()
        .unwrap()
        .contains("CREATE TABLE"));

    // TableData.svelte's query shape.
    let query = json!({
        "database": "main", "schema": null, "table": "t", "limit": 50, "offset": 0,
        "sort": [{ "column": "a", "dir": "desc" }], "filter": "a < 3", "search": null, "searchColumns": []
    });
    let page = h.ok("fetch_table", json!({ "id": id, "query": query }));
    assert_eq!(page["rows"], json!([["2", "two"], ["1", "one"]]));
    assert_eq!(
        h.ok("count_rows", json!({ "id": id, "query": query })),
        json!(2)
    );

    // edits.ts toRowChanges() output.
    let changes = json!([
        { "type": "update", "key": [["a", "1"]], "values": [["b", "uno"]] },
        { "type": "delete", "key": [["a", "3"]] },
        { "type": "insert", "values": [["a", "4"], ["b", null]] }
    ]);
    let preview = h.ok(
        "preview_changes",
        json!({ "id": id, "table": table, "changes": changes }),
    );
    assert_eq!(preview[0], r#"UPDATE "t" SET "b" = 'uno' WHERE "a" = '1'"#);
    assert_eq!(
        h.ok(
            "apply_changes",
            json!({ "id": id, "table": table, "changes": changes })
        ),
        json!(3)
    );

    let hits = h.ok(
        "search_objects",
        json!({ "id": id, "database": "main", "term": "t" }),
    );
    assert!(hits.as_array().unwrap().iter().any(|h| h["table"] == "t"));
    assert_eq!(
        h.ok("cancel_query", json!({ "id": id, "runId": "nope" })),
        json!(false)
    );

    assert_eq!(
        h.ok("close_session", json!({ "id": id, "session": "tab-1" })),
        Value::Null
    );
    h.ok("disconnect", json!({ "id": id }));
    let err = h.call("list_databases", json!({ "id": id })).unwrap_err();
    assert_eq!(
        err["kind"], "not_connected",
        "errors carry a kind for the UI: {err}"
    );

    h.ok("delete_connection", json!({ "id": id }));
    assert_eq!(h.ok("list_connections", json!({})), json!([]));
}

#[test]
fn read_only_profile_is_enforced_through_ipc() {
    let (h, db) = Harness::new();
    let rw = h.ok(
        "save_connection",
        json!({ "input": sqlite_input(&db, false) }),
    );
    let rw_id = rw["id"].as_str().unwrap();
    h.ok("connect", json!({ "id": rw_id }));
    h.ok(
        "execute_query",
        json!({ "id": rw_id, "context": {}, "sql": "CREATE TABLE t (a INT PRIMARY KEY)" }),
    );

    let mut ro_input = sqlite_input(&db, true);
    ro_input["id"] = json!("ro");
    h.ok("save_connection", json!({ "input": ro_input }));
    h.ok("connect", json!({ "id": "ro" }));

    let blocked = h
        .call(
            "execute_query",
            json!({ "id": "ro", "context": {}, "sql": "SELECT 1; DROP TABLE t" }),
        )
        .unwrap_err();
    assert_eq!(blocked["kind"], "read_only", "{blocked}");

    let table = json!({ "database": "main", "schema": null, "table": "t" });
    let changes = json!([{ "type": "insert", "values": [["a", "1"]] }]);
    let refused = h
        .call(
            "apply_changes",
            json!({ "id": "ro", "table": table, "changes": changes }),
        )
        .unwrap_err();
    assert_eq!(refused["kind"], "read_only");

    let still_there = h.ok(
        "execute_query",
        json!({ "id": "ro", "context": {}, "sql": "SELECT COUNT(*) FROM t" }),
    );
    assert_eq!(still_there["results"][0]["rows"], json!([["0"]]));
}

#[test]
fn transfer_and_saved_query_commands() {
    let (h, db) = Harness::new();
    let state = h.webview.state::<AppState>();
    let saved = h.ok(
        "save_connection",
        json!({ "input": sqlite_input(&db, false) }),
    );
    let id = saved["id"].as_str().unwrap().to_string();
    h.ok("connect", json!({ "id": id }));
    h.ok("execute_query", json!({ "id": id, "context": {}, "sql": "CREATE TABLE t (a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t VALUES (1, 'x')" }));

    let dir = tempfile::tempdir().unwrap();
    // Dump through a save token.
    let out = dir.path().join("d.sql");
    let write_token = state.register_file(out.clone(), true);
    let summary = h.ok("export_dump", json!({
        "id": id, "token": write_token, "taskId": "t1", "database": "main", "schema": null,
        "objects": [{ "name": "t", "kind": "table" }],
        "options": { "structure": true, "data": true, "dropExisting": true, "rowsPerInsert": 100 }
    }));
    assert_eq!(summary["rows"], 1);
    assert!(std::fs::read_to_string(&out)
        .unwrap()
        .contains("INSERT INTO \"t\""));

    // A token picked for reading can't be used to write, and vice versa.
    let read_token = state.register_file(out.clone(), false);
    let refused = h.call("export_dump", json!({
        "id": id, "token": read_token, "taskId": "t2", "database": "main", "schema": null,
        "objects": [{ "name": "t", "kind": "table" }],
        "options": { "structure": true, "data": true, "dropExisting": true, "rowsPerInsert": 100 }
    })).unwrap_err();
    assert_eq!(refused["kind"], "invalid");
    assert!(h
        .call(
            "preview_csv",
            json!({ "token": write_token, "hasHeader": true })
        )
        .is_err());

    // CSV preview + import.
    let csv = dir.path().join("in.csv");
    std::fs::write(&csv, "a,b\n2,y\n3,\n").unwrap();
    let token = state.register_file(csv, false);
    let preview = h.ok(
        "preview_csv",
        json!({ "token": token, "delimiter": null, "hasHeader": true }),
    );
    assert_eq!(preview["headers"], json!(["a", "b"]));
    let n = h.ok("import_csv", json!({
        "id": id, "token": token, "taskId": "t3",
        "table": { "database": "main", "schema": null, "table": "t" },
        "options": { "delimiter": ",", "hasHeader": true, "emptyAsNull": true, "truncateFirst": false,
                     "mapping": [{ "column": "a", "source": 0 }, { "column": "b", "source": 1 }] }
    }));
    assert_eq!(n, json!(2));

    // Running a SQL file.
    let script = dir.path().join("s.sql");
    std::fs::write(&script, "UPDATE t SET b = 'z' WHERE a = 3; SELECT 1;").unwrap();
    let token = state.register_file(script, false);
    let outcome = h.ok(
        "run_sql_file",
        json!({ "id": id, "token": token, "taskId": "t4", "context": {} }),
    );
    assert_eq!(outcome["executed"], 2);
    assert!(outcome["error"].is_null());
    assert_eq!(
        h.ok("cancel_task", json!({ "taskId": "t4" })),
        json!(false),
        "finished tasks are gone"
    );

    // Saved queries.
    let q = h.ok(
        "save_query",
        json!({ "query": { "connId": id, "name": "Count", "sql": "SELECT COUNT(*) FROM t" } }),
    );
    let qid = q["id"].as_str().unwrap().to_string();
    assert_eq!(h.ok("list_saved_queries", json!({}))[0]["name"], "Count");
    h.ok("delete_saved_query", json!({ "id": qid }));
    h.ok("save_query", json!({ "query": { "connId": id, "name": "Kept until the connection goes", "sql": "SELECT 1" } }));
    h.ok("delete_connection", json!({ "id": id }));
    assert_eq!(h.ok("list_saved_queries", json!({})), json!([]));
}
