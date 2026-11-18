//! `library select check`: what the scheduled task `selective-update-check` runs. It fetches
//! every source that has a selection, builds the plan and records what is waiting in
//! `updates.json`; it never applies anything.
//!
//! [`pending`] is the hook the attention list reads to tell the user a plan is waiting. The
//! attention snapshot is not part of this branch, so nothing calls it yet.

use super::ops::{self, Env};
use super::store;
use crate::context::fsx;
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::Path;

pub const UPDATES_FILE: &str = "updates.json";

pub fn run(env: &Env, fetch: bool) -> Result<Value, OpError> {
    let base = env.base();
    let selections = store::load(&base)?;
    let mut rows = Vec::new();
    for key in selections.sources.keys() {
        let row = match ops::summary(env, key, fetch) {
            Ok(summary) => summary,
            Err(error) => json!({"source": key, "checked": false, "error": error.message}),
        };
        rows.push(row);
    }
    let waiting = rows.iter().filter(|r| r["waiting"] == json!(true)).count();
    let doc = json!({"checkedAt": fsx::now_zulu(), "sources": rows});
    let text = serde_json::to_string_pretty(&doc).map_err(|e| OpError::conflict(e.to_string()))?;
    let file = base.join(UPDATES_FILE);
    atomic_write(&file, &format!("{text}\n"), Mode::Private)
        .map_err(|e| OpError::conflict(format!("{}: {e}", file.display())))?;
    Ok(json!({
        "checkedAt": doc["checkedAt"],
        "waiting": waiting,
        "sources": doc["sources"],
        "applied": false,
    }))
}

/// The sources the last check found a plan waiting for.
pub fn pending(base: &Path) -> Vec<Value> {
    fsx::read_text(&base.join(UPDATES_FILE), 1024 * 1024)
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|d| d.get("sources").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s["waiting"] == json!(true))
        .collect()
}
