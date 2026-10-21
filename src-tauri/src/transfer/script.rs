//! Running a .sql file from disk.

use std::path::Path;

use super::Progress;
use crate::db::Session;
use crate::error::{AppError, AppResult};
use crate::model::{QueryContext, ScriptOutcome};

pub async fn run_file(
    s: &Session,
    ctx: &QueryContext,
    path: &Path,
    progress: &dyn Progress,
) -> AppResult<ScriptOutcome> {
    // Reading a large file must not hold up an async worker.
    let owned = path.to_path_buf();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(owned))
        .await
        .map_err(|e| AppError::Storage(e.to_string()))??;
    let text = String::from_utf8(bytes).map_err(|_| {
        AppError::Invalid("The file isn't UTF-8 text. Save it as UTF-8 and try again.".into())
    })?;
    s.run_script(ctx, text.trim_start_matches('\u{feff}'), progress)
        .await
}
