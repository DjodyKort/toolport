//! `plugins config` and `plugins mcp deny|allow` (D-075): the controls a plugin really has. A
//! folder gets `env` keys (the adapter's knobs) or a `deniedMcpServers` entry in its
//! `settings.local.json`, written through the bundle ledger; without a folder, only knobs that are
//! plugin options are set, through `claude plugin configure --values-stdin`.

use super::adapters::{Adapter, Kind, Knob, Registry};
use super::claude::{ClaudeRunner, NOT_FOUND};
use super::installed::{self, valid_ident, Installed};
use super::report::{self, Failure};
use super::settings::Scope;
use super::{cli, manifest, Env};
use crate::plus::context::bundle_apply::Desired;
use crate::plus::context::bundle_controls::{self as controls, Run};
use crate::plus::context::bundle_api::op as bundle_op;
use crate::plus::op::OpError;
use crate::plus::plan::{Effects, PlanV1, ResultV1};
use crate::plus::sources::fsx;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SWITCHED_OFF: &str = "A switched-off hook still starts a process and exits at once; only turning the plugin off removes it";
const MAX_VALUE: usize = 4096;
const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvChange {
    Set(String, String),
    Remove(String),
}

fn single_line(value: &str) -> Result<(), String> {
    if value.len() > MAX_VALUE || value.chars().any(|c| c.is_control()) {
        return Err("a value is one line of at most 4096 characters without control characters".into());
    }
    Ok(())
}

fn list_of(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn truth(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// The environment change a knob value stands for, or why the value is refused.
pub fn change_of(knob: &Knob, value: &str) -> Result<EnvChange, String> {
    single_line(value)?;
    let env = knob.env.clone();
    match knob.kind {
        Kind::Enum => {
            let choices = knob.choices.as_deref().unwrap_or_default();
            if choices.iter().any(|c| c == value) {
                Ok(EnvChange::Set(env, value.to_string()))
            } else {
                Err(format!("{} must be one of {}", knob.key, choices.join(", ")))
            }
        }
        Kind::Bool => truth(value)
            .map(|on| EnvChange::Set(env, on.to_string()))
            .ok_or_else(|| format!("{} is true or false", knob.key)),
        Kind::BoolOff => match truth(value) {
            Some(true) => Ok(EnvChange::Remove(env)),
            Some(false) => Ok(EnvChange::Set(env, "off".into())),
            None => Err(format!("{} is on or off", knob.key)),
        },
        Kind::Csv | Kind::Globs | Kind::HookIds => {
            let items = list_of(value);
            if items.is_empty() {
                return Err(format!("{} needs at least one value (use --unset to remove it)", knob.key));
            }
            Ok(EnvChange::Set(env, items.join(",")))
        }
    }
}

pub fn check_value(knob: &Knob, value: &str) -> Result<(), String> {
    change_of(knob, value).map(|_| ())
}

/// The `env` pairs of a bundle's `plugins.config`; what cannot be written becomes a warning.
pub fn bundle_env(registry: &Registry, config: &[(String, Vec<(String, String)>)]) -> (Vec<(String, String)>, Vec<String>) {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut warnings = Vec::new();
    for (plugin, knobs) in config {
        let Some(adapter) = registry.for_plugin(plugin) else {
            warnings.push(format!("plugins.config: no adapter for {plugin}; not applied"));
            continue;
        };
        for (key, value) in knobs {
            match adapter.knob(key).map(|k| change_of(k, value)) {
                None => warnings.push(format!("plugins.config.{plugin}.{key}: unknown knob; not applied")),
                Some(Err(e)) => warnings.push(format!("plugins.config.{plugin}.{key}: {e}; not applied")),
                Some(Ok(EnvChange::Set(name, v))) => {
                    env.insert(name, v);
                }
                Some(Ok(EnvChange::Remove(_))) => {}
            }
        }
    }
    (env.into_iter().collect(), warnings)
}

pub(super) fn quote(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "_./-:@%+=,~*".contains(c)) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

pub struct ConfigArgs<'a> {
    pub id: &'a str,
    pub cwd: Option<&'a Path>,
    pub sets: Vec<(String, String)>,
    pub unsets: Vec<String>,
    pub dry_run: bool,
}

pub(super) fn host_failure(f: Failure) -> OpError {
    match f {
        Failure::Missing(m) => OpError::not_found(m),
        Failure::Failed(m) => OpError::conflict(m),
    }
}

fn installed_plugin(env: &Env, runner: Option<&dyn ClaudeRunner>, id: &str) -> Result<Installed, OpError> {
    if !valid_ident(id) {
        return Err(OpError::usage(format!("invalid plugin id: {id}")));
    }
    let listing = installed::list(runner, &env.claude_home);
    report::find(&listing.plugins, id).cloned().map_err(host_failure)
}

fn adapter_of<'a>(registry: &'a Registry, plugin: &Installed) -> Result<&'a Adapter, OpError> {
    registry.for_plugin(&plugin.id).ok_or_else(|| {
        OpError::usage(format!(
            "{} has no adapter, so no knob can be written; add <data dir>/plus/adapters/{}.yaml",
            plugin.id, plugin.id
        ))
    })
}

