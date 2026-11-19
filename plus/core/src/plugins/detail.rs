//! What `plugins ls|show` read about one plugin beyond the install list: the version its
//! marketplace offers, Claude Code's projected token cost, the options the plugin declares and
//! their current values, its MCP servers with where each is denied, and the adapter knobs.
//!
//! A plugin checkout is somebody else's folder: everything inside it is read through
//! `untrusted`. Option values marked sensitive and the `env`/`headers` of an MCP server are never
//! returned.

use std::cmp::Ordering;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::claude::ClaudeRunner;
use super::installed::{self, Installed};
use super::marketplace;
use super::read_json;
use super::settings::{Layers, Scope};
use crate::context::plugin_adapters::{Adapter, Kind, Knob};
use crate::untrusted;

const JSON_CAP: u64 = 4 * 1024 * 1024;
const CLI_TIMEOUT: Duration = Duration::from_secs(20);
const MANIFEST: &str = ".claude-plugin/plugin.json";

pub fn manifest(root: &Path) -> Option<Value> {
    untrusted::read_json(&untrusted::below(root, MANIFEST).ok()?, JSON_CAP)
}

/// A file inside a plugin checkout, or the object the manifest holds inline under `key`.
fn manifest_part(root: &Path, key: &str) -> Option<Value> {
    match manifest(root)?.get(key)? {
        Value::String(rel) => untrusted::read_json(&untrusted::below(root, rel).ok()?, JSON_CAP),
        inline @ Value::Object(_) => Some(inline.clone()),
        _ => None,
    }
}

/// `~N tok` after `Always-on:` in `claude plugin details`; `None` when the line is absent.
pub fn parse_always_on(text: &str) -> Option<u64> {
    let rest = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Always-on:"))?;
    let figure = rest
        .trim()
        .trim_start_matches('~')
        .split_whitespace()
        .next()?;
    let figure = figure.replace(',', "");
    match figure.strip_suffix(['k', 'K']) {
        Some(thousands) => thousands
            .parse::<f64>()
            .ok()
            .map(|n| (n * 1000.0).round() as u64),
        None => figure.parse().ok(),
    }
}

pub fn projected_tokens(runner: &dyn ClaudeRunner, id: &str) -> Option<u64> {
    let out = runner.run(&["plugin", "details", id], CLI_TIMEOUT).ok()?;
    out.ok().then(|| parse_always_on(&out.stdout)).flatten()
}

fn numeric(version: &str) -> Option<Vec<u64>> {
    version
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .take_while(|part| part.chars().all(|c| c.is_ascii_digit()) && !part.is_empty())
        .map(|part| part.parse().ok())
        .collect()
}

fn compare(offered: &str, installed: &str) -> Option<Ordering> {
    if offered == installed {
        return Some(Ordering::Equal);
    }
    let (a, b) = (numeric(offered)?, numeric(installed)?);
    (!a.is_empty() && !b.is_empty()).then(|| a.cmp(&b))
}

/// The marketplace entry for `name`: its `version`, else the plugin manifest's.
fn offered_version(clone: &Path, name: &str) -> Option<String> {
    if let Some(Ok(market)) = marketplace::read(clone) {
        if let Some(version) = market.plugin(name).and_then(|p| p.version.clone()) {
            return Some(version);
        }
    }
    let file = untrusted::below(clone, marketplace::FILE).ok()?;
    let doc = untrusted::read_json(&file, JSON_CAP)?;
    doc.get("plugins")?
        .as_array()?
        .iter()
        .find(|e| e.get("name").and_then(Value::as_str) == Some(name))?
        .get("version")?
        .as_str()
        .map(String::from)
}

