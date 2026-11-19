//! `plugins ls|show` and `hooks ls`: what is installed and which hooks Claude Code would run.
//! Reads files only (plus `claude plugin list --json` through the runner when it is given) and
//! never starts a hook. Fields that need `claude plugin details` are left out and `from` says
//! which source answered.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Map, Value};

use super::claude::ClaudeRunner;
use super::installed::{self, Installed};
use super::read_json;
use super::settings::{Layers, Scope};
use crate::context::fsx;
use crate::op::OpError;

const TOOLS: [&str; 4] = ["Bash", "Edit", "Write", "Read"];
const TOOLPORT_MARKERS: [&str; 2] = ["--toolport-hook", "--toolport-guard"];

pub struct World<'a> {
    pub home: &'a Path,
    pub claude_home: &'a Path,
}

fn shorten(world: &World, text: &str) -> String {
    let home = world.home.to_string_lossy();
    if home.is_empty() {
        return text.to_string();
    }
    text.replace(home.as_ref(), "~")
}

fn names_in(dir: &Path, want_dirs: bool) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if want_dirs {
                path.join("SKILL.md").is_file().then_some(name)
            } else {
                name.strip_suffix(".md").map(String::from)
            }
        })
        .collect();
    names.sort();
    names
}

fn keys_of(file: &Path, key: &str) -> Vec<String> {
    let Some(doc) = read_json(file) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = doc
        .get(key)
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn exact_list(matcher: &str) -> bool {
    matcher
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' ' | ',' | '|'))
}

fn tools_of(event: &str, matcher: &str) -> Vec<&'static str> {
    if !matches!(event, "PreToolUse" | "PostToolUse") {
        return Vec::new();
    }
    if matcher.is_empty() || matcher == "*" {
        return TOOLS.to_vec();
    }
    if exact_list(matcher) {
        let names: Vec<&str> = matcher
            .split(['|', ','])
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .collect();
        return TOOLS
            .into_iter()
            .filter(|tool| names.contains(tool))
            .collect();
    }
    match Regex::new(matcher) {
        Ok(regex) => TOOLS
            .into_iter()
            .filter(|tool| regex.is_match(tool))
            .collect(),
        Err(_) => Vec::new(),
    }
}

struct Owner {
    kind: &'static str,
    name: String,
}

struct Source<'a> {
    owner: Owner,
    path: &'a Path,
    switch: (&'static str, String),
}

fn entries_of(world: &World, hooks: &Value, source: &Source) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(events) = hooks.as_object() else {
        return out;
    };
    for (event, groups) in events {
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group.get("matcher").and_then(Value::as_str).unwrap_or("");
            for hook in group
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let kind = hook
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("command");
                let command = hook
                    .get("command")
                    .or_else(|| hook.get("url"))
                    .or_else(|| hook.get("prompt"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let marker = TOOLPORT_MARKERS.iter().find(|m| command.contains(*m));
                let (owner_kind, owner_name, switch) = match marker {
                    Some(_) => (
                        "toolport",
                        "toolport".to_string(),
                        (
                            "toolport-command",
                            "managed by Toolport; its own command removes it".to_string(),
                        ),
                    ),
                    None => (
                        source.owner.kind,
                        source.owner.name.clone(),
                        source.switch.clone(),
                    ),
                };
                out.push(json!({
                    "owner": {"kind": owner_kind, "name": owner_name},
                    "event": event,
                    "matcher": matcher,
                    "type": kind,
                    "command": shorten(world, command),
                    "runtimeId": null,
                    "timeoutSec": hook.get("timeout").and_then(Value::as_u64),
                    "async": hook.get("async").and_then(Value::as_bool).unwrap_or(false),
                    "source": shorten(world, &fsx::display(source.path)),
                    "marker": marker,
                    "switch": {"method": switch.0, "detail": switch.1},
                    "tools": tools_of(event, matcher),
                }));
            }
        }
    }
    out
}