pub(super) fn folder(cwd: &Path) -> Result<PathBuf, OpError> {
    if !fsx::is_dir(cwd) {
        return Err(OpError::usage(format!("cwd is not a folder: {}", cwd.display())));
    }
    Ok(fsx::canonical(cwd))
}

fn plan_of(summary: String, run: &Run, mut warnings: Vec<String>, undo: String) -> PlanV1 {
    warnings.extend(run.warnings.iter().cloned());
    PlanV1 { summary, steps: run.steps.clone(), effects: Effects::default(), warnings, undo }
}

pub(super) fn envelope(id: &str, scope: &str, cwd: Option<&Path>, dry: bool, plan: &PlanV1, result: Option<ResultV1>, extra: Value) -> Value {
    let mut out = json!({
        "id": id, "scope": scope, "cwd": cwd.map(fsx::display), "dryRun": dry,
        "plan": plan, "result": result,
    });
    if let (Value::Object(base), Value::Object(more)) = (&mut out, extra) {
        base.extend(more);
    }
    out
}

fn owned_env(data_dir: &Path, cwd: &Path, control: &str) -> BTreeMap<String, String> {
    let drift = controls::drifted(data_dir, cwd, control);
    controls::record(data_dir, cwd, control)
        .map(|rec| {
            rec.settings
                .owned
                .iter()
                .filter(|o| o.kind == "set" && o.path.len() == 2 && o.path[0] == "env")
                .filter(|o| !drift.contains(&o.label()))
                .filter_map(|o| Some((o.path[1].clone(), serde_json::from_str::<String>(&o.written).ok()?)))
                .collect()
        })
        .unwrap_or_default()
}

