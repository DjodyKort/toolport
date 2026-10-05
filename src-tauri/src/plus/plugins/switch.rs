//! `plugins off|on` (one folder) and `plugins disable|enable` (user scope), D-097. A folder gets
//! `enabledPlugins.<id> = false` in its `settings.local.json` through the controls ledger of
//! D-093, so every other key stays byte-identical and `on` removes only what Toolport wrote. The
//! user scope is Claude Code's own `claude plugin disable|enable <id> --scope user`, nothing else.

use super::claude::{ClaudeRunner, SystemClaude, NOT_FOUND};
use super::config::{envelope, folder, host_failure, quote};
use super::installed::{self, valid_ident, Installed};
use super::report::{self, Opts};
use super::{api, Env};
use crate::plus::context::bundle_api::op as bundle_op;
use crate::plus::context::bundle_apply::{folder_key, Desired};
use crate::plus::context::bundle_controls::{self as controls, Run};
use crate::plus::context::bundle_ledger as ledger;
use crate::plus::op::OpError;
use crate::plus::plan::{Effects, PlanV1, ResultV1, Step};
use crate::plus::sources::fsx;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const CLAUDE_TIMEOUT: Duration = Duration::from_secs(60);
const SHOWN_NAMES: usize = 8;
const RESTART: &str = "Claude Code reads plugin settings when a session starts: sessions that are already running keep the old state until they restart";
const STAYS: &str = "your own skills, agents, commands and hooks, every other plugin and the servers behind the Toolport gateway";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Off,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserOp {
    Disable,
    Enable,
}

impl UserOp {
    fn verb(self) -> &'static str {
        match self {
            UserOp::Disable => "disable",
            UserOp::Enable => "enable",
        }
    }

    fn other(self) -> &'static str {
        match self {
            UserOp::Disable => "enable",
            UserOp::Enable => "disable",
        }
    }
}

#[derive(Default)]
struct Brings {
    skills: Vec<String>,
    agents: Vec<String>,
    commands: Vec<String>,
    hooks: usize,
    servers: Vec<String>,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default()
}

fn brings_of(shown: &Value) -> Brings {
    Brings {
        skills: strings(&shown["components"]["skills"]),
        agents: strings(&shown["components"]["agents"]),
        commands: strings(&shown["components"]["commands"]),
        hooks: shown["hooks"].as_array().map_or(0, Vec::len),
        servers: shown["mcpServers"]
            .as_array()
            .map(|list| list.iter().filter_map(|s| s["key"].as_str()).map(String::from).collect())
            .unwrap_or_default(),
    }
}

fn counted(names: &[String], one: &str, many: &str) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let shown: Vec<&str> = names.iter().take(SHOWN_NAMES).map(String::as_str).collect();
    let more = names.len().saturating_sub(SHOWN_NAMES);
    Some(format!(
        "{} {} ({}{})",
        names.len(),
        if names.len() == 1 { one } else { many },
        shown.join(", "),
        if more > 0 { format!(", and {more} more") } else { String::new() }
    ))
}

fn brings_text(b: &Brings) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.extend(counted(&b.skills, "skill", "skills"));
    parts.extend(counted(&b.agents, "agent", "agents"));
    parts.extend(counted(&b.commands, "command", "commands"));
    if b.hooks > 0 {
        parts.push(format!("{} hook handler{}", b.hooks, if b.hooks == 1 { "" } else { "s" }));
    }
    parts.extend(counted(&b.servers, "MCP server", "MCP servers"));
    if parts.is_empty() {
        "nothing that could be read (the install folder may be missing)".to_string()
    } else {
        parts.join(", ")
    }
}

fn state_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "not set",
    }
}

fn note(detail: String) -> Step {
    Step { op: "note", path: None, detail, keys: None, diff: None }
}

fn installed_plugin(env: &Env, id: &str) -> Result<Installed, OpError> {
    if !valid_ident(id) {
        return Err(OpError::usage(format!("invalid plugin id: {id}")));
    }
    let listing = installed::list(None, &env.claude_home);
    report::find(&listing.plugins, id).cloned().map_err(host_failure)
}

fn managed_warning(id: &str, managed: Option<bool>) -> Option<String> {
    managed.map(|on| {
        format!(
            "managed settings set {id} to {} and win over user, project and local settings",
            if on { "on" } else { "off" }
        )
    })
}

#[derive(Default)]
struct Outcome {
    acted: bool,
    steps: Vec<Step>,
    warnings: Vec<String>,
    conflicts: Vec<String>,
    changed: Vec<String>,
    ledger: Option<String>,
}

impl Outcome {
    fn skipped(warning: String) -> Self {
        Self { warnings: vec![warning], ..Self::default() }
    }

    fn from_run(run: Run) -> Self {
        Self {
            acted: true,
            steps: run.steps,
            warnings: run.warnings,
            conflicts: run.conflicts,
            changed: run.changed,
            ledger: Some(run.ledger),
        }
    }
}

