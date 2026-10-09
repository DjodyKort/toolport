//! Verifying the stack against Claude Code itself. The as-is measure request carries a
//! throwaway `InstructionsLoaded` hook that appends what Claude Code reports to a temp file; the
//! reported files are kept with the measurement and compared with the rows of `context loads`.
//! A row is `loaded` or `absent` when the log agrees with the stack, `mismatch` when it does not,
//! and a file Claude Code loaded that the stack has no row for is listed as `unknown`.

use super::loads::LoadItem;
use super::measure::MeasuredInfo;
use crate::plus::sources::fsx;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const LATER_REASONS: [&str; 2] = ["nested_traversal", "path_glob_match"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logged {
    pub path: String,
    pub memory_type: String,
    pub load_reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Mismatch {
    pub path: String,
    pub expected: &'static str,
    pub actual: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verified {
    pub status: &'static str,
    pub checked: usize,
    pub loaded: usize,
    pub absent: usize,
    pub mismatches: Vec<Mismatch>,
    pub unknown: Vec<String>,
    pub measured_at: String,
    pub claude_code_version: String,
    pub stale: bool,
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The `--settings` content that makes Claude Code append every `InstructionsLoaded` event to `log`.
pub fn hook_settings(log: &Path) -> Value {
    let target = shell_quote(&log.to_string_lossy());
    json!({
        "hooks": { "InstructionsLoaded": [ { "hooks": [ {
            "type": "command",
            "command": format!("cat >> {target}; echo >> {target}"),
        } ] } ] }
    })
}

/// The log file a settings file written by [`hook_settings`] points at.
pub fn log_target(settings: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(settings).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let command = value["hooks"]["InstructionsLoaded"][0]["hooks"][0]["command"].as_str()?;
    let rest = command.strip_prefix("cat >> '")?;
    Some(PathBuf::from(rest.split("';").next()?))
}

/// The events of one run that describe what Claude Code loaded at the start: the later ones
/// (a nested folder, a path-scoped rule) belong to files Claude reads afterwards.
pub fn parse_log(text: &str) -> Vec<Logged> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter_map(|event| {
            let path = event["file_path"].as_str()?.to_string();
            let load_reason = event["load_reason"].as_str().unwrap_or("").to_string();
            if LATER_REASONS.contains(&load_reason.as_str()) {
                return None;
            }
            Some(Logged {
                path,
                memory_type: event["memory_type"].as_str().unwrap_or("").to_string(),
                load_reason,
            })
        })
        .collect()
}

fn key(path: &str) -> String {
    fsx::display(&fsx::canonical(Path::new(path)))
}

/// Compare the stack with a logged run and mark every covered row. `None` when the measurement
/// holds no log (it predates verification, or the hook did not run).
pub fn compare(items: &mut [LoadItem], info: &MeasuredInfo) -> Option<Verified> {
    let logged = info.instructions.as_ref().filter(|events| !events.is_empty())?;
    let seen: BTreeSet<String> = logged.iter().map(|l| key(&l.path)).collect();
    let mut known = BTreeSet::new();
    let (mut loaded, mut absent) = (0, 0);
    let mut mismatches = Vec::new();
    for item in items.iter_mut() {
        let Some(path) = item.path.clone().filter(|_| matches!(item.kind, "memory" | "rule")) else {
            continue;
        };
        let at = key(&path);
        known.insert(at.clone());
        let expected = if item.loaded && !item.lazy {
            "loaded"
        } else if !item.loaded && !item.lazy && item.reason.starts_with("excluded") {
            "absent"
        } else {
            continue;
        };
        let actual = if seen.contains(&at) { "loaded" } else { "absent" };
        if expected == actual {
            item.confirmed = Some(expected);
            if expected == "loaded" {
                loaded += 1;
            } else {
                absent += 1;
            }
        } else {
            item.confirmed = Some("mismatch");
            mismatches.push(Mismatch { path, expected, actual });
        }
    }
    let unknown: Vec<String> = logged
        .iter()
        .filter(|l| !known.contains(&key(&l.path)))
        .map(|l| l.path.clone())
        .collect();
    Some(Verified {
        status: if mismatches.is_empty() && unknown.is_empty() { "match" } else { "mismatch" },
        checked: loaded + absent + mismatches.len(),
        loaded,
        absent,
        mismatches,
        unknown,
        measured_at: info.measured_at.clone(),
        claude_code_version: info.claude_code_version.clone(),
        stale: info.stale,
    })
}