fn plugin_hook_entries(world: &World, plugin: &Installed) -> Vec<Value> {
    let Some(root) = &plugin.install_path else {
        return Vec::new();
    };
    let file = root.join("hooks/hooks.json");
    let Some(doc) = read_json(&file) else {
        return Vec::new();
    };
    let hooks = doc.get("hooks").cloned().unwrap_or(Value::Null);
    entries_of(
        world,
        &hooks,
        &Source {
            owner: Owner {
                kind: "plugin",
                name: plugin.id.clone(),
            },
            path: &file,
            switch: (
                "disable-plugin",
                format!("turn the plugin off: plugins off {} --cwd <dir>", plugin.id),
            ),
        },
    )
}

fn list(world: &World, runner: Option<&dyn ClaudeRunner>) -> installed::Listing {
    installed::list(runner.filter(|r| r.present()), world.claude_home)
}

fn layers(world: &World, cwd: Option<&Path>) -> Layers {
    Layers::load(world.claude_home, cwd, None, Some(world.home))
}

fn row(world: &World, plugin: &Installed, layers: &Layers, via: installed::Via) -> Value {
    let enabled = layers.enabled(&plugin.id);
    let root = plugin.install_path.clone().unwrap_or_default();
    let hooks = plugin_hook_entries(world, plugin).len();
    json!({
        "id": plugin.id,
        "name": plugin.name,
        "marketplace": plugin.marketplace,
        "version": plugin.version,
        "installPath": plugin.install_path.as_deref().map(|p| shorten(world, &fsx::display(p))),
        "source": plugin.marketplace.as_deref()
            .and_then(|m| installed::marketplace_source(world.claude_home, m)),
        "enabled": {
            "user": enabled.user,
            "project": enabled.project,
            "local": enabled.local,
            "managed": enabled.managed,
            "effective": enabled.effective,
        },
        "brings": {
            "skills": names_in(&root.join("skills"), true).len(),
            "agents": names_in(&root.join("agents"), false).len(),
            "commands": names_in(&root.join("commands"), false).len(),
            "hooks": hooks,
            "mcpServers": keys_of(&root.join(".mcp.json"), "mcpServers").len(),
            "lspServers": keys_of(&root.join(".lsp.json"), "lspServers").len(),
        },
        "errors": plugin.errors,
        "from": via.as_str(),
    })
}

pub fn ls(world: &World, runner: Option<&dyn ClaudeRunner>, cwd: Option<&Path>) -> Value {
    let listing = list(world, runner);
    let layers = layers(world, cwd);
    json!({
        "cwd": cwd.map(|c| fsx::display(c)),
        "plugins": listing.plugins.iter()
            .map(|p| row(world, p, &layers, listing.via))
            .collect::<Vec<_>>(),
        "refreshError": listing.cli_error,
        "partial": true,
    })
}

pub fn show(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    id: &str,
    cwd: Option<&Path>,
) -> Result<Value, OpError> {
    let listing = list(world, runner);
    let plugin = listing
        .plugins
        .iter()
        .find(|p| p.id == id || p.name == id)
        .ok_or_else(|| OpError::not_found(format!("plugin not found: {id}")))?;
    let layers = layers(world, cwd);
    let mut data = row(world, plugin, &layers, listing.via);
    let root: PathBuf = plugin.install_path.clone().unwrap_or_default();
    let manifest = read_json(&root.join(".claude-plugin/plugin.json")).unwrap_or(Value::Null);
    let mcp: Vec<Value> = keys_of(&root.join(".mcp.json"), "mcpServers")
        .into_iter()
        .map(|name| {
            json!({
                "key": format!("plugin:{}:{name}", plugin.name),
                "name": name,
                "toolPrefix": format!("mcp__plugin_{}_{name}__*", plugin.name),
            })
        })
        .collect();
    let extra = json!({
        "description": manifest.get("description").and_then(Value::as_str),
        "components": {
            "skills": names_in(&root.join("skills"), true),
            "agents": names_in(&root.join("agents"), false),
            "commands": names_in(&root.join("commands"), false),
            "lspServers": keys_of(&root.join(".lsp.json"), "lspServers"),
        },
        "hooks": plugin_hook_entries(world, plugin),
        "mcpServers": mcp,
    });
    if let (Some(target), Some(source)) = (data.as_object_mut(), extra.as_object()) {
        target.extend(source.clone());
    }
    Ok(data)
}