fn local_value(cwd: &Path, id: &str) -> Option<bool> {
    let text = fsx::read_text(&cwd.join(".claude/settings.local.json"), 4 * 1024 * 1024)?;
    serde_json::from_str::<Value>(&text).ok()?.get("enabledPlugins")?.get(id)?.as_bool()
}

fn bundle_owner(data_dir: &Path, cwd: &Path, id: &str) -> Option<String> {
    let all = ledger::load(data_dir);
    let rec = all.folders.get(&folder_key(cwd))?;
    rec.settings
        .owned
        .iter()
        .any(|o| o.path == ["enabledPlugins", id])
        .then(|| rec.bundle.clone())
}

fn change_row(op: Switch, id: &str, outcome: &Outcome, owned: Option<&ledger::Owned>) -> Value {
    let key = format!("enabledPlugins.{id}");
    let moved = outcome.acted && outcome.conflicts.is_empty();
    match (op, moved) {
        (Switch::Off, true) => json!({"key": key, "action": "set", "value": false}),
        (Switch::On, true) => match owned.filter(|o| o.existed) {
            Some(o) => json!({
                "key": key, "action": "restore",
                "value": o.before.as_deref().and_then(|b| serde_json::from_str::<Value>(b).ok()),
            }),
            None => json!({"key": key, "action": "remove", "value": null}),
        },
        _ => json!({"key": key, "action": "none", "value": null}),
    }
}

