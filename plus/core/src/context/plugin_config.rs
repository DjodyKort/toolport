//! `plugins config` and `plugins mcp`: the switches a plugin really has (its adapter's knobs) and
//! the MCP servers it starts, written the way a bundle writes a folder and undone the same way.
//!
//! In a folder a knob is an `env` key and a denied server a `deniedMcpServers` entry of
//! `.claude/settings.local.json`, through the controls ledger (`config:<id>`, `mcp:<id>:<server>`):
//! every other key stays byte-identical and undo restores only what Toolport wrote. Without a
//! folder a knob that is a plugin option goes through `claude plugin configure --values-stdin`,
//! with the values on stdin and never in an argument list.

use super::bundle_apply::Desired;
use super::bundle_controls as controls;
use super::bundle_store::op as bundle_op;
use super::fsx;
use super::plan::{Effects, PlanV1, ResultV1};
use super::plugin_adapters::{self as adapters, Adapter, EnvChange, Knob, Registry};
use super::plugin_switch::{folder, note, quote, Outcome, RESTART};
use crate::op::OpError;
use crate::plugins::claude::{ClaudeRunner, NOT_FOUND};
use crate::plugins::detail;
use crate::plugins::installed::{self, Installed};
use crate::plugins::inventory::{layers, World};
use crate::untrusted;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(30);
const MENTION_CAP: u64 = 256 * 1024;
const STILL_STARTS: &str = "a hook that is switched off still starts a process and exits at once; only turning the plugin off removes it";

pub fn config_control(id: &str) -> String {
    format!("config:{id}")
}

pub fn mcp_control(id: &str, server: &str) -> String {
    format!("mcp:{id}:{server}")
}

