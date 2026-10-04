//! The `AppHandle` side of the bridge: one emitter and the three IPC commands. Everything that
//! can be tested without a window lives in the parent module.

use super::{Bridge, Emitter, JobEvent, JobResult, Secret, EVENT_NAME};
use std::sync::Arc;
use tauri::{AppHandle, Emitter as _, Manager};

struct AppEmitter(AppHandle);

impl Emitter for AppEmitter {
    fn emit(&self, event: &JobEvent) {
        let _ = self.0.emit(EVENT_NAME, event);
    }
}

/// Starts `toolportctl --json <argv>` and returns the job id. A secret goes to the child's stdin
/// and nowhere else.
#[tauri::command]
pub async fn plus_ctl(
    app: AppHandle,
    argv: Vec<String>,
    stdin_secret: Option<String>,
) -> Result<String, String> {
    let secret = stdin_secret
        .map(Secret::new)
        .transpose()
        .map_err(|e| e.to_string())?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        handle
            .state::<Bridge>()
            .start(argv, secret, Arc::new(AppEmitter(handle.clone())))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("plus_ctl task join failed: {e}"))?
}

/// Waits for the job to finish and returns its result once.
#[tauri::command]
pub async fn plus_ctl_result(app: AppHandle, job: String) -> Result<JobResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Bridge>().wait(&job).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("plus_ctl_result task join failed: {e}"))?
}

/// Kills the job's process group; the child is reaped before this returns.
#[tauri::command]
pub async fn plus_ctl_cancel(app: AppHandle, job: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Bridge>().cancel(&job).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("plus_ctl_cancel task join failed: {e}"))?
}
