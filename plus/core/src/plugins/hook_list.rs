//! `hooks ls`: every hook Claude Code would start, read from the settings files (user, project,
//! local, managed), the hook files of the plugins that are on, and the ledger of hooks synced
//! skills installed. A hook is never run.

use std::collections::BTreeMap;
use std::path::Path;

use regex::Regex;
use serde_json::{json, Map, Value};

use super::claude::ClaudeRunner;
use super::detail;
use super::installed::Installed;
use super::inventory::{shorten, World};
use super::settings::{Layers, Scope};
use crate::context::fsx;
use crate::context::plugin_adapters::{Adapter, Kind, Registry};
use crate::skills::lock;
use crate::untrusted;

const JSON_CAP: u64 = 4 * 1024 * 1024;
const TOOLS: [&str; 4] = ["Bash", "Edit", "Write", "Read"];
const TOOLPORT_MARKERS: [&str; 2] = ["--toolport-hook", "--toolport-guard"];
const HOOKS_FILE: &str = "hooks/hooks.json";

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

/// `NAME=value` words in front of a command are dropped to `NAME=…`: they are where a token
/// would be put.
fn drop_env_values(command: &str) -> String {
    let mut rest = command.trim_start();
    let mut out = String::new();
    loop {
        let name_len = rest.find('=').filter(|at| {
            *at > 0
                && rest[..*at].chars().enumerate().all(|(i, c)| {
                    c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                })
        });
        let Some(at) = name_len else { break };
        let value = &rest[at + 1..];
        let mut quote: Option<char> = None;
        let mut end = value.len();
        for (i, c) in value.char_indices() {
            match (quote, c) {
                (None, '"' | '\'') => quote = Some(c),
                (Some(q), c) if c == q => quote = None,
                (None, c) if c.is_whitespace() => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        out.push_str(&rest[..=at]);
        out.push('…');
        rest = value[end..].trim_start();
        if end < value.len() {
            out.push(' ');
        } else {
            return out;
        }
    }
    out.push_str(rest);
    out
}

pub(super) struct Owner {
    pub kind: &'static str,
    pub name: String,
}

pub(super) struct Source<'a> {
    pub owner: Owner,
    pub path: &'a Path,
    pub switch: (&'static str, String),
    /// The adapter of the plugin that owns the file, and the file's place inside the plugin.
    pub adapter: Option<(&'a Adapter, &'a str)>,
    pub plugin_id: Option<&'a str>,
}

/// Command to skill name, from the hooks the skill sync recorded as installed.
pub(super) fn skill_hooks(data_dir: Option<&Path>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(file) = data_dir.and_then(lock::load_lockfile) else {
        return out;
    };
    for (name, entry) in file.skills.iter().chain(file.rules.iter()) {
        for (_, ids) in &entry.hooks_installed {
            for id in ids {
                out.insert(id.clone(), name.clone());
            }
        }
    }
    out
}

fn runtime_id(source: &Source, command: &str) -> Option<String> {
    let (adapter, rel) = source.adapter?;
    let ids = adapter.hook_ids.as_ref().filter(|ids| ids.file == rel)?;
    Some(ids.regex.captures(command)?.get(1)?.as_str().to_string())
}

fn entries_of(
    world: &World,
    hooks: &Value,
    source: &Source,
    skills: &BTreeMap<String, String>,
) -> Vec<Value> {
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
                let runtime = runtime_id(source, command);
                let (owner_kind, owner_name, switch) = if marker.is_some() {
                    (
                        "toolport",
                        "toolport".to_string(),
                        (
                            "toolport-command",
                            "managed by Toolport; its own command removes it".to_string(),
                        ),
                    )
                } else if let Some(skill) = skills.get(command) {
                    (
                        "skill",
                        skill.clone(),
                        (
                            "skills-sync",
                            format!(
                                "remove the hook from the skill {skill} in the library and sync"
                            ),
                        ),
                    )
                } else if let (Some(id), Some(plugin), Some((adapter, _))) =
                    (&runtime, source.plugin_id, source.adapter)
                {
                    let knob = adapter.knobs.iter().find(|k| k.kind == Kind::HookIds);
                    match knob {
                        Some(knob) => (
                            source.owner.kind,
                            source.owner.name.clone(),
                            (
                                "plugin-setting",
                                format!(
                                    "plugins config {plugin} --set {}={id} --cwd <dir>; the hook still starts a process and exits at once",
                                    knob.key
                                ),
                            ),
                        ),
                        None => (
                            source.owner.kind,
                            source.owner.name.clone(),
                            source.switch.clone(),
                        ),
                    }
                } else {
                    (
                        source.owner.kind,
                        source.owner.name.clone(),
                        source.switch.clone(),
                    )
                };
                let shown = if kind == "command" {
                    drop_env_values(command)
                } else {
                    command.to_string()
                };
                out.push(json!({
                    "owner": {"kind": owner_kind, "name": owner_name},
                    "event": event,
                    "matcher": matcher,
                    "type": kind,
                    "command": shorten(world, &shown),
                    "runtimeId": runtime,
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

/// The hooks a plugin declares: `hooks/hooks.json`, and what its manifest names or holds inline.
pub(super) fn plugin_entries(
    world: &World,
    registry: &Registry,
    skills: &BTreeMap<String, String>,
    plugin: &Installed,
) -> Vec<Value> {
    let Some(root) = &plugin.install_path else {
        return Vec::new();
    };
    let adapter = registry.for_plugin(&plugin.id);
    let mut docs: Vec<(std::path::PathBuf, &str, Value)> = Vec::new();
    if let Some(doc) = untrusted::below(root, HOOKS_FILE)
        .ok()
        .and_then(|path| untrusted::read_json(&path, JSON_CAP))
    {
        docs.push((root.join(HOOKS_FILE), HOOKS_FILE, doc));
    }
    if let Some(manifest) = detail::manifest(root) {
        match manifest.get("hooks") {
            Some(Value::String(rel)) if rel.trim_start_matches("./") != HOOKS_FILE => {
                let rel = rel.trim_start_matches("./").to_string();
                if let Some(doc) = untrusted::below(root, &rel)
                    .ok()
                    .and_then(|path| untrusted::read_json(&path, JSON_CAP))
                {
                    docs.push((root.join(&rel), "", doc));
                }
            }
            Some(inline @ Value::Object(_)) => docs.push((
                root.join(".claude-plugin/plugin.json"),
                "",
                json!({"hooks": inline}),
            )),
            _ => {}
        }
    }
    docs.into_iter()
        .flat_map(|(file, rel, doc)| {
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
                    adapter: adapter.map(|a| (a, rel)),
                    plugin_id: Some(&plugin.id),
                },
                skills,
            )
        })
        .collect()
}

#[derive(Default)]
pub struct HookFilter<'a> {
    pub tool: Option<&'a str>,
    pub event: Option<&'a str>,
    pub owner: Option<&'a str>,
}

fn settings_entries(
    world: &World,
    layers: &Layers,
    skills: &BTreeMap<String, String>,
) -> Vec<Value> {
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
                adapter: None,
                plugin_id: None,
            },
            skills,
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
    let layers = super::inventory::layers(world, cwd);
    let registry = Registry::load(world.data_dir.as_deref());
    let skills = skill_hooks(world.data_dir.as_deref());
    let disabled_all = layers
        .flag("disableAllHooks")
        .is_some_and(|(value, _)| value);
    let managed_only = layers
        .managed("allowManagedHooksOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let listing = super::inventory::list(world, runner);
    let mut entries = settings_entries(world, &layers, &skills);
    for plugin in &listing.plugins {
        if layers.enabled(&plugin.id).effective {
            entries.extend(plugin_entries(world, &registry, &skills, plugin));
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
        "cwd": cwd.map(fsx::display),
        "disabledAll": disabled_all,
        "managedOnly": managed_only,
        "managedSettings": super::inventory::managed_info(world, &layers),
        "hooks": entries,
        "counts": {"byOwner": by_owner, "perTool": per_tool_json, "otherEvents": other},
        "conflicts": conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::drop_env_values;

    #[test]
    fn test01_values_in_front_of_a_command_are_dropped_and_the_command_stays() {
        assert_eq!(
            drop_env_values("GH_TOKEN=abc123 FOO='a b' node run.js --flag=1"),
            "GH_TOKEN=… FOO=… node run.js --flag=1"
        );
        assert_eq!(drop_env_values("node run.js --a=b"), "node run.js --a=b");
        assert_eq!(drop_env_values("A=1"), "A=…");
    }
}
