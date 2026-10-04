//! Launch profiles: per-profile `--strict-mcp-config` / `--settings` / `--append-system-prompt-file`
//! argv for `claude`, replacing the `CLAUDE_CONFIG_DIR` profile. The login stays in the default
//! config dir, so there is no second `/login`.

use super::compact;
use super::config::ProfileSpec;
use super::doctor::{sha256_file, PROFILE_STATE_FILE};
use super::layers::{body_of, list_layers, Layer};
use super::roots::Roots;
use super::Report;
use serde_json::{json, Map, Value};
use std::cell::OnceCell;
use std::fs;
use std::path::{Path, PathBuf};

pub const MCP_FILE: &str = "mcp.json";
pub const SETTINGS_FILE: &str = "settings.json";
pub const APPEND_FILE: &str = "append-system-prompt.md";
pub const APPEND_HEADER: &str =
    "<!-- Managed by `toolportctl context` — regenerate with `toolportctl context sync`; do not edit. -->";

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

pub(super) fn read_json_object(path: &Path) -> Map<String, Value> {
    crate::plus::jsonfs::read_json(path).unwrap_or_default()
}

/// What every profile of one run reads from disk, each read once on first use.
pub struct Sources<'a> {
    roots: &'a Roots,
    servers: OnceCell<Map<String, Value>>,
    settings: OnceCell<Map<String, Value>>,
    settings_sha: OnceCell<Option<String>>,
    layers: OnceCell<Vec<Layer>>,
}

impl<'a> Sources<'a> {
    pub fn new(roots: &'a Roots) -> Self {
        Sources {
            roots,
            servers: OnceCell::new(),
            settings: OnceCell::new(),
            settings_sha: OnceCell::new(),
            layers: OnceCell::new(),
        }
    }

    fn servers(&self) -> &Map<String, Value> {
        self.servers.get_or_init(|| {
            match read_json_object(&self.roots.claude_json).remove("mcpServers") {
                Some(Value::Object(m)) => m,
                _ => Map::new(),
            }
        })
    }

    fn settings(&self) -> &Map<String, Value> {
        self.settings
            .get_or_init(|| read_json_object(&self.roots.claude_home.join("settings.json")))
    }

    fn settings_sha(&self) -> &Option<String> {
        self.settings_sha
            .get_or_init(|| sha256_file(&self.roots.claude_home.join("settings.json")))
    }

    fn layers(&self) -> &[Layer] {
        self.layers.get_or_init(|| list_layers(self.roots))
    }
}

pub fn build_mcp_config(
    src: &Sources,
    spec: &ProfileSpec,
    report: &mut Report,
    name: &str,
) -> Result<Value, String> {
    let all = src.servers();
    let chosen = match parse_selection(&spec.servers, "servers")? {
        Selection::All => all.clone(),
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
                        src.roots.claude_json.display()
                    )),
                }
            }
            picked
        }
    };
    Ok(json!({ "mcpServers": chosen }))
}

pub fn build_settings(src: &Sources, spec: &ProfileSpec) -> Value {
    let roots = src.roots;
    let mut settings = src.settings().clone();
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
    src: &Sources,
    spec: &ProfileSpec,
    report: &mut Report,
    name: &str,
) -> Option<String> {
    let wanted = named_rules(spec);
    let instructions = compact::instructions_block(spec);
    if wanted.is_empty() && instructions.is_none() {
        return None;
    }
    let layers = src.layers();
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
    crate::registry::atomic_write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
pub fn generate_profile(
    roots: &Roots,
    name: &str,
    spec: &ProfileSpec,
    report: &mut Report,
    dry_run: bool,
) -> Result<(), String> {
    generate_profile_with(&Sources::new(roots), name, spec, report, dry_run)
}

/// Writes `mcp.json`, `settings.json`, the optional append file and the drift state file.
pub fn generate_profile_with(
    src: &Sources,
    name: &str,
    spec: &ProfileSpec,
    report: &mut Report,
    dry_run: bool,
) -> Result<(), String> {
    let roots = src.roots;
    let mcp = build_mcp_config(src, spec, report, name)?;
    parse_selection(&spec.rules, "rules")?;
    for warning in compact::warnings(roots, name, spec) {
        report.warn(warning);
    }
    let settings = build_settings(src, spec);
    let append = build_append_prompt(src, spec, report, name);
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
    let verb = if dry_run { "would generate" } else { "generated" };
    report.add(format!(
        "{verb} launch profile {name} ({servers} server(s)) in {}",
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
    write_private(
        &dir.join(PROFILE_STATE_FILE),
        &json_text(&json!({ "settings_base_sha256": src.settings_sha() })),
    )
}
