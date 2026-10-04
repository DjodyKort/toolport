//! Opt-in wiring between Claude Code's telemetry and the local OTLP receiver (MIG-OBS-3, D-051).
//!
//! `enable` writes the telemetry `env` block of the user's Claude `settings.json` and the receiver
//! config in the obs data dir; `disable` removes exactly what `enable` wrote. A key the user
//! already set to something else is never overwritten, and prompt or tool-content logging is
//! never switched on.

use super::receiver::{self, Probe};
use super::store::{iso_from_ms, stream_events};
use crate::plus::args::{flag, str_nonempty};
use crate::plus::context::backup::snapshot;
use crate::plus::context::Roots;
use crate::plus::jsonfs::read_json;
use crate::plus::skills::json::{parse, J};
use crate::plus::skills::pyfs::write_text;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "otel.json";
pub const STATUS_FILE: &str = "otel-receiver.json";

const ENV_TELEMETRY: &str = "CLAUDE_CODE_ENABLE_TELEMETRY";
const ENV_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
const CONTENT_ENV: [&str; 3] = [
    "OTEL_LOG_USER_PROMPTS",
    "OTEL_LOG_TOOL_DETAILS",
    "OTEL_LOG_TOOL_CONTENT",
];

pub fn endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn desired_env(port: u16) -> [(&'static str, String); 5] {
    [
        (ENV_TELEMETRY, "1".to_string()),
        ("OTEL_METRICS_EXPORTER", "otlp".to_string()),
        ("OTEL_LOGS_EXPORTER", "otlp".to_string()),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json".to_string()),
        (ENV_ENDPOINT, endpoint(port)),
    ]
}

fn default_port() -> u16 {
    receiver::DEFAULT_PORT
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Managed {
    #[serde(default)]
    pub settings_path: String,
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
    #[serde(default)]
    pub created_env: bool,
    #[serde(default)]
    pub created_file: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtelConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub managed: Managed,
}

impl Default for OtelConfig {
    fn default() -> Self {
        OtelConfig {
            enabled: false,
            port: default_port(),
            managed: Managed::default(),
        }
    }
}

pub fn load_config(dir: &Path) -> OtelConfig {
    read_json(&dir.join(CONFIG_FILE)).unwrap_or_default()
}

fn save_config(dir: &Path, config: &OtelConfig) -> Result<(), String> {
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    crate::registry::atomic_write(&dir.join(CONFIG_FILE), &format!("{text}\n"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostStatus {
    pub port: u16,
    pub pid: u32,
    pub state: String,
    #[serde(default)]
    pub message: String,
}

pub fn load_host_status(dir: &Path) -> Option<HostStatus> {
    read_json(&dir.join(STATUS_FILE))
}

pub fn save_host_status(dir: &Path, status: &HostStatus) {
    if let Ok(text) = serde_json::to_string(status) {
        let _ = crate::registry::atomic_write(&dir.join(STATUS_FILE), &text);
    }
}

pub fn clear_host_status(dir: &Path, pid: u32) {
    if load_host_status(dir).is_some_and(|s| s.pid == pid) {
        let _ = std::fs::remove_file(dir.join(STATUS_FILE));
    }
}

fn obs_dir() -> Result<PathBuf, String> {
    Ok(crate::registry::conduit_dir()
        .ok_or_else(|| "data directory unavailable".to_string())?
        .join("obs"))
}

struct Target {
    path: PathBuf,
    roots: Roots,
}

fn target(args: &Value, recorded: &str) -> Result<Target, String> {
    let explicit = str_nonempty(args, "home");
    let home = match explicit {
        Some(home) => PathBuf::from(home),
        None => dirs::home_dir().ok_or("cannot resolve the home directory")?,
    };
    let mut roots = Roots::from_home(&home);
    if explicit.is_none() {
        if !recorded.is_empty() {
            return Ok(Target {
                path: PathBuf::from(recorded),
                roots,
            });
        }
        if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
            roots.claude_home = PathBuf::from(dir);
        }
    }
    Ok(Target {
        path: roots.claude_home.join("settings.json"),
        roots,
    })
}

struct Settings {
    root: Vec<(String, J)>,
    existed: bool,
}

fn read_settings(path: &Path) -> Result<Settings, String> {
    if !path.exists() {
        return Ok(Settings {
            root: Vec::new(),
            existed: false,
        });
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match parse(&text) {
        Ok(J::Obj(root)) => Ok(Settings {
            root,
            existed: true,
        }),
        Ok(_) => Err(format!("{}: top level is not a JSON object", path.display())),
        Err(_) => Err(format!(
            "{}: not valid JSON; fix it first, Toolport does not rewrite it",
            path.display()
        )),
    }
}

fn env_of(root: &[(String, J)]) -> Result<Option<&Vec<(String, J)>>, String> {
    match root.iter().find(|(k, _)| k == "env") {
        None => Ok(None),
        Some((_, J::Obj(items))) => Ok(Some(items)),
        Some(_) => Err("settings.json: `env` is not an object".into()),
    }
}

fn env_mut(root: &mut Vec<(String, J)>) -> &mut Vec<(String, J)> {
    let at = match root.iter().position(|(k, _)| k == "env") {
        Some(at) => at,
        None => {
            root.push(("env".into(), J::Obj(Vec::new())));
            root.len() - 1
        }
    };
    match &mut root[at].1 {
        J::Obj(items) => items,
        _ => unreachable!("env_of rejected a non-object env"),
    }
}

fn text_of(value: &J) -> String {
    match value {
        J::Str(s) => s.clone(),
        J::Num(n) => n.clone(),
        J::Bool(b) => b.to_string(),
        _ => "\u{0}not-a-scalar".to_string(),
    }
}

fn current(env: Option<&Vec<(String, J)>>, key: &str) -> Option<String> {
    env?.iter().find(|(k, _)| k == key).map(|(_, v)| text_of(v))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Add,
    Update,
    Keep,
    Conflict,
}

fn plan_enable(
    env: Option<&Vec<(String, J)>>,
    managed: &Managed,
    port: u16,
) -> Vec<(&'static str, String, Change)> {
    desired_env(port)
        .into_iter()
        .map(|(key, want)| {
            let change = match (current(env, key), managed.keys.get(key)) {
                (None, _) => Change::Add,
                (Some(have), _) if have == want => Change::Keep,
                (Some(have), Some(ours)) if have == *ours => Change::Update,
                _ => Change::Conflict,
            };
            (key, want, change)
        })
        .collect()
}

fn content_warnings(env: Option<&Vec<(String, J)>>) -> Vec<String> {
    CONTENT_ENV
        .iter()
        .filter(|key| {
            current(env, key).is_some_and(|v| !matches!(v.to_ascii_lowercase().as_str(), "" | "0" | "false"))
        })
        .map(|key| {
            format!("env.{key} is set in your Claude settings; Toolport never sets it and drops prompt and tool content on receipt")
        })
        .collect()
}

fn set_env(items: &mut Vec<(String, J)>, key: &str, value: &str) {
    match items.iter_mut().find(|(k, _)| k == key) {
        Some((_, slot)) => *slot = J::str(value),
        None => items.push((key.to_string(), J::str(value))),
    }
}

fn write_settings(target: &Target, root: Vec<(String, J)>, existed: bool) -> Result<(), String> {
    if existed {
        snapshot(&target.roots, &target.path)?;
    }
    write_text(&target.path, &format!("{}\n", J::Obj(root).dumps()))
}

fn port_arg(args: &Value, fallback: u16) -> Result<u16, String> {
    match args.get("port") {
        None | Some(Value::Null) => Ok(fallback),
        Some(v) => v
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| *n != 0)
            .ok_or_else(|| "port must be a number between 1 and 65535".to_string()),
    }
}

pub fn enable_at(dir: &Path, args: &Value) -> Result<Value, String> {
    let dry_run = flag(args, "dryRun");
    let previous = load_config(dir);
    let port = port_arg(args, previous.port)?;
    let target = target(args, "")?;
    let recorded = &previous.managed.settings_path;
    if !previous.managed.keys.is_empty() && Path::new(recorded) != target.path {
        return Err(format!(
            "already enabled through {recorded}; run `toolportctl obs otel disable` first"
        ));
    }
    let settings = read_settings(&target.path)?;
    let env = env_of(&settings.root)?;
    let plan = plan_enable(env, &previous.managed, port);
    let warnings = content_warnings(env);
    let conflicts: Vec<&str> = plan
        .iter()
        .filter(|(_, _, change)| *change == Change::Conflict)
        .map(|(key, _, _)| *key)
        .collect();
    let report = |actions: Vec<String>, changed: bool, conflicts: &[&str]| {
        json!({
            "enabled": conflicts.is_empty(),
            "port": port,
            "endpoint": endpoint(port),
            "dryRun": dry_run,
            "changed": changed,
            "settingsPath": target.path.to_string_lossy(),
            "actions": actions,
            "conflicts": conflicts,
            "warnings": warnings,
        })
    };
    let actions: Vec<String> = plan
        .iter()
        .map(|(key, _, change)| {
            let verb = match change {
                Change::Add => "added",
                Change::Update => "updated",
                Change::Keep => "already set",
                Change::Conflict => "kept, set to another value",
            };
            format!("env.{key}: {verb}")
        })
        .collect();
    if !conflicts.is_empty() {
        return Ok(report(actions, false, &conflicts));
    }

    let mut managed = previous.managed.clone();
    managed.settings_path = target.path.to_string_lossy().into_owned();
    for (key, want, change) in &plan {
        match change {
            Change::Add | Change::Update => {
                managed.keys.insert((*key).to_string(), want.clone());
            }
            Change::Keep if managed.keys.contains_key(*key) => {
                managed.keys.insert((*key).to_string(), want.clone());
            }
            _ => {}
        }
    }
    let writes_settings = plan
        .iter()
        .any(|(_, _, change)| matches!(change, Change::Add | Change::Update));
    if writes_settings {
        managed.created_env |= env.is_none();
        managed.created_file |= !settings.existed;
    }
    let config = OtelConfig {
        enabled: true,
        port,
        managed,
    };
    let changed = writes_settings || config != previous;
    if dry_run || !changed {
        return Ok(report(actions, changed, &[]));
    }
    if writes_settings {
        let mut root = settings.root;
        let items = env_mut(&mut root);
        for (key, want, change) in &plan {
            if matches!(change, Change::Add | Change::Update) {
                set_env(items, key, want);
            }
        }
        write_settings(&target, root, settings.existed)?;
    }
    save_config(dir, &config)?;
    Ok(report(actions, true, &[]))
}

pub fn disable_at(dir: &Path, args: &Value) -> Result<Value, String> {
    let dry_run = flag(args, "dryRun");
    let previous = load_config(dir);
    let target = target(args, &previous.managed.settings_path)?;
    let mut actions = Vec::new();
    let mut kept = Vec::new();
    let mut settings_changed = false;
    let mut remove_file = false;
    let mut next_root = None;
    let settings = if previous.managed.keys.is_empty() {
        None
    } else {
        Some(read_settings(&target.path)?)
    };
    if let Some(settings) = settings {
        let env = env_of(&settings.root)?;
        let mut root = settings.root.clone();
        let mut removed_any = false;
        for (key, ours) in &previous.managed.keys {
            match current(env, key) {
                None => actions.push(format!("env.{key}: already absent")),
                Some(have) if have == *ours => {
                    if let Some((_, J::Obj(items))) = root.iter_mut().find(|(k, _)| k == "env") {
                        items.retain(|(k, _)| k != key);
                    }
                    actions.push(format!("env.{key}: removed"));
                    removed_any = true;
                }
                Some(_) => {
                    actions.push(format!("env.{key}: kept, changed since Toolport set it"));
                    kept.push(key.clone());
                }
            }
        }
        if removed_any {
            if previous.managed.created_env
                && matches!(root.iter().find(|(k, _)| k == "env"), Some((_, J::Obj(items))) if items.is_empty())
            {
                root.retain(|(k, _)| k != "env");
            }
            remove_file = previous.managed.created_file && root.is_empty();
            settings_changed = true;
            next_root = Some((root, settings.existed));
        }
    }
    let config_changed = previous.enabled || !previous.managed.keys.is_empty();
    let changed = settings_changed || config_changed;
    let report = json!({
        "enabled": false,
        "port": previous.port,
        "dryRun": dry_run,
        "changed": changed,
        "settingsPath": target.path.to_string_lossy(),
        "actions": actions,
        "kept": kept,
    });
    if dry_run || !changed {
        return Ok(report);
    }
    if let Some((root, existed)) = next_root {
        if remove_file {
            std::fs::remove_file(&target.path).map_err(|e| format!("{}: {e}", target.path.display()))?;
        } else {
            write_settings(&target, root, existed)?;
        }
    }
    save_config(
        dir,
        &OtelConfig {
            enabled: false,
            port: previous.port,
            managed: Managed::default(),
        },
    )?;
    Ok(report)
}

fn settings_status(target: &Target, port: u16) -> Value {
    let settings = match read_settings(&target.path) {
        Ok(s) => s,
        Err(message) => {
            return json!({"path": target.path.to_string_lossy(), "exists": true, "state": "unreadable", "error": message, "keys": {}, "warnings": []});
        }
    };
    let env = match env_of(&settings.root) {
        Ok(env) => env,
        Err(message) => {
            return json!({"path": target.path.to_string_lossy(), "exists": true, "state": "unreadable", "error": message, "keys": {}, "warnings": []});
        }
    };
    let mut keys = serde_json::Map::new();
    let mut ok = 0;
    for (key, want) in desired_env(port) {
        let state = match current(env, key) {
            None => "missing",
            Some(have) if have == want => {
                ok += 1;
                "ok"
            }
            Some(_) => "differs",
        };
        keys.insert(key.to_string(), json!(state));
    }
    let state = match ok {
        5 => "configured",
        0 => "missing",
        _ => "partial",
    };
    json!({
        "path": target.path.to_string_lossy(),
        "exists": settings.existed,
        "state": state,
        "keys": keys,
        "warnings": content_warnings(env),
    })
}

fn receiver_status(dir: &Path, config: &OtelConfig) -> Value {
    if !config.enabled {
        return json!({"state": "disabled", "listening": false});
    }
    let last = load_host_status(dir).filter(|s| s.port == config.port && s.state == "error");
    match receiver::probe(config.port) {
        Probe::Ours => json!({"state": "listening", "listening": true}),
        Probe::Foreign => json!({
            "state": "port-in-use",
            "listening": false,
            "error": last.map(|s| s.message).unwrap_or_else(|| format!("port {} is used by another program", config.port)),
        }),
        Probe::Closed => json!({
            "state": "stopped",
            "listening": false,
            "error": last.map(|s| s.message),
        }),
    }
}

pub fn status_at(dir: &Path, args: &Value) -> Result<Value, String> {
    let config = load_config(dir);
    let target = target(args, &config.managed.settings_path)?;
    let (mut count, mut latest) = (0u64, None::<i64>);
    for event in stream_events(dir) {
        count += 1;
        latest = latest.max(Some(event.ts_ms));
    }
    Ok(json!({
        "enabled": config.enabled,
        "port": config.port,
        "endpoint": endpoint(config.port),
        "settings": settings_status(&target, config.port),
        "receiver": receiver_status(dir, &config),
        "events": {"count": count, "latest": latest.map(iso_from_ms)},
    }))
}

pub fn enable_handler(args: Value) -> Result<Value, String> {
    enable_at(&obs_dir()?, &args)
}

pub fn disable_handler(args: Value) -> Result<Value, String> {
    disable_at(&obs_dir()?, &args)
}

pub fn status_handler(args: Value) -> Result<Value, String> {
    status_at(&obs_dir()?, &args)
}