fn folder_config(env: &Env, plugin: &Installed, adapter: &Adapter, cwd: &Path, a: &ConfigArgs) -> Result<Value, OpError> {
    let data_dir = env.data_dir.as_deref().ok_or_else(|| OpError::failed("no_data_dir", "no data directory"))?;
    let control = format!("config:{}", plugin.id);
    let layers = env.layers(Some(cwd));
    let mut want = owned_env(data_dir, cwd, &control);
    let before = want.clone();
    let mut warnings = vec![SWITCHED_OFF.to_string()];
    let mut changes = Vec::new();
    for (key, value) in &a.sets {
        let knob = adapter.knob(key).expect("checked");
        let change = change_of(knob, value).map_err(OpError::usage)?;
        let (name, written) = match &change {
            EnvChange::Set(n, v) => (n.clone(), Some(v.clone())),
            EnvChange::Remove(n) => (n.clone(), None),
        };
        if knob.kind.is_list() {
            if let Some((user, Scope::User)) = layers.env_value(&name) {
                warnings.push(format!(
                    "{name} is also set in your user settings.json env ({user}); in this folder the value replaces it, it is not appended"
                ));
            }
        }
        match written {
            Some(v) => {
                want.insert(name.clone(), v.clone());
                changes.push(json!({"knob": key, "env": name, "action": "set", "value": v}));
            }
            None => {
                if want.remove(&name).is_none() {
                    warnings.push(format!("{name} is not set by Toolport in this folder; left alone"));
                }
                changes.push(json!({"knob": key, "env": name, "action": "remove", "value": null}));
            }
        }
    }
    for key in &a.unsets {
        let knob = adapter.knob(key).expect("checked");
        if want.remove(&knob.env).is_none() {
            warnings.push(format!("{} is not set by Toolport in this folder; left alone", knob.env));
        }
        changes.push(json!({"knob": key, "env": knob.env, "action": "unset", "value": null}));
    }
    let drift = controls::drifted(data_dir, cwd, &control);
    if !drift.is_empty() {
        warnings.push(format!("changed since the last apply and left as they are: {}", drift.join(", ")));
    }
    let desired = Desired { env: want.iter().map(|(k, v)| (k.clone(), v.clone())).collect(), ..Desired::default() };
    let run = controls::apply(data_dir, cwd, &control, &desired, a.dry_run).map_err(bundle_op)?;

    let mut undo = format!("toolportctl plugins config {} --cwd {}", plugin.id, quote(&fsx::display(cwd)));
    let knob_of_env = |name: &str| adapter.knobs.iter().find(|k| k.env == name);
    for change in &changes {
        let env_name = change["env"].as_str().unwrap_or_default();
        let Some(knob) = knob_of_env(env_name) else { continue };
        match before.get(env_name) {
            Some(old) => {
                let shown = if knob.kind == Kind::BoolOff { "off" } else { old.as_str() };
                undo.push_str(&format!(" --set {}={}", knob.key, quote(shown)));
            }
            None => undo.push_str(&format!(" --unset {}", knob.key)),
        }
    }
    let summary = format!("Set {} knob(s) of {} in {}", changes.len(), plugin.id, cwd.display());
    let plan = plan_of(summary, &run, warnings, undo.clone());
    let result = (!a.dry_run).then(|| ResultV1 { applied: true, changed: run.changed.clone(), undo, backups: Vec::new() });
    Ok(envelope(
        &plugin.id,
        "folder",
        Some(cwd),
        a.dry_run,
        &plan,
        result,
        json!({"changes": changes, "conflicts": run.conflicts, "ledger": run.ledger, "adapter": adapter.plugin}),
    ))
}

