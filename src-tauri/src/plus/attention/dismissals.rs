//! `<data dir>/plus/attention.json`: id -> until date (or forever). The only thing Attention
//! stores. An expired entry is dropped on read; an id no feed returns right now can be
//! dismissed too, so a feed that comes back later stays hidden.

use crate::plus::op::OpError;
use crate::plus::tasks::cron;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    at: String,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    dismissed: BTreeMap<String, Entry>,
}

pub fn path() -> Option<PathBuf> {
    crate::registry::conduit_dir().map(|d| d.join("plus").join("attention.json"))
}

fn read() -> File {
    path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn live(entry: &Entry, today: &str) -> bool {
    entry.until.as_deref().is_none_or(|until| today < until)
}

/// The ids hidden on `today` (a `YYYY-MM-DD` date), each with its end date; `None` is forever.
pub fn load(today: &str) -> BTreeMap<String, Option<String>> {
    read().dismissed.into_iter().filter(|(_, e)| live(e, today)).map(|(id, e)| (id, e.until)).collect()
}

pub fn parse_until(text: &str) -> Result<String, OpError> {
    let usage = || OpError::usage(format!("--until must be a calendar date like 2026-10-31, got {text:?}"));
    if text.len() != 10 {
        return Err(usage());
    }
    let epoch = cron::parse_rfc3339(&format!("{text}T00:00:00Z")).ok_or_else(usage)?;
    if cron::rfc3339(epoch)[..10] != *text {
        return Err(usage());
    }
    Ok(text.to_string())
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control)
}

/// Hides `id` until `until` (it returns on that date) or for good. A date that has passed shows
/// the row again, which is how the preview's undo command works.
pub fn dismiss(id: &str, until: Option<&str>, now: i64, dry_run: bool) -> Result<Value, OpError> {
    if !valid_id(id) {
        return Err(OpError::usage("attention dismiss needs an id as `attention ls` prints it"));
    }
    let until = until.map(parse_until).transpose()?;
    let today = cron::rfc3339(now)[..10].to_string();
    let path = path().ok_or_else(|| OpError::failed("no_data_dir", "the data directory could not be resolved"))?;
    let shown = path.display().to_string();
    let restores = until.as_deref().is_some_and(|u| u <= today.as_str());
    let summary = if restores {
        format!("Show {id} again")
    } else {
        match &until {
            Some(date) => format!("Hide {id} until {date}"),
            None => format!("Hide {id} for good"),
        }
    };
    let detail = format!("{} in {shown}", if restores { "remove the entry" } else { "write the entry" });
    let undo = format!("toolportctl attention dismiss {id} --until {today}");
    if dry_run {
        let plan = json!({
            "summary": summary,
            "steps": [{"op": if restores { "delete" } else { "merge" }, "path": shown, "detail": detail, "keys": ["dismissed"]}],
            "effects": {},
            "warnings": [],
            "undo": undo,
        });
        return Ok(json!({"id": id, "until": until, "dryRun": true, "plan": plan}));
    }
    let mut file = read();
    file.dismissed.retain(|_, e| live(e, &today));
    if restores {
        file.dismissed.remove(id);
    } else {
        file.dismissed.insert(id.to_string(), Entry { until: until.clone(), at: cron::rfc3339(now) });
    }
    let text = serde_json::to_string_pretty(&file).unwrap_or_default() + "\n";
    crate::registry::atomic_write(&path, &text).map_err(|e| OpError::failed("write", format!("cannot write {shown}: {e}")))?;
    Ok(json!({
        "id": id,
        "until": until,
        "dryRun": false,
        "result": {"applied": true, "changed": [shown], "undo": undo, "backups": []},
    }))
}
