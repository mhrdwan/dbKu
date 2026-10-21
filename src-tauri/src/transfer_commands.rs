//! IPC commands for saved queries and data transfer (dump, CSV import,
//! running SQL files). Files are chosen in native dialogs opened from Rust;
//! the UI only receives a token for the chosen path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::model::{DbObject, QueryContext, ScriptOutcome, TableRef};
use crate::saved::SavedQuery;
use crate::transfer::csv_import::{self, CsvImportOptions, CsvPreview};
use crate::transfer::dump::{self, DumpOptions, DumpSummary};
use crate::transfer::{script, Progress};

type Cmd<'a> = State<'a, AppState>;

pub const PROGRESS_EVENT: &str = "transfer-progress";
const EMIT_EVERY: Duration = Duration::from_millis(120);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    task_id: String,
    done: u64,
    total: Option<u64>,
    label: String,
}

/// Sends throttled progress events and exposes the task's cancel flag.
struct EventProgress<R: Runtime> {
    app: AppHandle<R>,
    task_id: String,
    cancel: Arc<AtomicBool>,
    last: Mutex<Option<Instant>>,
}

impl<R: Runtime> Progress for EventProgress<R> {
    fn report(&self, done: u64, total: Option<u64>, label: &str) {
        let finished = total.is_some_and(|t| done >= t);
        if let Ok(mut last) = self.last.lock() {
            if !finished && last.is_some_and(|l| l.elapsed() < EMIT_EVERY) {
                return;
            }
            *last = Some(Instant::now());
        }
        let _ = self.app.emit(
            PROGRESS_EVENT,
            ProgressEvent {
                task_id: self.task_id.clone(),
                done,
                total,
                label: label.to_string(),
            },
        );
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Runs `work` as a cancellable task, unregistering it however it ends.
async fn as_task<R: Runtime, T, F>(
    app: AppHandle<R>,
    state: &AppState,
    task_id: &str,
    work: F,
) -> AppResult<T>
where
    F: AsyncFnOnce(&EventProgress<R>) -> AppResult<T>,
{
    let progress = EventProgress {
        app,
        task_id: task_id.to_string(),
        cancel: state.tasks.start(task_id),
        last: Mutex::new(None),
    };
    let result = work(&progress).await;
    state.tasks.finish(task_id);
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    token: String,
    name: String,
    size: Option<u64>,
}

/// Open a native file dialog. `purpose` is "dump" (save) or "csv"/"sql" (open).
#[tauri::command]
pub async fn pick_file<R: Runtime>(
    app: AppHandle<R>,
    state: Cmd<'_>,
    purpose: String,
    suggested_name: Option<String>,
) -> AppResult<Option<PickedFile>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let dialog = app.dialog().file();
    let for_writing = purpose == "dump";
    match purpose.as_str() {
        "dump" => dialog
            .add_filter("SQL script", &["sql"])
            .set_file_name(suggested_name.unwrap_or_else(|| "dump.sql".into()))
            .save_file(move |p| {
                let _ = tx.send(p);
            }),
        "csv" => dialog
            .add_filter("CSV", &["csv", "tsv", "txt"])
            .pick_file(move |p| {
                let _ = tx.send(p);
            }),
        "sql" => dialog
            .add_filter("SQL script", &["sql", "txt"])
            .pick_file(move |p| {
                let _ = tx.send(p);
            }),
        other => return Err(AppError::Invalid(format!("unknown file purpose `{other}`"))),
    }
    let Some(chosen) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = chosen
        .into_path()
        .map_err(|e| AppError::Invalid(e.to_string()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let size = std::fs::metadata(&path).ok().map(|m| m.len());
    let token = state.register_file(path, for_writing);
    Ok(Some(PickedFile { token, name, size }))
}

#[tauri::command]
pub async fn preview_csv(
    state: Cmd<'_>,
    token: String,
    delimiter: Option<String>,
    has_header: bool,
) -> AppResult<CsvPreview> {
    let path = state.picked(&token, false)?;
    csv_import::preview(
        &path,
        delimiter.as_deref().filter(|d| !d.is_empty()),
        has_header,
    )
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn export_dump<R: Runtime>(
    app: AppHandle<R>,
    state: Cmd<'_>,
    id: String,
    token: String,
    task_id: String,
    database: Option<String>,
    schema: Option<String>,
    objects: Vec<DbObject>,
    options: DumpOptions,
) -> AppResult<DumpSummary> {
    if objects.is_empty() {
        return Err(AppError::Invalid(
            "choose at least one table or view".into(),
        ));
    }
    let path = state.picked(&token, true)?;
    let session = state.session(&id).await?;
    let summary = as_task(app, &state, &task_id, async |p| {
        dump::dump_to_file(
            &session,
            database.as_deref(),
            schema.as_deref(),
            &objects,
            &options,
            &path,
            p,
        )
        .await
    })
    .await?;
    state.forget_file(&token);
    Ok(summary)
}

#[tauri::command]
pub async fn import_csv<R: Runtime>(
    app: AppHandle<R>,
    state: Cmd<'_>,
    id: String,
    token: String,
    task_id: String,
    table: TableRef,
    options: CsvImportOptions,
) -> AppResult<u64> {
    let path = state.picked(&token, false)?;
    let session = state.session(&id).await?;
    as_task(app, &state, &task_id, async |p| {
        csv_import::import_csv(&session, &table, &path, &options, p).await
    })
    .await
}

#[tauri::command]
pub async fn run_sql_file<R: Runtime>(
    app: AppHandle<R>,
    state: Cmd<'_>,
    id: String,
    token: String,
    task_id: String,
    context: QueryContext,
) -> AppResult<ScriptOutcome> {
    let path = state.picked(&token, false)?;
    let session = state.session(&id).await?;
    as_task(app, &state, &task_id, async |p| {
        script::run_file(&session, &context, &path, p).await
    })
    .await
}

#[tauri::command]
pub async fn cancel_task(state: Cmd<'_>, task_id: String) -> AppResult<bool> {
    Ok(state.tasks.cancel(&task_id))
}

fn saved_store<T>(
    state: &AppState,
    f: impl FnOnce(&crate::saved::SavedQueryStore) -> AppResult<T>,
) -> AppResult<T> {
    let store = state
        .saved
        .lock()
        .map_err(|_| AppError::Storage("saved queries lock poisoned".into()))?;
    f(&store)
}

#[tauri::command]
pub async fn list_saved_queries(state: Cmd<'_>) -> AppResult<Vec<SavedQuery>> {
    saved_store(&state, |s| s.list())
}

#[tauri::command]
pub async fn save_query(state: Cmd<'_>, query: SavedQuery) -> AppResult<SavedQuery> {
    saved_store(&state, |s| s.save(query))
}

#[tauri::command]
pub async fn delete_saved_query(state: Cmd<'_>, id: String) -> AppResult<()> {
    saved_store(&state, |s| s.delete(&id))
}