fn global_config(
    runner: Option<&dyn ClaudeRunner>,
    plugin: &Installed,
    adapter: &Adapter,
    a: &ConfigArgs,
) -> Result<Value, OpError> {
    let Some(runner) = runner else {
        return Err(OpError::conflict(format!("{NOT_FOUND}: the global path needs `claude plugin configure`")));
    };
    let options = cli::configure(runner, &plugin.id).map_err(OpError::conflict)?;
    let mut values = serde_json::Map::new();
    let mut undo_values: Vec<(String, Option<String>)> = Vec::new();
    let mut warnings = vec![
        "This changes the plugin option for every folder (user scope); a folder's env key still wins over it".to_string(),
        SWITCHED_OFF.to_string(),
    ];
    let mut put = |knob: &Knob, text: String, action: &str, changes: &mut Vec<Value>| -> Result<(), OpError> {
        let option = knob.option.as_deref().expect("checked");
        let row = options.iter().find(|r| r.key == option).ok_or_else(|| {
            OpError::usage(format!("{} has no plugin option '{option}'; set {} in a folder with --cwd", plugin.id, knob.key))
        })?;
        if row.sensitive {
            return Err(OpError::usage(format!("{} is a sensitive option; set it with `claude plugin configure`", knob.key)));
        }
        undo_values.push((knob.key.clone(), row.current.clone()));
        values.insert(option.to_string(), Value::String(text.clone()));
        changes.push(json!({"knob": knob.key, "option": option, "action": action, "value": text}));
        Ok(())
    };
    let mut changes_out = Vec::new();
    for (key, value) in &a.sets {
        let knob = adapter.knob(key).expect("checked");
        let text = match change_of(knob, value).map_err(OpError::usage)? {
            EnvChange::Set(_, v) => v,
            EnvChange::Remove(_) => "on".to_string(),
        };
        put(knob, text, "set", &mut changes_out)?;
    }
    for key in &a.unsets {
        let knob = adapter.knob(key).expect("checked");
        let Some(default) = knob.default.clone() else {
            return Err(OpError::usage(format!(
                "{key} has no default to go back to; a plugin option cannot be removed, set it to a value"
            )));
        };
        warnings.push(format!("{key} goes back to its default ({default}): `claude plugin configure` cannot remove an option"));
        put(knob, default, "unset", &mut changes_out)?;
    }
    let body = Value::Object(values).to_string();
    let step = crate::plus::plan::Step {
        op: "exec",
        path: None,
        detail: format!(
            "claude plugin configure {} --values-stdin (user scope); the JSON object goes over stdin, never in an argument: {}",
            plugin.id,
            changes_out.iter().map(|c| format!("{}={}", c["option"].as_str().unwrap_or(""), c["value"].as_str().unwrap_or(""))).collect::<Vec<_>>().join(", ")
        ),
        keys: Some(changes_out.iter().filter_map(|c| c["option"].as_str().map(String::from)).collect()),
        diff: None,
    };
    let mut undo = format!("toolportctl plugins config {}", plugin.id);
    for (key, old) in &undo_values {
        match old {
            Some(old) => undo.push_str(&format!(" --set {key}={}", quote(old))),
            None => undo.push_str(&format!(" --unset {key}")),
        }
    }
    let plan = PlanV1 {
        summary: format!("Set {} plugin option(s) of {} for all folders", changes_out.len(), plugin.id),
        steps: vec![step],
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let mut result = None;
    if !a.dry_run {
        let out = runner
            .run_with_input(&["plugin", "configure", &plugin.id, "--values-stdin"], &body, CONFIGURE_TIMEOUT)
            .map_err(OpError::conflict)?;
        if !out.ok() {
            let line = out.first_error_line();
            return Err(OpError::conflict(if line.is_empty() {
                format!("claude plugin configure exited {}", out.code)
            } else {
                line
            }));
        }
        result = Some(ResultV1 {
            applied: true,
            changed: vec![format!("claude plugin configure {} (user scope)", plugin.id)],
            undo,
            backups: Vec::new(),
        });
    }
    Ok(envelope(&plugin.id, "global", None, a.dry_run, &plan, result, json!({"changes": changes_out, "conflicts": [], "adapter": adapter.plugin})))
}

/// `plugins config <id> [--cwd] [--set k=v]... [--unset k]...`.
pub fn config(env: &Env, runner: Option<&dyn ClaudeRunner>, a: &ConfigArgs) -> Result<Value, OpError> {
    if a.sets.is_empty() && a.unsets.is_empty() {
        return Err(OpError::usage("nothing to do: give --set <knob>=<value> or --unset <knob>"));
    }
    let plugin = installed_plugin(env, runner, a.id)?;
    let registry = Registry::load(env.data_dir.as_deref());
    let adapter = adapter_of(&registry, &plugin)?;
    let mut seen: Vec<&str> = Vec::new();
    for key in a.sets.iter().map(|(k, _)| k.as_str()).chain(a.unsets.iter().map(String::as_str)) {
        let knob = adapter
            .knob(key)
            .ok_or_else(|| OpError::usage(format!("{} has no knob '{key}' (it has: {})", plugin.id, adapter.knob_names())))?;
        if seen.contains(&key) {
            return Err(OpError::usage(format!("knob '{key}' is given more than once")));
        }
        seen.push(key);
        if a.cwd.is_none() && knob.option.is_none() {
            return Err(OpError::usage(format!(
                "knob '{key}' is folder-only (no plugin option behind it); pass --cwd <dir> to set it as an env key in that folder"
            )));
        }
    }
    match a.cwd {
        Some(cwd) => folder_config(env, &plugin, adapter, &folder(cwd)?, a),
        None => global_config(runner, &plugin, adapter, a),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum McpOp {
    Deny,
    Allow,
}

fn walk_md(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 || out.len() > 2000 {
        return;
    }
    for e in fsx::list_dir(dir) {
        match e.kind {
            fsx::Kind::Dir => walk_md(&e.path, depth - 1, out),
            _ if e.name.ends_with(".md") => out.push(e.path),
            _ => {}
        }
    }
}

/// Skills, agents and commands of the plugin whose text names the server's tools.
pub fn naming_tools(install: &Path, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (kind, dir) in [("skill", "skills"), ("agent", "agents"), ("command", "commands")] {
        let mut files = Vec::new();
        walk_md(&install.join(dir), 4, &mut files);
        files.sort();
        for f in files {
            if fsx::read_text(&f, 256 * 1024).is_some_and(|t| t.contains(prefix)) {
                let name = if kind == "skill" {
                    f.parent().and_then(|p| p.file_name())
                } else {
                    f.file_stem()
                };
                out.push(format!("{kind} {}", name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            }
        }
    }
    out
}

/// `plugins mcp deny|allow <id> <server> --cwd <dir>`.
pub fn mcp(
    env: &Env,
    runner: Option<&dyn ClaudeRunner>,
    op: McpOp,
    id: &str,
    server: &str,
    cwd: &Path,
    dry_run: bool,
) -> Result<Value, OpError> {
    let plugin = installed_plugin(env, runner, id)?;
    let cwd = folder(cwd)?;
    let data_dir = env.data_dir.as_deref().ok_or_else(|| OpError::failed("no_data_dir", "no data directory"))?;
    let install = plugin.install_path.clone().filter(|d| fsx::is_dir(d));
    let doc = install.as_deref().map(manifest::load).unwrap_or(Value::Null);
    let servers = install.as_deref().map(|d| manifest::mcp_servers(d, &doc)).unwrap_or_default();
    let Some(found) = servers.iter().find(|s| s.name == server) else {
        let names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
        return Err(OpError::not_found(format!(
            "{} has no MCP server '{server}' (it has: {})",
            plugin.id,
            if names.is_empty() { "none".to_string() } else { names.join(", ") }
        )));
    };
    let key = format!("plugin:{}:{}", plugin.name, found.name);
    let control = format!("mcp:{}:{}", plugin.id, found.name);
    let prefix = manifest::tool_prefix(&plugin.name, &found.name);
    let layers = env.layers(Some(&cwd));
    let denied = layers.denied(&key, found.raw_command.as_deref(), found.raw_url.as_deref());
    let owned_here = controls::record(data_dir, &cwd, &control).is_some();
    let cwd_text = quote(&fsx::display(&cwd));
    let mut warnings = Vec::new();
    let run = match op {
        McpOp::Deny => {
            if denied.any() && !owned_here {
                warnings.push(format!("{key} is already denied by settings Toolport did not write; nothing is added"));
            }
            let want = Desired { deny_servers: vec![key.clone()], ..Desired::default() };
            let run = controls::apply(data_dir, &cwd, &control, &want, dry_run).map_err(bundle_op)?;
            warnings.push(format!(
                "Denying removes every tool named {prefix}* from Claude Code in this folder (a measured run shows the count); the server stays outside the gateway"
            ));
            let naming = naming_tools(install.as_deref().unwrap_or(Path::new("")), &prefix);
            if !naming.is_empty() {
                warnings.push(format!(
                    "{} of this plugin's skills, agents or commands name these tools and will fail without them: {}",
                    naming.len(),
                    naming.join(", ")
                ));
            }
            run
        }
        McpOp::Allow => {
            if !owned_here {
                warnings.push(if denied.any() {
                    format!("{key} is denied by settings Toolport did not write; they are left alone")
                } else {
                    format!("{key} is not denied here; nothing to remove")
                });
            }
            controls::undo(data_dir, &cwd, &control, dry_run).map_err(bundle_op)?
        }
    };
    let (verb, undo_op) = match op {
        McpOp::Deny => ("Deny", "allow"),
        McpOp::Allow => ("Allow", "deny"),
    };
    let undo = format!("toolportctl plugins mcp {undo_op} {} {} --cwd {cwd_text}", plugin.id, found.name);
    let plan = plan_of(format!("{verb} {key} in {}", cwd.display()), &run, warnings, undo.clone());
    let result = (!dry_run).then(|| ResultV1 { applied: true, changed: run.changed.clone(), undo, backups: Vec::new() });
    Ok(envelope(
        &plugin.id,
        "folder",
        Some(&cwd),
        dry_run,
        &plan,
        result,
        json!({"server": found.name, "serverName": key, "toolPrefix": prefix, "conflicts": run.conflicts, "ledger": run.ledger}),
    ))
}