/// `plugins off|on <id> --cwd <dir>`.
pub fn folder_switch(env: &Env, op: Switch, id: &str, cwd: &Path, dry_run: bool) -> Result<Value, OpError> {
    let plugin = installed_plugin(env, id)?;
    let cwd = folder(cwd)?;
    let data_dir = env.data_dir.as_deref().ok_or_else(|| OpError::failed("no_data_dir", "no data directory"))?;
    let id = plugin.id.as_str();
    let control = format!("off:{id}");
    let state = env.layers(Some(&cwd)).enabled(id);
    let shown = report::show(env, None, id, &Opts { cwd: Some(&cwd), refresh: false }).map_err(host_failure)?;
    let brings = brings_text(&brings_of(&shown));

    let record = controls::record(data_dir, &cwd, &control);
    let drift = controls::drifted(data_dir, &cwd, &control);
    let bundle = bundle_owner(data_dir, &cwd, id);
    let here = local_value(&cwd, id);
    let elsewhere = state.managed.or(state.project).or(state.user).unwrap_or(false);
    let label = format!("enabledPlugins.{id}");

    let mut outcome = match op {
        Switch::Off => {
            if record.is_some() && !drift.is_empty() {
                let mut o = Outcome::skipped(format!(
                    "{label} was changed since Toolport turned the plugin off here; it is left as it is (`toolportctl plugins on` drops Toolport's record of it)"
                ));
                o.conflicts = drift.clone();
                o
            } else if record.is_some() {
                Outcome::skipped("already turned off in this folder by `plugins off`; nothing is written".into())
            } else if let Some(name) = &bundle {
                Outcome::skipped(format!("already off in this folder by bundle {name}; `context bundle undo` puts it back, nothing is written"))
            } else if here == Some(false) {
                Outcome::skipped(format!("{label} is already false in this folder's settings.local.json and Toolport did not write it; it is left alone"))
            } else {
                let want = Desired { plugins: vec![id.to_string()], ..Desired::default() };
                Outcome::from_run(controls::apply(data_dir, &cwd, &control, &want, dry_run).map_err(bundle_op)?)
            }
        }
        Switch::On => {
            if record.is_some() {
                Outcome::from_run(controls::undo(data_dir, &cwd, &control, dry_run).map_err(bundle_op)?)
            } else if let Some(name) = &bundle {
                Outcome::skipped(format!("turned off by bundle {name}, not by `plugins off`; `context bundle undo` turns it back on"))
            } else if here == Some(false) {
                Outcome::skipped(format!("{label} is false in this folder's settings.local.json and Toolport did not write it; it is left alone, edit the file to turn the plugin on"))
            } else {
                Outcome::skipped("not turned off in this folder by `plugins off`; nothing to remove".into())
            }
        }
    };
    let mut warnings = std::mem::take(&mut outcome.warnings);
    warnings.extend(managed_warning(id, state.managed));
    match op {
        Switch::Off if !elsewhere && state.managed.is_none() => warnings.push(format!(
            "{id} is not on in your user or project settings, so it is already off here; the entry only matters if one of them turns it on later"
        )),
        Switch::On if !elsewhere && state.managed.is_none() => warnings.push(format!(
            "{id} is not on in your user or project settings, so it stays off here after this; `toolportctl plugins enable {id}` turns it on everywhere"
        )),
        _ => {}
    }
    warnings.push(RESTART.to_string());

    let mut steps = std::mem::take(&mut outcome.steps);
    steps.push(note(match op {
        Switch::Off => format!("goes away in this folder: {brings}"),
        Switch::On => format!("comes back in this folder: {brings}"),
    }));
    steps.push(note(format!(
        "stays: {STAYS}; the setting in your user settings ({}) is not changed",
        state_text(state.user)
    )));

    let cwd_text = quote(&fsx::display(&cwd));
    let (verb, back) = match op {
        Switch::Off => ("off", "on"),
        Switch::On => ("on", "off"),
    };
    let undo = format!("toolportctl plugins {back} {id} --cwd {cwd_text}");
    let plan = PlanV1 {
        summary: format!("Turn {verb} {id} in {}", cwd.display()),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let owned = record.as_ref().and_then(|r| r.settings.owned.iter().find(|o| o.path == ["enabledPlugins", id]));
    let changes = vec![change_row(op, id, &outcome, owned)];
    let result = (!dry_run).then(|| ResultV1 { applied: true, changed: outcome.changed.clone(), undo, backups: Vec::new() });
    Ok(envelope(
        id,
        "folder",
        Some(&cwd),
        dry_run,
        &plan,
        result,
        json!({"changes": changes, "conflicts": outcome.conflicts, "ledger": outcome.ledger}),
    ))
}

/// `plugins disable|enable <id>`: Claude Code's own command at user scope.
pub fn user_switch(env: &Env, runner: &dyn ClaudeRunner, op: UserOp, id: &str, dry_run: bool) -> Result<Value, OpError> {
    let plugin = installed_plugin(env, id)?;
    let id = plugin.id.as_str();
    let verb = op.verb();
    let command = format!("claude plugin {verb} {id} --scope user");
    if !runner.present() {
        return Err(OpError::conflict(format!("{NOT_FOUND}: `{command}` needs it")));
    }
    let state = env.layers(None).enabled(id);
    let shown = report::show(env, None, id, &Opts::default()).map_err(host_failure)?;
    let brings = brings_text(&brings_of(&shown));

    let mut warnings = Vec::new();
    match op {
        UserOp::Disable => {
            warnings.push(format!(
                "This turns {id} off for every project on this machine and for every folder that has no setting of its own; a folder or project that sets it itself keeps its own value"
            ));
            if state.user == Some(false) {
                warnings.push("already off in your user settings; running it again changes nothing".into());
            }
        }
        UserOp::Enable => {
            warnings.push(format!(
                "This turns {id} on at user scope for every project and every folder that has no setting of its own; a folder that was turned off with `plugins off` stays off"
            ));
            if state.user == Some(true) {
                warnings.push("already on in your user settings; running it again changes nothing".into());
            }
        }
    }
    warnings.extend(managed_warning(id, state.managed));
    warnings.push(RESTART.to_string());

    let steps = vec![
        Step { op: "exec", path: None, detail: command.clone(), keys: None, diff: None },
        note(match op {
            UserOp::Disable => format!("goes away everywhere it is not set on by a folder or project: {brings}"),
            UserOp::Enable => format!("comes back everywhere it is not set off by a folder or project: {brings}"),
        }),
        note(format!("stays: {STAYS}")),
    ];
    let undo = format!("toolportctl plugins {} {id}", op.other());
    let plan = PlanV1 {
        summary: format!("{} {id} for every project (user scope)", if op == UserOp::Disable { "Disable" } else { "Enable" }),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let mut result = None;
    if !dry_run {
        let out = runner
            .run(&["plugin", verb, id, "--scope", "user"], CLAUDE_TIMEOUT)
            .map_err(|e| OpError::conflict(if e == NOT_FOUND { format!("{NOT_FOUND}: `{command}` needs it") } else { e }))?;
        if !out.ok() {
            let line = out.first_error_line();
            return Err(OpError::conflict(if line.is_empty() {
                format!("claude plugin {verb} exited {}", out.code)
            } else {
                line
            }));
        }
        result = Some(ResultV1 { applied: true, changed: vec![command.clone()], undo, backups: Vec::new() });
    }
    let changes = vec![json!({"scope": "user", "action": verb, "command": ["claude", "plugin", verb, id, "--scope", "user"]})];
    Ok(envelope(id, "user", None, dry_run, &plan, result, json!({"changes": changes, "conflicts": []})))
}

/// `plugins off|on` on this machine: `--cwd` is required.
pub fn off_on(op: Switch, id: &str, cwd: Option<&str>, dry_run: bool) -> Result<Value, OpError> {
    let cwd = api::folder(cwd)?.ok_or_else(|| OpError::usage("cwd is required: a plugin is turned off or on per folder (user scope: plugins disable|enable)"))?;
    folder_switch(&api::env()?, op, id, &cwd, dry_run)
}

/// `plugins disable|enable` on this machine, through the real `claude` (or `TOOLPORT_CLAUDE_BIN`).
pub fn disable_enable(op: UserOp, id: &str, dry_run: bool) -> Result<Value, OpError> {
    user_switch(&api::env()?, &SystemClaude::from_env(), op, id, dry_run)
}
