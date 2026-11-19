//! What the app's notifier last learned about the system's permission. Only the app process can
//! ask the system, so it leaves the answer here for `attention settings` to read.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toolport_plus_api::Permission;

use crate::fsx::{atomic_write, Mode};
use crate::tasks::cron::rfc3339;

const FILE: &str = "attention-notifier.json";

fn file(base: &Path) -> PathBuf {
    base.join("plus").join(FILE)
}

/// Records the permission the app saw; a failed write only leaves the older answer in place.
pub fn record(base: &Path, permission: Permission, now: i64) {
    let text = json!({"permission": permission.word(), "at": now}).to_string() + "\n";
    let _ = atomic_write(&file(base), &text, Mode::Private);
}

/// The last answer, as `attention settings` reports it. Without a running app there is none, and
/// that is `unknown`, not `allowed`.
pub fn read(base: &Path) -> Value {
    let found: Option<Value> = std::fs::read_to_string(file(base))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let state = found
        .as_ref()
        .and_then(|v| Permission::parse(v["permission"].as_str()?))
        .unwrap_or(Permission::Unknown);
    let at = found.as_ref().and_then(|v| v["at"].as_i64());
    json!({"state": state.word(), "checkedAt": at.map(rfc3339)})
}
