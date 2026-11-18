//! `library select check`: what the scheduled task `selective-update-check` runs. It fetches
//! every source that has a selection, builds the plan and records what is waiting in
//! `updates.json`; it never applies anything.
//!
//! A check that is asked to deepen also compares each pinned commit with the whole history of
//! its source; [`rewritten`] lists the sources that lost it, which the attention list shows.

use super::ops::{self, Env};
use super::store;
use crate::context::fsx;
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::Path;

pub const UPDATES_FILE: &str = "updates.json";

pub struct Request<'a> {
    pub fetch: bool,
    /// Only this source; the others keep what the last check recorded.
    pub source: Option<&'a str>,
    /// Look at the whole history of each source to see whether its pinned commit is still in it.
    pub deepen: bool,
}

fn rows_of(doc: &Value) -> Vec<Value> {
    doc.get("sources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn recorded(base: &Path) -> Option<Value> {
    fsx::read_text(&base.join(UPDATES_FILE), 1024 * 1024)
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
}

/// A history verdict stays until the source is pinned elsewhere or looked at again: a check that
/// does not deepen has nothing new to say about it.
fn carry_history(row: &mut Value, before: Option<&Value>) {
    if row["history"]["state"] != json!("unchecked") {
        return;
    }
    let Some(old) = before.map(|b| &b["history"]).filter(|h| {
        h["state"].is_string()
            && h["state"] != json!("unchecked")
            && h["pinned"] == row["history"]["pinned"]
    }) else {
        return;
    };
    row["history"] = old.clone();
}

pub fn run(env: &Env, request: Request) -> Result<Value, OpError> {
    let base = env.base();
    let selections = store::load(&base)?;
    if let Some(source) = request.source {
        if !selections.sources.contains_key(source) {
            return Err(OpError::not_found(format!(
                "{source} has no selection; `library select ls` lists the sources that do"
            )));
        }
    }
    let before = recorded(&base).map(|doc| rows_of(&doc)).unwrap_or_default();
    let previous = |key: &str| before.iter().find(|r| r["source"] == json!(key));
    let checked_at = fsx::now_zulu();
    let mut rows = Vec::new();
    for key in selections.sources.keys() {
        if request.source.is_some_and(|only| only != key) {
            rows.extend(previous(key).cloned());
            continue;
        }
        let mut row = match ops::summary(env, key, request.fetch, request.deepen) {
            Ok(summary) => summary,
            Err(error) => json!({"source": key, "checked": false, "error": error.message}),
        };
        if row["history"].is_object() {
            carry_history(&mut row, previous(key));
            if row["history"]["state"] != json!("unchecked")
                && row["history"].get("checkedAt").is_none()
            {
                row["history"]["checkedAt"] = json!(checked_at);
            }
        }
        rows.push(row);
    }
    let waiting = rows.iter().filter(|r| r["waiting"] == json!(true)).count();
    let doc = json!({"checkedAt": checked_at, "sources": rows});
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

/// What the last check recorded about the history of `source`, if it looked at the commit the
/// source is pinned to now.
pub fn recorded_history(base: &Path, source: &str, pinned: Option<&str>) -> Value {
    let unchecked = json!({"state": "unchecked", "pinned": pinned});
    let Some(doc) = recorded(base) else {
        return unchecked;
    };
    rows_of(&doc)
        .into_iter()
        .find(|row| row["source"] == json!(source))
        .map(|row| row["history"].clone())
        .filter(|h| h["state"].is_string() && h["pinned"] == json!(pinned))
        .unwrap_or(unchecked)
}

/// The sources the last check found a plan waiting for.
pub fn pending(base: &Path) -> Vec<Value> {
    recorded(base)
        .map(|doc| rows_of(&doc))
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s["waiting"] == json!(true))
        .collect()
}

/// A source whose pinned commit the remote no longer has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rewritten {
    pub source: String,
    pub pinned: String,
    pub latest: String,
}

/// The sources the last deepening check found rewritten and that are still pinned to the commit
/// it looked for; pinning elsewhere ends the finding.
pub fn rewritten(base: &Path) -> Vec<Rewritten> {
    let Ok(selections) = store::load(base) else {
        return Vec::new();
    };
    recorded(base)
        .map(|doc| rows_of(&doc))
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row["history"]["state"] == json!("rewritten"))
        .filter_map(|row| {
            let source = row["source"].as_str()?.to_string();
            let pinned = row["history"]["pinned"].as_str()?.to_string();
            let now = selections.sources.get(&source)?.pinned.as_deref()?;
            (now == pinned).then(|| Rewritten {
                latest: row["latest"].as_str().unwrap_or_default().to_string(),
                source,
                pinned,
            })
        })
        .collect()
}
