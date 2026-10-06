//! Auto-compact management for launch profiles: window, per-model windows, enable switch,
//! compact instructions and the optional checkpoint (PreCompact hook plus a token offset).

use super::config::ProfileSpec;
use super::roots::Roots;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const MIN_WINDOW: u64 = 100_000;
pub const MAX_WINDOW: u64 = 1_000_000;
pub const ENV_WINDOW: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
pub const INSTRUCTIONS_HEADING: &str = "# Compact instructions";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_at: Option<u64>,
}

/// Where Claude Code reads what an administrator manages: settings and the policy CLAUDE.md.
pub fn managed_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ClaudeCode")
    } else if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\ClaudeCode")
    } else {
        PathBuf::from("/etc/claude-code")
    }
}

pub fn managed_settings_path() -> PathBuf {
    managed_dir().join("managed-settings.json")
}

pub fn apply_env(roots: &mut Roots) {
    roots.env_auto_compact_window = std::env::var(ENV_WINDOW).ok();
    roots.managed_settings = Some(managed_settings_path());
}

pub fn parse_window(value: &Value, field: &str) -> Result<Value, String> {
    match value {
        Value::String(s) if s == "auto" => Ok(value.clone()),
        Value::Number(n) => match n.as_u64() {
            Some(t) if (MIN_WINDOW..=MAX_WINDOW).contains(&t) => Ok(value.clone()),
            _ => Err(format!(
                "{field} must be \"auto\" or a token count between {MIN_WINDOW} and {MAX_WINDOW}"
            )),
        },
        _ => Err(format!(
            "{field} must be \"auto\" or a token count between {MIN_WINDOW} and {MAX_WINDOW}"
        )),
    }
}

pub fn validate(name: &str, spec: &ProfileSpec) -> Result<(), String> {
    if let Some(v) = &spec.auto_compact_window {
        parse_window(v, &format!("profile {name}: auto_compact_window"))?;
    }
    for (model, v) in &spec.model_windows {
        if model.trim().is_empty() {
            return Err(format!(
                "profile {name}: model_windows keys must be non-empty model ids"
            ));
        }
        parse_window(v, &format!("profile {name}: model_windows.{model}"))?;
    }
    if let Some(text) = &spec.compact_instructions {
        if text.trim().is_empty() {
            return Err(format!(
                "profile {name}: compact_instructions must not be empty"
            ));
        }
    }
    if let Some(cp) = &spec.checkpoint {
        if cp.command.as_deref().is_some_and(|c| c.trim().is_empty()) {
            return Err(format!(
                "profile {name}: checkpoint.command must not be empty"
            ));
        }
        if cp.checkpoint_at == Some(0) {
            return Err(format!(
                "profile {name}: checkpoint.checkpoint_at must be positive"
            ));
        }
    }
    Ok(())
}

pub fn configures(spec: &ProfileSpec) -> bool {
    spec.auto_compact_window.is_some()
        || !spec.model_windows.is_empty()
        || spec.auto_compact_enabled.is_some()
        || spec.compact_instructions.is_some()
        || spec.checkpoint.is_some()
}