#[derive(Default)]
pub struct HookFilter<'a> {
    pub tool: Option<&'a str>,
    pub event: Option<&'a str>,
    pub owner: Option<&'a str>,
}

fn settings_sources(world: &World, layers: &Layers) -> Vec<Value> {
    let mut all = Vec::new();
    for file in layers.files() {
        let Some(hooks) = file.get("hooks") else {
            continue;
        };
        let kind = match file.scope {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::Local => "local",
            Scope::Managed => "managed",
        };
        all.extend(entries_of(
            world,
            hooks,
            &Source {
                owner: Owner {
                    kind,
                    name: file.name.to_string(),
                },
                path: &file.path,
                switch: (
                    "edit-settings",
                    format!(
                        "edit the hooks key of {}",
                        shorten(world, &fsx::display(&file.path))
                    ),
                ),
            },
        ));
    }
    all
}

pub fn hooks(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    cwd: Option<&Path>,
    filter: &HookFilter,
) -> Value {
    let layers = layers(world, cwd);
    let disabled_all = layers
        .flag("disableAllHooks")
        .is_some_and(|(value, _)| value);
    let listing = list(world, runner);
    let mut entries = settings_sources(world, &layers);
    for plugin in &listing.plugins {
        if layers.enabled(&plugin.id).effective {
            entries.extend(plugin_hook_entries(world, plugin));
        }
    }
    entries.retain(|entry| {
        filter.event.is_none_or(|e| entry["event"] == e)
            && filter.owner.is_none_or(|o| entry["owner"]["kind"] == o)
            && filter.tool.is_none_or(|t| {
                entry["tools"]
                    .as_array()
                    .is_some_and(|l| l.iter().any(|x| x == t))
            })
    });
    let mut by_owner: BTreeMap<String, u64> = BTreeMap::new();
    let mut per_tool: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    let mut other: BTreeMap<String, u64> = BTreeMap::new();
    let mut pre_owners: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for entry in &entries {
        let kind = entry["owner"]["kind"].as_str().unwrap_or("").to_string();
        *by_owner.entry(kind).or_default() += 1;
        let event = entry["event"].as_str().unwrap_or("");
        let tools: Vec<&str> = entry["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if matches!(event, "PreToolUse" | "PostToolUse") {
            for tool in TOOLS.iter().filter(|t| tools.contains(t)) {
                let slot = per_tool.entry(tool).or_default();
                if event == "PreToolUse" {
                    slot.0 += 1;
                    let owner = entry["owner"]["name"].as_str().unwrap_or("").to_string();
                    pre_owners.entry(tool).or_default().push(owner);
                } else {
                    slot.1 += 1;
                }
            }
        } else {
            *other.entry(event.to_string()).or_default() += 1;
        }
    }
    let conflicts: Vec<Value> = pre_owners
        .into_iter()
        .filter_map(|(tool, mut owners)| {
            owners.sort();
            owners.dedup();
            (owners.len() > 1).then(|| {
                json!({
                    "event": "PreToolUse",
                    "tools": [tool],
                    "hooks": owners,
                    "note": "the strictest answer wins: deny, then defer, then ask, then allow",
                })
            })
        })
        .collect();
    let per_tool_json: Map<String, Value> = per_tool
        .into_iter()
        .map(|(tool, (pre, post))| {
            (
                tool.to_string(),
                json!({"pre": pre, "post": post, "total": pre + post}),
            )
        })
        .collect();
    json!({
        "cwd": cwd.map(|c| fsx::display(c)),
        "disabledAll": disabled_all,
        "hooks": entries,
        "counts": {"byOwner": by_owner, "perTool": per_tool_json, "otherEvents": other},
        "conflicts": conflicts,
    })
}
