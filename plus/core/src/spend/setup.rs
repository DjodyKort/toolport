//! Turning collection on in Claude Code: five `env` keys in each profile's `settings.json`,
//! written through the host's settings writer after a plan. A key that already holds a different
//! value is never overwritten without an explicit choice, and Undo restores only keys Toolport
//! wrote and that still hold what it wrote. The three content flags are never written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use toolport_plus_api::SettingsStore;

use super::config;

pub const UNKNOWN_PROFILE: &str = "no Claude Code profile";
pub const LEDGER_FILE: &str = "setup.json";
const ENV_KEY: &str = "env";

pub fn wanted(port: u16) -> Vec<(&'static str, String)> {
    vec![
        ("CLAUDE_CODE_ENABLE_TELEMETRY", "1".to_string()),
        ("OTEL_METRICS_EXPORTER", "otlp".to_string()),
        ("OTEL_LOGS_EXPORTER", "otlp".to_string()),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json".to_string()),
        (
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            format!("http://127.0.0.1:{port}"),
        ),
    ]
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Written {
    value: String,
    previous: Option<Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileLedger {
    env_existed: bool,
    written: BTreeMap<String, Written>,
    applied_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Ledger {
    #[serde(default)]
    profiles: BTreeMap<String, ProfileLedger>,
}

fn ledger_path(dir: &Path) -> PathBuf {
    dir.join(LEDGER_FILE)
}

fn load_ledger(dir: &Path) -> Ledger {
    std::fs::read_to_string(ledger_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_ledger(dir: &Path, ledger: &Ledger) -> Result<(), String> {
    let text = serde_json::to_string_pretty(ledger).map_err(|e| e.to_string())? + "\n";
    crate::fsx::atomic_write(&ledger_path(dir), &text, crate::fsx::Mode::Private)
        .map_err(|e| format!("cannot write {}: {e}", ledger_path(dir).display()))
}

fn env_of(root: &Value) -> Result<Option<&Map<String, Value>>, String> {
    match root.get(ENV_KEY) {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(_) => Err("settings.json has an `env` that is not an object".to_string()),
    }
}

struct Entry {
    key: &'static str,
    wanted: String,
    current: Option<Value>,
    action: &'static str,
}

pub const ACTION_ADD: &str = "add";
pub const ACTION_UPDATE: &str = "update";
pub const ACTION_KEEP: &str = "already set";
pub const ACTION_CONFLICT: &str = "conflict";
pub const ACTION_OVERWRITE: &str = "overwrite";

fn entries(
    root: &Value,
    ledger: Option<&ProfileLedger>,
    port: u16,
    overwrite: &[String],
) -> Result<Vec<Entry>, String> {
    let env = env_of(root)?;
    Ok(wanted(port)
        .into_iter()
        .map(|(key, value)| {
            let current = env.and_then(|e| e.get(key)).cloned();
            let ours = ledger
                .and_then(|l| l.written.get(key))
                .is_some_and(|w| current.as_ref().and_then(Value::as_str) == Some(&w.value));
            let action = match &current {
                None => ACTION_ADD,
                Some(v) if v.as_str() == Some(&value) => ACTION_KEEP,
                Some(_) if ours => ACTION_UPDATE,
                Some(_) if overwrite.iter().any(|k| k == key || k == "all") => ACTION_OVERWRITE,
                Some(_) => ACTION_CONFLICT,
            };
            Entry {
                key,
                wanted: value,
                current,
                action,
            }
        })
        .collect())
}

fn profile_json(path: &Path, entries: &[Entry], exists: bool) -> Value {
    let conflicts = entries
        .iter()
        .filter(|e| e.action == ACTION_CONFLICT)
        .count();
    json!({
        "profile": path.display().to_string(),
        "exists": exists,
        "entries": entries.iter().map(|e| json!({
            "key": e.key,
            "wanted": e.wanted,
            "current": e.current,
            "action": e.action,
        })).collect::<Vec<_>>(),
        "conflicts": conflicts,
        "writes": entries.iter().filter(|e| matches!(e.action, ACTION_ADD | ACTION_UPDATE | ACTION_OVERWRITE)).count(),
    })
}

fn matches_profile(path: &Path, wanted: &str) -> bool {
    path.to_string_lossy() == wanted || path.parent().is_some_and(|p| p.to_string_lossy() == wanted)
}

/// `names` is the settings path or its folder; empty selects every profile.
fn select(paths: Vec<PathBuf>, names: &[String]) -> Result<Vec<PathBuf>, String> {
    if names.is_empty() {
        return Ok(paths);
    }
    if let Some(unknown) = names
        .iter()
        .find(|n| !paths.iter().any(|p| matches_profile(p, n)))
    {
        return Err(format!("{UNKNOWN_PROFILE} '{unknown}'"));
    }
    Ok(paths
        .into_iter()
        .filter(|p| names.iter().any(|n| matches_profile(p, n)))
        .collect())
}

pub fn plan(
    store: &dyn SettingsStore,
    dir: &Path,
    port: u16,
    overwrite: &[String],
    only: &[String],
) -> Result<Value, String> {
    let ledger = load_ledger(dir);
    let mut profiles = Vec::new();
    for path in select(store.claude_settings_paths(), only)? {
        let (root, text) = store.read_settings(&path)?;
        let list = entries(
            &root,
            ledger.profiles.get(&path.to_string_lossy().into_owned()),
            port,
            overwrite,
        )?;
        profiles.push(profile_json(&path, &list, text.is_some()));
    }
    let writes: u64 = profiles
        .iter()
        .map(|p| p["writes"].as_u64().unwrap_or(0))
        .sum();
    let conflicts: u64 = profiles
        .iter()
        .map(|p| p["conflicts"].as_u64().unwrap_or(0))
        .sum();
    Ok(json!({
        "port": port,
        "endpoint": format!("http://127.0.0.1:{port}"),
        "profiles": profiles,
        "writes": writes,
        "conflicts": conflicts,
        "contentFlags": "never written",
        "note": if conflicts > 0 {
            "Keys that already hold another value stay as they are unless you choose to overwrite them; collection will not work until they match"
        } else {
            "Each profile's settings file is backed up by the settings writer before it changes"
        },
    }))
}

pub fn apply(
    store: &dyn SettingsStore,
    dir: &Path,
    port: u16,
    overwrite: &[String],
    only: &[String],
    now_text: &str,
) -> Result<Value, String> {
    let mut ledger = load_ledger(dir);
    let mut results = Vec::new();
    for path in select(store.claude_settings_paths(), only)? {
        let name = path.to_string_lossy().into_owned();
        let (root, text) = store.read_settings(&path)?;
        let list = entries(&root, ledger.profiles.get(&name), port, overwrite)?;
        let to_write: Vec<&Entry> = list
            .iter()
            .filter(|e| matches!(e.action, ACTION_ADD | ACTION_UPDATE | ACTION_OVERWRITE))
            .collect();
        let mut summary = profile_json(&path, &list, text.is_some());
        if to_write.is_empty() {
            summary["applied"] = json!(false);
            results.push(summary);
            continue;
        }
        let existed = env_of(&root)?.is_some();
        let mut env = env_of(&root)?.cloned().unwrap_or_default();
        let entry = ledger
            .profiles
            .entry(name)
            .or_insert_with(|| ProfileLedger {
                env_existed: existed,
                ..ProfileLedger::default()
            });
        for item in &to_write {
            env.insert(item.key.to_string(), Value::String(item.wanted.clone()));
            let previous = entry
                .written
                .get(item.key)
                .map_or(item.current.clone(), |w| w.previous.clone());
            entry.written.insert(
                item.key.to_string(),
                Written {
                    value: item.wanted.clone(),
                    previous,
                },
            );
        }
        entry.applied_at = now_text.to_string();
        let mut next = root.clone();
        next[ENV_KEY] = Value::Object(env);
        store.write_settings_key(&path, text.as_deref(), &next, ENV_KEY)?;
        summary["applied"] = json!(true);
        results.push(summary);
    }
    save_ledger(dir, &ledger)?;
    let mut cfg = config::load(dir);
    cfg.enabled = true;
    cfg.port = port;
    config::save(dir, &cfg)?;
    let complete = results.iter().all(|r| r["conflicts"] == 0);
    Ok(json!({"profiles": results, "port": port, "enabled": true, "complete": complete}))
}

/// Profiles Toolport wrote to that `only` names (all of them when empty).
fn undo_targets(ledger: &Ledger, only: &[String]) -> Result<Vec<String>, String> {
    let known: Vec<PathBuf> = ledger.profiles.keys().map(PathBuf::from).collect();
    Ok(select(known, only)?
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// How many profiles stay set up after undoing `only`.
pub fn remaining_after_undo(dir: &Path, only: &[String]) -> Result<usize, String> {
    let ledger = load_ledger(dir);
    Ok(ledger.profiles.len() - undo_targets(&ledger, only)?.len())
}

pub fn undo(store: &dyn SettingsStore, dir: &Path, only: &[String]) -> Result<Value, String> {
    let mut ledger = load_ledger(dir);
    let targets = undo_targets(&ledger, only)?;
    let mut results = Vec::new();
    for name in targets {
        let Some(entry) = ledger.profiles.remove(&name) else {
            continue;
        };
        let path = PathBuf::from(&name);
        let (root, text) = store.read_settings(&path)?;
        let mut env = env_of(&root)?.cloned().unwrap_or_default();
        let (mut restored, mut kept) = (Vec::new(), Vec::new());
        for (key, written) in &entry.written {
            if env.get(key).and_then(Value::as_str) != Some(&written.value) {
                kept.push(key.clone());
                continue;
            }
            match &written.previous {
                Some(previous) => env.insert(key.clone(), previous.clone()),
                None => env.remove(key),
            };
            restored.push(key.clone());
        }
        if !restored.is_empty() {
            let mut next = root.clone();
            if env.is_empty() && !entry.env_existed {
                next.as_object_mut().map(|o| o.remove(ENV_KEY));
            } else {
                next[ENV_KEY] = Value::Object(env);
            }
            store.write_settings_key(&path, text.as_deref(), &next, ENV_KEY)?;
        }
        results.push(json!({"profile": name, "restored": restored, "keptChanged": kept}));
    }
    save_ledger(dir, &ledger)?;
    let enabled = !ledger.profiles.is_empty();
    let mut cfg = config::load(dir);
    cfg.enabled = enabled;
    config::save(dir, &cfg)?;
    Ok(json!({"profiles": results, "enabled": enabled}))
}

pub fn applied_profiles(dir: &Path) -> usize {
    load_ledger(dir).profiles.len()
}

/// Where collection was written and when, for the Collecting card.
pub fn applied_info(dir: &Path) -> Value {
    Value::Array(
        load_ledger(dir)
            .profiles
            .iter()
            .map(|(profile, entry)| json!({"profile": profile, "appliedAt": entry.applied_at}))
            .collect(),
    )
}
