//! IPC surface. Every command returns `Result<T, AppError>`; the UI receives
//! errors as `{ kind, message }`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri::State;
use tokio::sync::RwLock;

use crate::db::{meta, Session};
use crate::error::{AppError, AppResult};
use crate::model::{
    ColumnInfo, ConnectionInput, ConnectionProfile, DbObject, ExecOptions, ExecOutcome, IndexInfo,
    QueryContext, ResultSet, RowChange, SearchHit, ServerInfo, TableQuery, TableRef,
};
use crate::saved::SavedQueryStore;
use crate::store::ConnectionStore;
use crate::transfer::Tasks;

/// A file the user picked in a native dialog. The UI only ever sees the
/// token, so it can't make the backend read or write any other path.
#[derive(Clone)]
pub(crate) struct PickedPath {
    pub path: std::path::PathBuf,
    pub for_writing: bool,
}

pub struct AppState {
    store: Mutex<ConnectionStore>,
    sessions: RwLock<HashMap<String, Arc<Session>>>,
    pub(crate) saved: Mutex<SavedQueryStore>,
    pub(crate) files: Mutex<HashMap<String, PickedPath>>,
    pub(crate) tasks: Tasks,
}

impl AppState {
    pub fn new(store: ConnectionStore, saved: SavedQueryStore) -> Self {
        Self {
            store: Mutex::new(store),
            sessions: RwLock::new(HashMap::new()),
            saved: Mutex::new(saved),
            files: Mutex::new(HashMap::new()),
            tasks: Tasks::default(),
        }
    }

    /// Register a user-chosen path and get the token the UI may use for it.
    pub fn register_file(&self, path: std::path::PathBuf, for_writing: bool) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        if let Ok(mut files) = self.files.lock() {
            files.insert(token.clone(), PickedPath { path, for_writing });
        }
        token
    }

    /// A save location is used once: a later export must ask again.
    pub(crate) fn forget_file(&self, token: &str) {
        if let Ok(mut files) = self.files.lock() {
            files.remove(token);
        }
    }

    pub(crate) fn picked(&self, token: &str, for_writing: bool) -> AppResult<std::path::PathBuf> {
        let files = self
            .files
            .lock()
            .map_err(|_| AppError::Storage("file list lock poisoned".into()))?;
        match files.get(token) {
            Some(p) if p.for_writing == for_writing => Ok(p.path.clone()),
            _ => Err(AppError::Invalid("choose the file again".into())),
        }
    }

    fn with_store<T>(&self, f: impl FnOnce(&ConnectionStore) -> AppResult<T>) -> AppResult<T> {
        let store = self
            .store
            .lock()
            .map_err(|_| AppError::Storage("store lock poisoned".into()))?;
        f(&store)
    }

    pub(crate) async fn session(&self, id: &str) -> AppResult<Arc<Session>> {
        self.sessions
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::NotConnected(id.to_string()))
    }

    /// Forget the session now; close it in the background so a long query
    /// can't make disconnect or save hang.
    async fn drop_session(&self, id: &str) {
        let removed = self.sessions.write().await.remove(id);
        if let Some(session) = removed {
            tauri::async_runtime::spawn(async move { session.close().await });
        }
    }
}

type Cmd<'a> = State<'a, AppState>;

#[tauri::command]
pub async fn list_connections(state: Cmd<'_>) -> AppResult<Vec<ConnectionProfile>> {
    state.with_store(|s| s.list())
}

#[tauri::command]
pub async fn save_connection(
    state: Cmd<'_>,
    input: ConnectionInput,
) -> AppResult<ConnectionProfile> {
    let mut profile = input.profile;
    if profile.id.trim().is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }
    let saved = state.with_store(|s| s.save(profile, input.password.as_deref()))?;
    // Settings such as read-only must apply now, not after a reconnect.
    state.drop_session(&saved.id).await;
    Ok(saved)
}

#[tauri::command]
pub async fn delete_connection(state: Cmd<'_>, id: String) -> AppResult<()> {
    state.drop_session(&id).await;
    state.with_store(|s| s.delete(&id))?;
    let saved = state
        .saved
        .lock()
        .map_err(|_| AppError::Storage("saved queries lock poisoned".into()))?;
    saved.delete_for_connection(&id)
}