fn resolve(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    id: &str,
) -> Result<Installed, OpError> {
    if !adapters::valid_ident(id) {
        return Err(OpError::usage(format!("invalid plugin id: {id}")));
    }
    let listing = installed::list(runner.filter(|r| r.present()), &world.claude_home);
    if let Some(found) = listing.plugins.iter().find(|p| p.id == id) {
        return Ok(found.clone());
    }
    let named: Vec<&Installed> = listing.plugins.iter().filter(|p| p.name == id).collect();
    match named.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(OpError::not_found(format!(
            "{id} is not among the installed plugins; `toolportctl plugins ls` shows the ids"
        ))),
        many => Err(OpError::usage(format!(
            "{id} is installed from more than one marketplace: {}",
            many.iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn data_dir(world: &World) -> Result<&Path, OpError> {
    world
        .data_dir
        .as_deref()
        .ok_or_else(|| OpError::failed("no_data_dir", "the data directory could not be resolved"))
}

fn written_env(data_dir: &Path, cwd: &Path, control: &str) -> BTreeMap<String, String> {
    controls::record(data_dir, cwd, control)
        .map(|rec| {
            rec.settings
                .owned
                .iter()
                .filter(|o| o.path.len() == 2 && o.path[0] == "env")
                .filter_map(|o| {
                    let value = serde_json::from_str::<Value>(&o.written).ok()?;
                    Some((o.path[1].clone(), value.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What `plugins config <id>` shows without `--set` or `--unset`: the plugin's own options, the
/// adapter's knobs with their current values and, in a folder, what Toolport wrote there.
pub fn read(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    id: &str,
    cwd: Option<&Path>,
) -> Result<Value, OpError> {
    let plugin = resolve(world, runner, id)?;
    let cwd = cwd.map(folder).transpose()?;
    let layers = layers(world, cwd.as_deref());
    let registry = Registry::load(world.data_dir.as_deref());
    let adapter = registry.for_plugin(&plugin.id);
    let (options, from) = detail::options(runner.filter(|r| r.present()), &plugin, &layers);
    let knobs = detail::knobs(adapter, &layers, &plugin.id);
    let written: Vec<Value> = match (&cwd, world.data_dir.as_deref(), adapter) {
        (Some(cwd), Some(data), Some(adapter)) => {
            let env = written_env(data, cwd, &config_control(&plugin.id));
            adapter
                .knobs
                .iter()
                .filter_map(|k| {
                    Some(json!({"knob": k.key, "env": k.env, "value": env.get(&k.env)?}))
                })
                .collect()
        }
        _ => Vec::new(),
    };
    Ok(json!({
        "id": plugin.id,
        "scope": if cwd.is_some() { "folder" } else { "global" },
        "cwd": cwd.as_deref().map(fsx::display),
        "adapter": adapter.map(|a| a.plugin.clone()),
        "options": options,
        "optionsFrom": from,
        "knobs": knobs,
        "writtenByToolport": written,
        "adapterProblems": registry.problems_json(),
    }))
}

pub struct ConfigArgs<'a> {
    pub id: &'a str,
    pub cwd: Option<&'a Path>,
    pub set: &'a [(String, String)],
    pub unset: &'a [String],
    pub dry_run: bool,
}

fn need_adapter<'a>(registry: &'a Registry, id: &str) -> Result<&'a Adapter, OpError> {
    registry.for_plugin(id).ok_or_else(|| {
        OpError::usage(format!(
            "{id} has no adapter, so Toolport does not know its switches; an adapter is a file <data dir>/plus/adapters/{id}.yaml"
        ))
    })
}

fn known_knob<'a>(adapter: &'a Adapter, key: &str) -> Result<&'a Knob, OpError> {
    adapter.knob(key).ok_or_else(|| {
        OpError::usage(format!(
            "{key} is not a knob of {}: use {}",
            adapter.plugin,
            adapter.knob_names()
        ))
    })
}

fn change_row(knob: &Knob, action: &str, value: Option<&str>, folder: bool) -> Value {
    json!({
        "knob": knob.key,
        "env": folder.then_some(&knob.env),
        "option": (!folder).then_some(&knob.option),
        "action": action,
        "value": value,
    })
}

/// `plugins config <id> --set k=v --unset k`, in a folder (`cwd`) or globally.
pub fn run(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    args: &ConfigArgs,
) -> Result<Value, OpError> {
    let plugin = resolve(world, runner, args.id)?;
    let registry = Registry::load(world.data_dir.as_deref());
    let adapter = need_adapter(&registry, &plugin.id)?;
    let mut sets: Vec<(&Knob, &str)> = Vec::new();
    for (key, value) in args.set {
        let knob = known_knob(adapter, key)?;
        adapters::check_value(knob, value).map_err(OpError::usage)?;
        sets.retain(|(k, _)| k.key != knob.key);
        sets.push((knob, value));
    }
    let mut unsets: Vec<&Knob> = Vec::new();
    for key in args.unset {
        let knob = known_knob(adapter, key)?;
        if sets.iter().any(|(k, _)| k.key == knob.key) {
            return Err(OpError::usage(format!("{key} is both set and unset")));
        }
        unsets.push(knob);
    }
    if sets.is_empty() && unsets.is_empty() {
        return Err(OpError::usage(
            "nothing to change: pass --set <knob>=<value> or --unset <knob>",
        ));
    }
    match args.cwd {
        Some(cwd) => in_folder(world, &plugin, adapter, cwd, &sets, &unsets, args.dry_run),
        None => globally(
            world,
            runner,
            &plugin,
            adapter,
            &sets,
            &unsets,
            args.dry_run,
        ),
    }
}

fn in_folder(
    world: &World,
    plugin: &Installed,
    adapter: &Adapter,
    cwd: &Path,
    sets: &[(&Knob, &str)],
    unsets: &[&Knob],
    dry_run: bool,
) -> Result<Value, OpError> {
    let data = data_dir(world)?;
    let cwd = folder(cwd)?;
    let control = config_control(&plugin.id);
    let layers = layers(world, Some(&cwd));
    let mut env = written_env(data, &cwd, &control);
    let mut changes = Vec::new();
    let mut warnings = Vec::new();
    for (knob, value) in sets {
        match adapters::change_of(knob, value).map_err(OpError::usage)? {
            EnvChange::Set(name, v) => {
                changes.push(change_row(knob, "set", Some(&v), true));
                env.insert(name, v);
            }
            EnvChange::Remove(name) => {
                changes.push(change_row(knob, "remove", None, true));
                env.remove(&name);
            }
        }
        let outside = layers
            .files()
            .iter()
            .filter(|f| f.scope == crate::plugins::settings::Scope::User)
            .find_map(|f| f.get("env")?.get(&knob.env)?.as_str());
        if let (Some(user), true) = (outside, knob.kind.is_list()) {
            warnings.push(format!(
                "{} is {user:?} in your user settings; in this folder the value is replaced, not appended",
                knob.env
            ));
        }
    }
    for knob in unsets {
        if env.remove(&knob.env).is_none() {
            warnings.push(format!(
                "{} holds no value Toolport wrote in this folder; nothing to remove",
                knob.key
            ));
        }
        changes.push(change_row(knob, "unset", None, true));
    }
    let want = Desired {
        env: env.clone().into_iter().collect(),
        ..Desired::default()
    };
    let outcome = Outcome::from_run(
        controls::apply(data, &cwd, &control, &want, dry_run).map_err(bundle_op)?,
    );
    warnings.extend(outcome.warnings.iter().cloned());
    warnings.push(STILL_STARTS.to_string());
    warnings.push(RESTART.to_string());
    let touched: Vec<String> = sets
        .iter()
        .map(|(k, _)| k.key.clone())
        .chain(unsets.iter().map(|k| k.key.clone()))
        .collect();
    let undo = format!(
        "toolportctl plugins config {} --cwd {}{}",
        plugin.id,
        quote(&fsx::display(&cwd)),
        touched
            .iter()
            .map(|k| format!(" --unset {k}"))
            .collect::<String>()
    );
    let mut steps = outcome.steps;
    steps.push(note(format!(
        "writes env keys of {} in {}; every other key stays as it is",
        plugin.id,
        fsx::display(&cwd.join(".claude/settings.local.json"))
    )));
    let plan = PlanV1 {
        summary: format!(
            "Set {} for {} in {}",
            touched.join(", "),
            plugin.id,
            cwd.display()
        ),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed: outcome.changed.clone(),
        undo,
        backups: Vec::new(),
    });
    Ok(json!({
        "id": plugin.id,
        "scope": "folder",
        "cwd": fsx::display(&cwd),
        "dryRun": dry_run,
        "plan": plan,
        "result": result,
        "changes": changes,
        "conflicts": outcome.conflicts,
        "ledger": outcome.ledger,
        "adapter": adapter.plugin,
    }))
}

fn globally(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    plugin: &Installed,
    adapter: &Adapter,
    sets: &[(&Knob, &str)],
    unsets: &[&Knob],
    dry_run: bool,
) -> Result<Value, OpError> {
    let mut values = serde_json::Map::new();
    let mut changes = Vec::new();
    let mut warnings = Vec::new();
    let option_of = |knob: &Knob| {
        knob.option.clone().ok_or_else(|| {
            OpError::usage(format!(
                "{} is a switch of one folder, not a plugin option: pass --cwd <dir>",
                knob.key
            ))
        })
    };
    for (knob, value) in sets {
        let option = option_of(knob)?;
        let text = match adapters::change_of(knob, value).map_err(OpError::usage)? {
            EnvChange::Set(_, v) => v,
            EnvChange::Remove(_) => knob.default.clone().ok_or_else(|| {
                OpError::usage(format!("{} has no value to write for {value}", knob.key))
            })?,
        };
        changes.push(change_row(knob, "set", Some(&text), false));
        values.insert(option, Value::String(text));
    }
    for knob in unsets {
        let option = option_of(knob)?;
        let default = knob.default.clone().ok_or_else(|| {
            OpError::usage(format!(
                "{} has no default; a plugin option cannot be removed, only set",
                knob.key
            ))
        })?;
        warnings.push(format!(
            "{} cannot be removed from the plugin's options: its default {default:?} is written",
            knob.key
        ));
        changes.push(change_row(knob, "unset", Some(&default), false));
        values.insert(option, Value::String(default));
    }
    let payload = Value::Object(values).to_string();
    let mut steps = vec![note(format!(
        "run `claude plugin configure {} --values-stdin`: the values go in on stdin, options left out keep their value",
        plugin.id
    ))];
    warnings.push(STILL_STARTS.to_string());
    warnings.push(RESTART.to_string());
    let touched: Vec<&str> = changes.iter().filter_map(|c| c["knob"].as_str()).collect();
    let undo = format!(
        "toolportctl plugins config {} --unset {}",
        plugin.id,
        touched.join(" --unset ")
    );
    let mut changed = Vec::new();
    if !dry_run {
        let runner = runner
            .filter(|r| r.present())
            .ok_or_else(|| OpError::conflict(NOT_FOUND.to_string()))?;
        let out = runner
            .run_input(
                &["plugin", "configure", &plugin.id, "--values-stdin"],
                Some(&payload),
                CONFIGURE_TIMEOUT,
            )
            .map_err(OpError::conflict)?;
        if !out.ok() {
            return Err(OpError::failed(
                "claude_failed",
                installed::failure(&out, "claude plugin configure"),
            ));
        }
        changed.push(format!("plugin options of {}", plugin.id));
        steps.push(note("the options were saved".into()));
    } else if runner.is_none_or(|r| !r.present()) {
        return Err(OpError::conflict(NOT_FOUND.to_string()));
    }
    let plan = PlanV1 {
        summary: format!("Set plugin options of {}", plugin.id),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed,
        undo,
        backups: Vec::new(),
    });
    let _ = world;
    Ok(json!({
        "id": plugin.id,
        "scope": "global",
        "cwd": null,
        "dryRun": dry_run,
        "plan": plan,
        "result": result,
        "changes": changes,
        "conflicts": Vec::<String>::new(),
        "ledger": null,
        "adapter": adapter.plugin,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpOp {
    Deny,
    Allow,
}

pub struct McpArgs<'a> {
    pub op: McpOp,
    pub id: &'a str,
    pub server: &'a str,
    pub cwd: &'a Path,
    pub dry_run: bool,
}

/// `plugins mcp ls <id>`: the servers a plugin starts and where each is denied.
pub fn mcp_read(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    id: &str,
    cwd: Option<&Path>,
) -> Result<Value, OpError> {
    let plugin = resolve(world, runner, id)?;
    let cwd = cwd.map(folder).transpose()?;
    let layers = layers(world, cwd.as_deref());
    let root = plugin.install_path.clone().unwrap_or_default();
    let servers: Vec<Value> = detail::mcp_servers(&root)
        .iter()
        .map(|(name, config)| {
            let mut row = detail::mcp_row(&plugin, name, config, &layers);
            let by_toolport =
                cwd.as_deref()
                    .zip(world.data_dir.as_deref())
                    .is_some_and(|(cwd, data)| {
                        controls::record(data, cwd, &mcp_control(&plugin.id, name)).is_some()
                    });
            row["deniedByToolport"] = json!(by_toolport);
            row
        })
        .collect();
    Ok(json!({
        "id": plugin.id,
        "cwd": cwd.as_deref().map(fsx::display),
        "servers": servers,
        "outsideGateway": "these servers are started by Claude Code, not by the Toolport gateway",
    }))
}

fn mentions(root: &Path, prefix: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (dir, want_dirs) in [("skills", true), ("agents", false), ("commands", false)] {
        for entry in untrusted::list_dir(&root.join(dir)) {
            let file = if want_dirs {
                entry.path.join("SKILL.md")
            } else {
                entry.path.clone()
            };
            if untrusted::read_text(&file, MENTION_CAP).is_some_and(|text| text.contains(prefix)) {
                found.push(format!("{dir}/{}", entry.name.trim_end_matches(".md")));
            }
        }
    }
    found.sort();
    found
}

/// `plugins mcp deny|allow <id> <server> --cwd <dir>`.
pub fn mcp(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    args: &McpArgs,
) -> Result<Value, OpError> {
    let plugin = resolve(world, runner, args.id)?;
    if !adapters::valid_ident(args.server) {
        return Err(OpError::usage(format!(
            "invalid server name: {}",
            args.server
        )));
    }
    let data = data_dir(world)?;
    let cwd = folder(args.cwd)?;
    let root = plugin.install_path.clone().unwrap_or_default();
    let servers = detail::mcp_servers(&root);
    let control = mcp_control(&plugin.id, args.server);
    let record = controls::record(data, &cwd, &control);
    if !servers.iter().any(|(name, _)| name == args.server) && record.is_none() {
        return Err(OpError::not_found(format!(
            "{} has no MCP server {}; it has: {}",
            plugin.id,
            args.server,
            servers
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let key = detail::mcp_key(&plugin.name, args.server);
    let prefix = format!("mcp__plugin_{}_{}__", plugin.name, args.server);
    let layers = layers(world, Some(&cwd));
    let denied_now = detail::denied(&layers, &key);
    let mut warnings = Vec::new();
    let outcome = match args.op {
        McpOp::Deny => {
            if record.is_some() {
                Outcome::skipped("already denied by `plugins mcp deny`; nothing is written".into())
            } else if denied_now["local"] == true {
                Outcome::skipped(format!(
                    "{key} is already denied in this folder's settings.local.json and Toolport did not write it; it is left alone"
                ))
            } else {
                let want = Desired {
                    deny_servers: vec![key.clone()],
                    ..Desired::default()
                };
                Outcome::from_run(
                    controls::apply(data, &cwd, &control, &want, args.dry_run)
                        .map_err(bundle_op)?,
                )
            }
        }
        McpOp::Allow => {
            if record.is_some() {
                Outcome::from_run(
                    controls::undo(data, &cwd, &control, args.dry_run).map_err(bundle_op)?,
                )
            } else {
                Outcome::skipped(
                    "not denied by `plugins mcp deny` in this folder; nothing to remove".into(),
                )
            }
        }
    };
    warnings.extend(outcome.warnings.iter().cloned());
    if args.op == McpOp::Deny {
        for scope in ["user", "project", "managed"] {
            if denied_now[scope] == true {
                warnings.push(format!("{key} is already denied in the {scope} settings"));
            }
        }
        let skills = mentions(&root, &prefix);
        if !skills.is_empty() {
            warnings.push(format!(
                "these parts of the plugin name the tools {prefix}*, which will not exist: {}",
                skills.join(", ")
            ));
        }
    }
    warnings.push("the server stays outside the Toolport gateway; this only tells Claude Code not to start it here".into());
    warnings.push(RESTART.to_string());
    let (verb, back) = match args.op {
        McpOp::Deny => ("deny", "allow"),
        McpOp::Allow => ("allow", "deny"),
    };
    let cwd_text = quote(&fsx::display(&cwd));
    let undo = format!(
        "toolportctl plugins mcp {back} {} {} --cwd {cwd_text}",
        plugin.id, args.server
    );
    let moved = outcome.acted && outcome.conflicts.is_empty();
    let change = match (args.op, moved) {
        (McpOp::Deny, true) => json!({"key": "deniedMcpServers", "action": "add", "value": key}),
        (McpOp::Allow, true) => {
            json!({"key": "deniedMcpServers", "action": "remove", "value": key})
        }
        _ => json!({"key": "deniedMcpServers", "action": "none", "value": key}),
    };
    let mut steps = outcome.steps;
    steps.push(note(format!(
        "tools of the server are named {prefix}*; the plugin itself stays on"
    )));
    let plan = PlanV1 {
        summary: format!(
            "{} MCP server {} of {} in {}",
            capital(verb),
            args.server,
            plugin.id,
            cwd.display()
        ),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = (!args.dry_run).then(|| ResultV1 {
        applied: true,
        changed: outcome.changed.clone(),
        undo,
        backups: Vec::new(),
    });
    Ok(json!({
        "id": plugin.id,
        "server": args.server,
        "serverName": key,
        "toolPrefix": format!("{prefix}*"),
        "scope": "folder",
        "cwd": fsx::display(&cwd),
        "dryRun": args.dry_run,
        "plan": plan,
        "result": result,
        "changes": [change],
        "conflicts": outcome.conflicts,
        "ledger": outcome.ledger,
    }))
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}
