//! Why Claude Code may have stopped exporting: sessions that ran after the newest event, sessions
//! that were already open at setup, and project settings that point the export elsewhere.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Value};

use super::transcript::TranscriptSession;

const ENDPOINT_KEY: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
const SETTINGS_FILES: [&str; 2] = ["settings.json", "settings.local.json"];
const FOLDERS_CHECKED: usize = 25;

fn overrides(folder: &Path, endpoint: &str) -> Vec<String> {
    SETTINGS_FILES
        .iter()
        .map(|name| folder.join(".claude").join(name))
        .filter(|path| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|v| v["env"][ENDPOINT_KEY].as_str().map(|s| s != endpoint))
                .unwrap_or(false)
        })
        .map(|path| path.display().to_string())
        .collect()
}

pub fn check(
    transcripts: &BTreeMap<String, TranscriptSession>,
    last_event_ms: i64,
    set_up_ms: Option<i64>,
    endpoint: &str,
) -> Value {
    let after: Vec<&TranscriptSession> = transcripts
        .values()
        .filter(|t| t.first_ts > last_event_ms)
        .collect();
    let folders: BTreeSet<&str> = after.iter().filter_map(|t| t.cwd.as_deref()).collect();
    let mut found = Vec::new();
    for folder in folders.into_iter().take(FOLDERS_CHECKED) {
        found.extend(overrides(Path::new(folder), endpoint));
    }
    let before_setup = set_up_ms.map_or(0, |at| {
        transcripts
            .values()
            .filter(|t| t.first_ts < at && t.last_ts > at)
            .count()
    });
    json!({
        "sessionsSince": after.len(),
        "startedBeforeSetup": before_setup,
        "projectOverrides": found,
    })
}
