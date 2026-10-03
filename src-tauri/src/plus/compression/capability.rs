//! What a pinned headroom build can be told (static scan of its package for genuine env
//! reads) and what a live proxy is running (`/health.config`, passed in by the caller).
//! Diagnostic only: neither ever drops a knob from the emit path.

use super::model::OrderedMap;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

/// `/health.config` key to the env knob that sets it: the sealable posture surface.
pub const POSTURE: [(&str, &str); 11] = [
    (
        "compress_system_messages",
        "HEADROOM_COMPRESS_SYSTEM_MESSAGES",
    ),
    ("compress_user_messages", "HEADROOM_COMPRESS_USER_MESSAGES"),
    ("max_items_after_crush", "HEADROOM_MAX_ITEMS"),
    ("min_tokens_to_crush", "HEADROOM_MIN_TOKENS"),
    ("savings_profile", "HEADROOM_SAVINGS_PROFILE"),
    ("accuracy_guard", "HEADROOM_ACCURACY_GUARD"),
    ("protect_recent", "HEADROOM_PROTECT_RECENT"),
    (
        "protect_analysis_context",
        "HEADROOM_PROTECT_ANALYSIS_CONTEXT",
    ),
    (
        "smart_crusher_with_compaction",
        "HEADROOM_SMART_CRUSHER_COMPACTION",
    ),
    ("target_ratio", "HEADROOM_TARGET_RATIO"),
    ("force_kompress", "HEADROOM_FORCE_KOMPRESS_ALL"),
];

/// Knobs `agent-savings` prints but the build never reads, and the one that works instead.
const KNOWN_INERT: [(&str, Option<&str>); 2] = [
    (
        "HEADROOM_FORCE_KOMPRESS",
        Some("HEADROOM_FORCE_KOMPRESS_ALL"),
    ),
    ("HEADROOM_SAVINGS_TARGET", None),
];

fn read_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?:os\.environ(?:\.get)?\(|getenv\(|_get_env_bool\(|_get_env_int\(|_get_env_float\(|_env_bool\(|_env_str\(|environ\[)\s*["'](HEADROOM_[A-Z0-9_]+)["']"#,
        )
        .expect("static pattern")
    })
}

fn walk_py(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk_py(&path, out),
            Ok(t) if t.is_file() && path.extension().is_some_and(|e| e == "py") => out.push(path),
            _ => {}
        }
    }
}

/// Every `HEADROOM_*` knob the package genuinely reads. Mentions in help text, comments
/// and docstrings do not count: matching bare literals over-reports by about 30%.
pub fn read_knobs(root: &Path) -> BTreeSet<String> {
    let mut files = Vec::new();
    walk_py(root, &mut files);
    let mut found = BTreeSet::new();
    for file in files {
        if let Ok(bytes) = std::fs::read(&file) {
            let text = String::from_utf8_lossy(&bytes);
            for cap in read_pattern().captures_iter(&text) {
                found.insert(cap[1].to_string());
            }
        }
    }
    found
}

/// Declared knobs the build never reads, mapped to the knob that would work. An empty scan
/// reports nothing: it must never read as "nothing works".
pub fn inert_for(declared: &[String], root: &Path) -> BTreeMap<String, Option<String>> {
    let read = read_knobs(root);
    if read.is_empty() {
        return BTreeMap::new();
    }
    declared
        .iter()
        .filter(|k| !read.contains(*k))
        .map(|k| {
            let better = KNOWN_INERT
                .iter()
                .find(|(name, _)| name == k)
                .and_then(|(_, b)| b.map(str::to_string));
            (k.clone(), better)
        })
        .collect()
}

/// The env string that would reproduce a `/health.config` value; `None` for unset, which
/// is a distinct state from any literal and cannot be expressed as env.
pub fn as_env_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(if *b { "1" } else { "0" }.into()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Posture settings a live proxy holds that policy never declared (the vendor's choices).
/// An empty `effective` (no proxy) makes no claim.
pub fn unsealed_for(
    declared: &OrderedMap<super::model::KnobSpec>,
    effective: &Map<String, Value>,
) -> Vec<(String, Value)> {
    if effective.is_empty() {
        return Vec::new();
    }
    POSTURE
        .iter()
        .filter(|(cfg_key, env_knob)| {
            !declared.contains_key(env_knob) && effective.contains_key(*cfg_key)
        })
        .map(|(cfg_key, env_knob)| (env_knob.to_string(), effective[*cfg_key].clone()))
        .collect()
}

/// Partitions into (declarable, unset): a concrete value policy could pin, versus the
/// build's internal default, which is stable only because the pin is.
pub fn split_unsealed(unsealed: &[(String, Value)]) -> (Vec<(String, String)>, Vec<String>) {
    let mut declarable = Vec::new();
    let mut unset = Vec::new();
    for (knob, raw) in unsealed {
        match as_env_value(raw) {
            Some(v) => declarable.push((knob.clone(), v)),
            None => unset.push(knob.clone()),
        }
    }
    (declarable, unset)
}