fn marketplace_clone(claude_home: &Path, marketplace: &str) -> std::path::PathBuf {
    read_json(&claude_home.join("plugins/known_marketplaces.json"))
        .and_then(|known| {
            known
                .get(marketplace)?
                .get("installLocation")?
                .as_str()
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(|| claude_home.join("plugins/marketplaces").join(marketplace))
}

/// Whether the marketplace clone on disk offers a newer version than the install. Reads files;
/// what the remote has is only known after `--refresh` updated the clone.
pub fn update(claude_home: &Path, plugin: &Installed) -> Value {
    let unknown = || json!({"state": "unknown", "available": null});
    let Some(installed) = plugin.version.as_deref() else {
        return unknown();
    };
    let offered = plugin
        .folder_version
        .clone()
        .filter(|v| v != installed)
        .or_else(|| {
            let market = plugin.marketplace.as_deref()?;
            offered_version(&marketplace_clone(claude_home, market), &plugin.name)
        });
    let Some(offered) = offered else {
        return unknown();
    };
    match compare(&offered, installed) {
        Some(Ordering::Greater) => json!({"state": "update", "available": offered}),
        Some(_) => json!({"state": "current", "available": null}),
        None => unknown(),
    }
}

/// An MCP server a plugin declares: `.mcp.json` or the manifest's `mcpServers`.
pub fn mcp_servers(root: &Path) -> Vec<(String, Value)> {
    let from_file = untrusted::below(root, ".mcp.json")
        .ok()
        .and_then(|path| untrusted::read_json(&path, JSON_CAP));
    let mut found: Vec<(String, Value)> = Vec::new();
    for doc in [from_file, manifest_part(root, "mcpServers")]
        .into_iter()
        .flatten()
    {
        let servers = doc.get("mcpServers").unwrap_or(&doc);
        for (name, config) in servers.as_object().into_iter().flatten() {
            if config.is_object() && !found.iter().any(|(n, _)| n == name) {
                found.push((name.clone(), config.clone()));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// The `library select` source (`<tap>/<plugin>`) that chooses single items of this plugin, when a
/// tap of the library holds the marketplace the plugin came from.
pub fn select_source(data_dir: Option<&Path>, plugin: &Installed) -> Option<String> {
    let data_dir = data_dir?;
    let wanted = plugin.marketplace.as_deref()?;
    crate::skills::taps::load_taps(data_dir)
        .into_iter()
        .find_map(|tap| {
            let clone =
                crate::skills::taps::tap_dir(&crate::skills::taps::taps_root(data_dir), &tap);
            let market = marketplace::read(&clone)?.ok()?;
            (market.name == wanted && market.plugin(&plugin.name).is_some())
                .then(|| format!("{}/{}", tap.name, plugin.name))
        })
}

pub fn mcp_key(plugin: &str, server: &str) -> String {
    format!("plugin:{plugin}:{server}")
}

fn denied_in(layers: &Layers, scope: Scope, key: &str) -> bool {
    layers
        .files()
        .iter()
        .filter(|f| f.scope == scope)
        .filter_map(|f| f.get("deniedMcpServers")?.as_array())
        .flatten()
        .any(|entry| entry.get("serverName").and_then(Value::as_str) == Some(key))
}

/// Where Claude Code is told not to start a plugin's server.
pub fn denied(layers: &Layers, key: &str) -> Value {
    json!({
        "user": denied_in(layers, Scope::User, key),
        "project": denied_in(layers, Scope::Project, key),
        "local": denied_in(layers, Scope::Local, key),
        "managed": denied_in(layers, Scope::Managed, key),
    })
}

pub fn is_denied(denied: &Value) -> bool {
    ["user", "project", "local", "managed"]
        .iter()
        .any(|scope| denied[*scope] == true)
}

/// One server for `plugins show`: how it starts, without its environment or headers.
pub fn mcp_row(plugin: &Installed, name: &str, config: &Value, layers: &Layers) -> Value {
    let key = mcp_key(&plugin.name, name);
    let command = config.get("command").and_then(Value::as_str).map(|c| {
        let mut line = vec![c.to_string()];
        line.extend(
            config
                .get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(String::from),
        );
        line
    });
    json!({
        "key": key,
        "name": name,
        "type": config.get("type").and_then(Value::as_str),
        "command": command,
        "url": config.get("url").and_then(Value::as_str),
        "toolPrefix": format!("mcp__plugin_{}_{name}__*", plugin.name),
        "denied": denied(layers, &key),
    })
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(list) => list.iter().map(text_of).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// The values the settings files hold for a plugin's options (`pluginConfigs.<id>.options`),
/// the most specific file last.
fn option_values(layers: &Layers, id: &str) -> Map<String, Value> {
    let mut out = Map::new();
    for file in layers.files() {
        let options = file
            .get("pluginConfigs")
            .and_then(|all| all.get(id))
            .and_then(|plugin| plugin.get("options"))
            .and_then(Value::as_object);
        for (key, value) in options.into_iter().flatten() {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

fn option_row(key: &str, spec: &Value, current: Option<&Value>, choices: Option<&Value>) -> Value {
    let sensitive = spec.get("sensitive").and_then(Value::as_bool) == Some(true);
    json!({
        "key": key,
        "title": spec.get("title").and_then(Value::as_str).unwrap_or(key),
        "description": spec.get("description").and_then(Value::as_str).unwrap_or(""),
        "type": spec.get("type").and_then(Value::as_str).unwrap_or("string"),
        "default": spec.get("default"),
        "sensitive": sensitive,
        "current": if sensitive { None } else { current.filter(|v| !v.is_null()).map(text_of) },
        "configured": if sensitive { None } else { current.map(|v| !v.is_null() && v != "") },
        "choices": choices.filter(|c| c.is_array()),
    })
}

/// The options a plugin declares. `claude plugin configure --json` first (it also knows the
/// choices); the manifest's `userConfig` and the settings files otherwise.
pub fn options(
    runner: Option<&dyn ClaudeRunner>,
    plugin: &Installed,
    layers: &Layers,
) -> (Vec<Value>, &'static str) {
    let from_cli = runner.and_then(|runner| {
        let out = runner
            .run(&["plugin", "configure", &plugin.id, "--json"], CLI_TIMEOUT)
            .ok()?;
        if !out.ok() {
            return None;
        }
        serde_json::from_str::<Value>(&out.stdout).ok()
    });
    if let Some(doc) = from_cli {
        let inputs = doc.get("inputs").and_then(Value::as_object);
        let choices = doc.get("choices").and_then(Value::as_object);
        let configured: Vec<&str> = doc
            .get("configured")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let rows = doc
            .get("schema")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(key, spec)| {
                let mut row = option_row(
                    key,
                    spec,
                    inputs.and_then(|i| i.get(key)).filter(|v| *v != ""),
                    choices.and_then(|c| c.get(key)),
                );
                row["configured"] = json!(configured.contains(&key.as_str()));
                row
            })
            .collect();
        return (rows, "claude-cli");
    }
    let Some(root) = plugin.install_path.as_deref() else {
        return (Vec::new(), "files");
    };
    let values = option_values(layers, &plugin.id);
    let rows = manifest(root)
        .and_then(|m| m.get("userConfig").cloned())
        .and_then(|c| c.as_object().cloned())
        .into_iter()
        .flatten()
        .map(|(key, spec)| option_row(&key, &spec, values.get(&key), spec.get("choices")))
        .collect();
    (rows, "files")
}

fn env_rank(scope: Scope) -> u8 {
    match scope {
        Scope::User => 0,
        Scope::Project => 1,
        Scope::Local => 2,
        Scope::Managed => 3,
    }
}

/// What a knob is set to now and where that comes from: an `env` entry (managed over local over
/// project over user settings), then the plugin option the knob names, then the adapter default.
fn knob_current(knob: &Knob, layers: &Layers, options: &Map<String, Value>) -> Value {
    let from_env = layers
        .files()
        .iter()
        .filter_map(|f| Some((f.scope, f.get("env")?.get(&knob.env)?.as_str()?)))
        .max_by_key(|(scope, _)| env_rank(*scope));
    if let Some((scope, value)) = from_env {
        let from = match scope {
            Scope::User => "user-env",
            Scope::Managed => "managed-env",
            Scope::Project | Scope::Local => "folder-env",
        };
        return json!({"value": value, "from": from});
    }
    if let Some(value) = knob.option.as_ref().and_then(|o| options.get(o)) {
        return json!({"value": text_of(value), "from": "option"});
    }
    json!({"value": knob.default, "from": "default"})
}

pub fn knobs(adapter: Option<&Adapter>, layers: &Layers, id: &str) -> Vec<Value> {
    let Some(adapter) = adapter else {
        return Vec::new();
    };
    let options = option_values(layers, id);
    adapter
        .knobs
        .iter()
        .map(|knob| {
            json!({
                "key": knob.key,
                "label": knob.label,
                "kind": knob.kind.as_str(),
                "env": knob.env,
                "option": knob.option,
                "choices": knob.choices,
                "default": knob.default,
                "folderOnly": knob.option.is_none(),
                "list": matches!(knob.kind, Kind::Csv | Kind::Globs | Kind::HookIds),
                "current": knob_current(knob, layers, &options),
            })
        })
        .collect()
}

pub fn marketplaces(claude_home: &Path, installed: &[Installed]) -> Vec<Value> {
    let known = read_json(&claude_home.join("plugins/known_marketplaces.json"));
    let mut names: Vec<String> = known
        .as_ref()
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    for plugin in installed {
        if let Some(name) = &plugin.marketplace {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let count = installed
                .iter()
                .filter(|p| p.marketplace.as_deref() == Some(name.as_str()))
                .count();
            json!({
                "name": name,
                "source": installed::marketplace_source(claude_home, &name),
                "followsFolder": known
                    .as_ref()
                    .and_then(|k| k.get(&name)?.get("source")?.get("source")?.as_str())
                    .is_some_and(|kind| kind == "directory"),
                "installed": count,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test01_always_on_reads_plain_comma_and_k_figures() {
        let text =
            |figure: &str| format!("x\nProjected token cost\n  Always-on:   {figure} added\n");
        assert_eq!(parse_always_on(&text("~640 tok")), Some(640));
        assert_eq!(parse_always_on(&text("~40,639 tok")), Some(40639));
        assert_eq!(parse_always_on(&text("~1.5k tok")), Some(1500));
        assert_eq!(parse_always_on("nothing"), None);
    }

    #[test]
    fn test02_versions_compare_by_number_not_by_text() {
        assert_eq!(compare("1.10.0", "1.9.0"), Some(Ordering::Greater));
        assert_eq!(compare("1.8.2", "1.8.2"), Some(Ordering::Equal));
        assert_eq!(compare("0.9.0", "1.0.0"), Some(Ordering::Less));
        assert_eq!(compare("abc123", "def456"), None);
    }
}