pub fn flag_value(spec: &ProfileSpec) -> Option<String> {
    if !spec.autocompact_flag {
        return None;
    }
    match spec.auto_compact_window.as_ref()? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn apply_settings(settings: &mut Map<String, Value>, spec: &ProfileSpec) {
    if let Some(v) = &spec.auto_compact_window {
        settings.insert("autoCompactWindow".into(), v.clone());
    }
    if let Some(on) = spec.auto_compact_enabled {
        settings.insert("autoCompactEnabled".into(), Value::Bool(on));
    }
    if !spec.model_windows.is_empty() {
        let mut models = match settings.remove("modelSettings") {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        for (id, v) in &spec.model_windows {
            let mut entry = match models.remove(id) {
                Some(Value::Object(m)) => m,
                _ => Map::new(),
            };
            entry.insert("autoCompactWindow".into(), v.clone());
            models.insert(id.clone(), Value::Object(entry));
        }
        settings.insert("modelSettings".into(), Value::Object(models));
    }
    if let Some(command) = spec.checkpoint.as_ref().and_then(|c| c.command.as_ref()) {
        let mut hooks = match settings.remove("hooks") {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        let mut pre = match hooks.remove("PreCompact") {
            Some(Value::Array(a)) => a,
            _ => Vec::new(),
        };
        pre.push(json!({ "hooks": [{ "type": "command", "command": command }] }));
        hooks.insert("PreCompact".into(), Value::Array(pre));
        settings.insert("hooks".into(), Value::Object(hooks));
    }
}

pub fn instructions_block(spec: &ProfileSpec) -> Option<String> {
    spec.compact_instructions
        .as_ref()
        .map(|t| format!("{INSTRUCTIONS_HEADING}\n\n{}", t.trim()))
}

pub fn env_window(roots: &Roots) -> Option<u64> {
    roots
        .env_auto_compact_window
        .as_ref()
        .and_then(|v| v.trim().parse().ok())
}

pub fn warnings(roots: &Roots, name: &str, spec: &ProfileSpec) -> Vec<String> {
    let mut out = Vec::new();
    if !configures(spec) {
        return out;
    }
    if let Some(v) = &roots.env_auto_compact_window {
        out.push(format!(
            "profile {name}: {ENV_WINDOW}={v} is set in the environment and overrides the profile's compact window"
        ));
    }
    if spec.auto_compact_window.is_some() || !spec.model_windows.is_empty() {
        if let Some(path) = &roots.managed_settings {
            let managed = crate::plus::jsonfs::read_json::<Value>(path);
            let sets = managed
                .as_ref()
                .is_some_and(|m| m.get("autoCompactWindow").is_some());
            if sets {
                out.push(format!(
                    "profile {name}: managed settings at {} set autoCompactWindow and preempt the profile",
                    path.display()
                ));
            }
        }
    }
    out
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CompactInfo {
    pub window: Value,
    pub window_source: &'static str,
    pub model_windows: BTreeMap<String, Value>,
    pub enabled: bool,
    pub checkpoint_at: Option<u64>,
    pub checkpoint_point: Option<u64>,
    pub checkpoint_command: Option<String>,
    pub instructions_tokens: u64,
}

pub fn effective_window(
    roots: &Roots,
    spec: &ProfileSpec,
    model: Option<&str>,
) -> (Value, &'static str) {
    if let Some(t) = env_window(roots) {
        return (json!(t), "env");
    }
    if let Some(v) = model.and_then(|m| spec.model_windows.get(m)) {
        return (v.clone(), "model");
    }
    if let Some(v) = &spec.auto_compact_window {
        return (v.clone(), "profile");
    }
    (json!("auto"), "default")
}

pub fn checkpoint_point(window: &Value, spec: &ProfileSpec) -> Option<u64> {
    let at = spec.checkpoint.as_ref()?.checkpoint_at?;
    window.as_u64().map(|w| w.saturating_sub(at))
}

pub fn info(roots: &Roots, spec: &ProfileSpec) -> Option<CompactInfo> {
    if !configures(spec) {
        return None;
    }
    let (window, window_source) = effective_window(roots, spec, None);
    Some(CompactInfo {
        checkpoint_point: checkpoint_point(&window, spec),
        window,
        window_source,
        model_windows: spec.model_windows.clone(),
        enabled: spec.auto_compact_enabled.unwrap_or(true),
        checkpoint_at: spec.checkpoint.as_ref().and_then(|c| c.checkpoint_at),
        checkpoint_command: spec.checkpoint.as_ref().and_then(|c| c.command.clone()),
        instructions_tokens: instructions_block(spec)
            .map(|b| crate::savings::estimated_tokens(b.len() as u64))
            .unwrap_or(0),
    })
}

fn used_tokens(window: &Value) -> Option<u64> {
    if let Some(cur) = window.get("current_usage").filter(|c| c.is_object()) {
        let n = |k: &str| cur.get(k).and_then(Value::as_u64).unwrap_or(0);
        return Some(
            n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens"),
        );
    }
    let size = window.get("context_window_size").and_then(Value::as_u64)?;
    let pct = window.get("used_percentage").and_then(Value::as_f64)?;
    Some((size as f64 * pct / 100.0).round() as u64)
}

pub fn checkpoint_status(
    roots: &Roots,
    statusline: &Value,
    spec: Option<&ProfileSpec>,
    window_override: Option<u64>,
    at_override: Option<u64>,
) -> Result<Value, String> {
    let cw = statusline
        .get("context_window")
        .ok_or_else(|| "statusline JSON has no context_window".to_string())?;
    let used = used_tokens(cw).ok_or_else(|| "context_window has no usage data".to_string())?;
    let model_id = statusline
        .get("model")
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str);
    let model_size = cw.get("context_window_size").and_then(Value::as_u64);
    let (profile_window, source) = match spec {
        Some(s) => effective_window(roots, s, model_id),
        None => match env_window(roots) {
            Some(t) => (json!(t), "env"),
            None => (json!("auto"), "default"),
        },
    };
    let (window, source) = match window_override {
        Some(w) => (Some(w), "flag"),
        None => match profile_window.as_u64() {
            Some(w) => (Some(model_size.map_or(w, |m| w.min(m))), source),
            None => (model_size, "model"),
        },
    };
    let at = at_override.or_else(|| spec.and_then(|s| s.checkpoint.as_ref()?.checkpoint_at));
    let point = match (window, at) {
        (Some(w), Some(a)) => Some(w.saturating_sub(a)),
        _ => None,
    };
    Ok(json!({
        "used_tokens": used,
        "window": window,
        "window_source": source,
        "checkpoint_at": at,
        "checkpoint_point": point,
        "remaining_to_checkpoint": point.map(|p| p.saturating_sub(used)),
        "at_checkpoint": point.map(|p| used >= p),
        "remaining_to_compact": window.map(|w| w.saturating_sub(used)),
    }))
}