/// Try a profile without saving it. Falls back to the stored password.
#[tauri::command]
pub async fn test_connection(state: Cmd<'_>, input: ConnectionInput) -> AppResult<ServerInfo> {
    let password = match input.password {
        Some(pw) => Some(pw),
        None if !input.profile.id.is_empty() => {
            state.with_store(|s| s.password(&input.profile.id))?
        }
        None => None,
    };
    let session = Session::open(input.profile, password.as_deref()).await?;
    let info = session.server_info().await;
    session.close().await;
    info
}

#[tauri::command]
pub async fn connect(
    state: Cmd<'_>,
    id: String,
    password: Option<String>,
) -> AppResult<ServerInfo> {
    let profile = state.with_store(|s| s.get(&id))?;
    let password = match password {
        Some(pw) => Some(pw),
        None => state.with_store(|s| s.password(&id))?,
    };
    state.drop_session(&id).await;
    let session = Arc::new(Session::open(profile, password.as_deref()).await?);
    let info = session.server_info().await?;
    state.sessions.write().await.insert(id, session);
    Ok(info)
}

#[tauri::command]
pub async fn disconnect(state: Cmd<'_>, id: String) -> AppResult<()> {
    state.drop_session(&id).await;
    Ok(())
}

#[tauri::command]
pub async fn list_databases(state: Cmd<'_>, id: String) -> AppResult<Vec<String>> {
    meta::databases(&*state.session(&id).await?).await
}

#[tauri::command]
pub async fn list_schemas(state: Cmd<'_>, id: String, database: String) -> AppResult<Vec<String>> {
    meta::schemas(&*state.session(&id).await?, &database).await
}

#[tauri::command]
pub async fn list_objects(
    state: Cmd<'_>,
    id: String,
    database: String,
    schema: Option<String>,
) -> AppResult<Vec<DbObject>> {
    meta::objects(&*state.session(&id).await?, &database, schema.as_deref()).await
}

#[tauri::command]
pub async fn table_columns(
    state: Cmd<'_>,
    id: String,
    table: TableRef,
) -> AppResult<Vec<ColumnInfo>> {
    meta::columns(&*state.session(&id).await?, &table).await
}

#[tauri::command]
pub async fn table_indexes(
    state: Cmd<'_>,
    id: String,
    table: TableRef,
) -> AppResult<Vec<IndexInfo>> {
    meta::indexes(&*state.session(&id).await?, &table).await
}

#[tauri::command]
pub async fn table_ddl(state: Cmd<'_>, id: String, table: TableRef) -> AppResult<String> {
    meta::ddl(&*state.session(&id).await?, &table).await
}

#[tauri::command]
pub async fn search_objects(
    state: Cmd<'_>,
    id: String,
    database: String,
    term: String,
) -> AppResult<Vec<SearchHit>> {
    let term = term.trim();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    meta::search_objects(&*state.session(&id).await?, &database, term).await
}

#[tauri::command]
pub async fn execute_query(
    state: Cmd<'_>,
    id: String,
    context: QueryContext,
    sql: String,
    options: Option<ExecOptions>,
) -> AppResult<ExecOutcome> {
    let session = state.session(&id).await?;
    session
        .execute(&context, &sql, &options.unwrap_or_default())
        .await
}

#[tauri::command]
pub async fn cancel_query(state: Cmd<'_>, id: String, run_id: String) -> AppResult<bool> {
    state.session(&id).await?.cancel(&run_id).await
}

/// A query tab closed (or switched connection): release its connection.
#[tauri::command]
pub async fn close_session(state: Cmd<'_>, id: String, session: String) -> AppResult<()> {
    if let Ok(s) = state.session(&id).await {
        s.close_tab(&session).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn fetch_table(state: Cmd<'_>, id: String, query: TableQuery) -> AppResult<ResultSet> {
    state.session(&id).await?.fetch_table(&query).await
}

#[tauri::command]
pub async fn count_rows(state: Cmd<'_>, id: String, query: TableQuery) -> AppResult<u64> {
    state.session(&id).await?.count_rows(&query).await
}

#[tauri::command]
pub async fn preview_changes(
    state: Cmd<'_>,
    id: String,
    table: TableRef,
    changes: Vec<RowChange>,
) -> AppResult<Vec<String>> {
    state.session(&id).await?.preview_changes(&table, &changes)
}

#[tauri::command]
pub async fn apply_changes(
    state: Cmd<'_>,
    id: String,
    table: TableRef,
    changes: Vec<RowChange>,
) -> AppResult<u64> {
    state
        .session(&id)
        .await?
        .apply_changes(&table, &changes)
        .await
}
