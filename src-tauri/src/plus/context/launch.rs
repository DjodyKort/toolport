//! Launch profiles: per-profile `--strict-mcp-config` / `--settings` / `--append-system-prompt-file`
//! argv for `claude`, replacing the `CLAUDE_CONFIG_DIR` profile. The login stays in the default
//! config dir, so there is no second `/login`.

use super::compact;
use super::config::ProfileSpec;
use super::doctor::{sha256_file, PROFILE_STATE_FILE};
use super::layers::{body_of, list_layers};
use super::roots::Roots;
use super::Report;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const MCP_FILE: &str = "mcp.json";
pub const SETTINGS_FILE: &str = "settings.json";
pub const APPEND_FILE: &str = "append-system-prompt.md";
pub const APPEND_HEADER: &str =
    "<!-- Managed by `mcpm context` — regenerate with `mcpm context sync`; do not edit. -->";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    All,
    None,
    Named(Vec<String>),
}

pub fn parse_selection(value: &Value, field: &str) -> Result<Selection, String> {
    match value {
        Value::String(s) if s == "inherit" => Ok(Selection::All),
        Value::String(s) if s == "none" => Ok(Selection::None),
        Value::Array(items) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or_else(|| format!("{field}: list entries must be strings"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Selection::Named),
        _ => Err(format!(
            "{field} must be \"inherit\", \"none\" or a list of names"
        )),
    }
}

pub fn profile_dir(roots: &Roots, name: &str) -> PathBuf {
    roots.profiles_root().join(name)
}

fn named_rules(spec: &ProfileSpec) -> Vec<String> {
    match parse_selection(&spec.rules, "rules") {
        Ok(Selection::Named(names)) => names,
        _ => Vec::new(),
    }
}

/// The arguments inserted between `command claude` and the user's own arguments.
pub fn launch_argv(roots: &Roots, name: &str, spec: &ProfileSpec) -> Vec<String> {
    let dir = profile_dir(roots, name);
    let show = |file: &str| dir.join(file).display().to_string();
    let mut argv = vec![
        "--strict-mcp-config".to_string(),
        "--mcp-config".to_string(),
        show(MCP_FILE),
        "--settings".to_string(),
        show(SETTINGS_FILE),
    ];
    if !named_rules(spec).is_empty() || spec.compact_instructions.is_some() {
        argv.push("--append-system-prompt-file".to_string());
        argv.push(show(APPEND_FILE));
    }
    if let Some(value) = compact::flag_value(spec) {
        argv.push("--autocompact".to_string());
        argv.push(value);
    }
    argv
}

fn read_json_object(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

pub fn build_mcp_config(
    roots: &Roots,
    spec: &ProfileSpec,
    report: &mut Report,
    name: &str,
) -> Result<Value, String> {
    let all = match read_json_object(&roots.claude_json).remove("mcpServers") {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let chosen = match parse_selection(&spec.servers, "servers")? {
        Selection::All => all,
        Selection::None => Map::new(),
        Selection::Named(names) => {
            let mut picked = Map::new();
            for server in names {
                match all.get(&server) {
                    Some(entry) => {
                        picked.insert(server, entry.clone());
                    }
                    None => report.warn(format!(
                        "profile {name}: server '{server}' not found in {}",
                        roots.claude_json.display()
                    )),
                }
            }
            picked
        }
    };
    Ok(json!({ "mcpServers": chosen }))
}

pub fn build_settings(roots: &Roots, spec: &ProfileSpec) -> Value {
    let mut settings = read_json_object(&roots.claude_home.join("settings.json"));
    for (key, value) in &spec.settings_overrides {
        settings.insert(key.clone(), value.clone());
    }
    if !spec.org {
        let org = roots.claude_home.join("CLAUDE.md").display().to_string();
        let mut excludes = match settings.remove("claudeMdExcludes") {
            Some(Value::Array(items)) => items,
            _ => Vec::new(),
        };
        let entry = Value::String(org);
        if !excludes.contains(&entry) {
            excludes.push(entry);
        }
        settings.insert("claudeMdExcludes".into(), Value::Array(excludes));
    }
    compact::apply_settings(&mut settings, spec);
    Value::Object(settings)
}

pub fn build_append_prompt(
    roots: &Roots,
    spec: &ProfileSpec,
    report: &mut Report,
    name: &str,
) -> Option<String> {
    let wanted = named_rules(spec);
    let instructions = compact::instructions_block(spec);
    if wanted.is_empty() && instructions.is_none() {
        return None;
    }
    let layers = list_layers(roots);
    let mut out = format!("{APPEND_HEADER}\n");
    for rule in wanted {
        match layers.iter().find(|l| l.name == rule) {
            Some(layer) => {
                out.push('\n');
                out.push_str(body_of(&layer.path).trim());
                out.push('\n');
            }
            None => report.warn(format!("profile {name}: rule layer '{rule}' not found")),
        }
    }
    if let Some(block) = instructions {
        out.push('\n');
        out.push_str(&block);
        out.push('\n');
    }
    Some(out)
}

fn json_text(value: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("json serializes")
    )
}

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Writes `mcp.json`, `settings.json`, the optional append file and the drift state file.
pub fn generate_profile(
    roots: &Roots,
    name: &str,
    spec: &ProfileSpec,
    report: &mut Report,
    dry_run: bool,
) -> Result<(), String> {
    let mcp = build_mcp_config(roots, spec, report, name)?;
    parse_selection(&spec.rules, "rules")?;
    for warning in compact::warnings(roots, name, spec) {
        report.warn(warning);
    }
    let settings = build_settings(roots, spec);
    let append = build_append_prompt(roots, spec, report, name);
    for (field, value) in [
        ("commands", spec.commands),
        ("skills", spec.skills),
        ("agents", spec.agents),
    ] {
        if !value {
            report.warn(format!(
                "profile {name}: {field}=false cannot be enforced without CLAUDE_CONFIG_DIR; ignored"
            ));
        }
    }
    let dir = profile_dir(roots, name);
    let servers = mcp["mcpServers"].as_object().map(Map::len).unwrap_or(0);
    report.add(format!(
        "generated launch profile {name} ({servers} server(s)) in {}",
        dir.display()
    ));
    if dry_run {
        return Ok(());
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_private(&dir.join(MCP_FILE), &json_text(&mcp))?;
    write_private(&dir.join(SETTINGS_FILE), &json_text(&settings))?;
    let append_path = dir.join(APPEND_FILE);
    match append {
        Some(text) => write_private(&append_path, &text)?,
        None => {
            let _ = fs::remove_file(&append_path);
        }
    }
    let base = sha256_file(&roots.claude_home.join("settings.json"));
    write_private(
        &dir.join(PROFILE_STATE_FILE),
        &json_text(&json!({ "settings_base_sha256": base })),
    )
}
