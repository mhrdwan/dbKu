pub mod commands;
pub mod db;
pub mod error;
pub mod model;
pub mod saved;
pub mod sql;
pub mod store;
pub mod transfer;
pub mod transfer_commands;

use tauri::Manager;

use commands::AppState;
use store::{ConnectionStore, Keychain};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            let path = dir.join("connections.json");
            let saved = saved::SavedQueryStore::new(dir.join("saved_queries.json"));
            app.manage(AppState::new(
                ConnectionStore::new(path, Box::new(Keychain)),
                saved,
            ));
            Ok(())
        })
        .invoke_handler(handler())
        .run(tauri::generate_context!())
        .expect("error while running dbKu");
}

/// Every IPC command the UI may call. Shared with the IPC contract tests.
pub fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    tauri::generate_handler![
        commands::list_connections,
        commands::save_connection,
        commands::delete_connection,
        commands::test_connection,
        commands::connect,
        commands::disconnect,
        commands::list_databases,
        commands::list_schemas,
        commands::list_objects,
        commands::table_columns,
        commands::table_indexes,
        commands::table_ddl,
        commands::search_objects,
        commands::execute_query,
        commands::cancel_query,
        commands::close_session,
        commands::fetch_table,
        commands::count_rows,
        commands::preview_changes,
        commands::apply_changes,
        transfer_commands::pick_file,
        transfer_commands::preview_csv,
        transfer_commands::export_dump,
        transfer_commands::import_csv,
        transfer_commands::run_sql_file,
        transfer_commands::cancel_task,
        transfer_commands::list_saved_queries,
        transfer_commands::save_query,
        transfer_commands::delete_saved_query,
    ]
}
