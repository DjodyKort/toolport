//! The IPC commands of the fork (D-060): `plus_ctl` starts `toolportctl --json <argv>` as a child
//! process, `plus_ctl_result` waits for its envelope and `plus_ctl_cancel` kills it.

use super::bridge::{Bridge, Emitter, JobEvent, JobResult, Secret};
use std::sync::{Arc, OnceLock};
use tauri::ipc::Invoke;
use tauri::{AppHandle, Emitter as _};

const EVENT_NAME: &str = "plus-ctl";

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

fn bridge() -> &'static Bridge {
    BRIDGE.get_or_init(Bridge::new)
}

struct AppEmitter(AppHandle);

impl Emitter for AppEmitter {
    fn emit(&self, event: &JobEvent) {
        let _ = self.0.emit(EVENT_NAME, event);
    }
}

/// Starts `toolportctl --json <argv>` and returns the job id. A secret goes to the child's stdin
/// and nowhere else.
#[tauri::command]
async fn plus_ctl(
    app: AppHandle,
    argv: Vec<String>,
    stdin_secret: Option<String>,
) -> Result<String, String> {
    let secret = stdin_secret
        .map(Secret::new)
        .transpose()
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        bridge()
            .start(argv, secret, Arc::new(AppEmitter(app)))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("plus_ctl task join failed: {e}"))?
}

/// Waits for the job to finish and returns its result once.
#[tauri::command]
async fn plus_ctl_result(job: String) -> Result<JobResult, String> {
    tauri::async_runtime::spawn_blocking(move || bridge().wait(&job).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("plus_ctl_result task join failed: {e}"))?
}

/// Kills the job's process group; the child is reaped before this returns.
#[tauri::command]
async fn plus_ctl_cancel(job: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || bridge().cancel(&job).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("plus_ctl_cancel task join failed: {e}"))?
}

fn dispatch(invoke: Invoke) -> bool {
    let own: fn(Invoke) -> bool =
        tauri::generate_handler![plus_ctl, plus_ctl_result, plus_ctl_cancel];
    own(invoke)
}

/// Answers the fork's commands by name and hands every other command to `next`.
pub(crate) fn handler(
    next: impl Fn(Invoke) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke) -> bool + Send + Sync + 'static {
    move |invoke| match invoke.message.command() {
        "plus_ctl" | "plus_ctl_result" | "plus_ctl_cancel" => dispatch(invoke),
        _ => next(invoke),
    }
}
