//! `plugins off|on`: `enabledPlugins.<id> = false` in one folder's `settings.local.json`, through
//! the controls ledger (`off:<id>`), so every other key stays byte-identical and `on` removes only
//! what Toolport wrote. The user-level switch is Claude Code's own `claude plugin disable`.

use super::bundle_apply::{folder_key, Desired};
use super::bundle_controls::{self as controls, Run};
use super::bundle_ledger as ledger;
use super::bundle_store::op as bundle_op;
use super::fsx;
use super::plan::{Effects, PlanV1, ResultV1, Step};
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const SETTINGS_CAP: usize = 4 * 1024 * 1024;
pub const RESTART: &str = "Claude Code reads plugin settings when a session starts: sessions that are already running keep the old state until they restart";
const STAYS: &str = "your own skills, agents, commands and hooks, every other plugin and the servers behind the Toolport gateway";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Off,
    On,
}

pub struct Args<'a> {
    pub op: Switch,
    pub id: &'a str,
    pub cwd: &'a Path,
    pub dry_run: bool,
}

pub struct World<'a> {
    pub claude_home: &'a Path,
    pub data_dir: &'a Path,
}

pub fn control_name(id: &str) -> String {
    format!("off:{id}")
}

/// The plugins `plugins off` switched off in a folder whose entry still holds what Toolport wrote.
pub fn switched_off(data_dir: &Path, cwd: &Path) -> Vec<String> {
    let cwd = fsx::canonical(cwd);
    let all = ledger::load(data_dir);
    let Some(mine) = all.controls.get(&folder_key(&cwd)) else {
        return Vec::new();
    };
    mine.keys()
        .filter_map(|control| control.strip_prefix("off:"))
        .filter(|id| controls::drifted(data_dir, &cwd, &control_name(id)).is_empty())
        .map(String::from)
        .collect()
}

fn valid_id(id: &str) -> bool {
    super::plugin_adapters::valid_ident(id)
}

pub(super) fn quote(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./-:@%+=,~*".contains(c))
    {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

fn enabled_in(file: &Path, id: &str) -> Option<bool> {
    let text = fsx::read_text(file, SETTINGS_CAP)?;
    serde_json::from_str::<Value>(&text)
        .ok()?
        .get("enabledPlugins")?
        .get(id)?
        .as_bool()
}

fn state_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "not set",
    }
}

/// `Some(false)` when the install list is readable and does not hold the plugin.
fn installed(claude_home: &Path, id: &str) -> Option<bool> {
    let text = fsx::read_text(
        &claude_home.join("plugins/installed_plugins.json"),
        SETTINGS_CAP,
    )?;
    let doc = serde_json::from_str::<Value>(&text).ok()?;
    Some(doc.get("plugins")?.get(id).is_some())
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

pub(super) fn note(detail: String) -> Step {
    Step {
        op: "note",
        path: None,
        detail,
        keys: None,
        diff: None,
    }
}

#[derive(Default)]
pub(super) struct Outcome {
    pub acted: bool,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
    pub conflicts: Vec<String>,
    pub changed: Vec<String>,
    pub ledger: Option<String>,
}

impl Outcome {
    pub(super) fn skipped(warning: String) -> Self {
        Self {
            warnings: vec![warning],
            ..Self::default()
        }
    }

    pub(super) fn from_run(run: Run) -> Self {
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

pub(super) fn folder(cwd: &Path) -> Result<PathBuf, OpError> {
    if !fsx::is_dir(cwd) {
        return Err(OpError::usage(format!(
            "cwd is not a folder: {}",
            cwd.display()
        )));
    }
    Ok(fsx::canonical(cwd))
}

pub fn run(world: &World, args: &Args) -> Result<Value, OpError> {
    let id = args.id;
    if !valid_id(id) {
        return Err(OpError::usage(format!("invalid plugin id: {id}")));
    }
    let cwd = folder(args.cwd)?;
    let (data_dir, op) = (world.data_dir, args.op);
    let known = installed(world.claude_home, id);
    if op == Switch::Off && known == Some(false) {
        return Err(OpError::not_found(format!(
            "{id} is not among the installed plugins; `claude plugin list` shows the ids"
        )));
    }
    let control = control_name(id);
    let user = enabled_in(&world.claude_home.join("settings.json"), id);
    let project = enabled_in(&cwd.join(".claude/settings.json"), id);
    let elsewhere = project.or(user).unwrap_or(false);
    let record = controls::record(data_dir, &cwd, &control);
    let drift = controls::drifted(data_dir, &cwd, &control);
    let bundle = bundle_owner(data_dir, &cwd, id);
    let here = enabled_in(&cwd.join(".claude/settings.local.json"), id);
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
                Outcome::skipped(
                    "already turned off in this folder by `plugins off`; nothing is written".into(),
                )
            } else if let Some(name) = &bundle {
                Outcome::skipped(format!("already off in this folder by bundle {name}; `context bundle undo` puts it back, nothing is written"))
            } else if here == Some(false) {
                Outcome::skipped(format!("{label} is already false in this folder's settings.local.json and Toolport did not write it; it is left alone"))
            } else {
                let want = Desired {
                    plugins: vec![id.to_string()],
                    ..Desired::default()
                };
                Outcome::from_run(
                    controls::apply(data_dir, &cwd, &control, &want, args.dry_run)
                        .map_err(bundle_op)?,
                )
            }
        }
        Switch::On => {
            if record.is_some() {
                Outcome::from_run(
                    controls::undo(data_dir, &cwd, &control, args.dry_run).map_err(bundle_op)?,
                )
            } else if let Some(name) = &bundle {
                Outcome::skipped(format!("turned off by bundle {name}, not by `plugins off`; `context bundle undo` turns it back on"))
            } else if here == Some(false) {
                Outcome::skipped(format!("{label} is false in this folder's settings.local.json and Toolport did not write it; it is left alone, edit the file to turn the plugin on"))
            } else {
                Outcome::skipped(
                    "not turned off in this folder by `plugins off`; nothing to remove".into(),
                )
            }
        }
    };
    let mut warnings = std::mem::take(&mut outcome.warnings);
    if known.is_none() {
        warnings.push(format!(
            "the list of installed plugins could not be read, so {id} was not checked against it"
        ));
    }
    match op {
        Switch::Off if !elsewhere => warnings.push(format!(
            "{id} is not on in your user or project settings, so it is already off here; the entry only matters if one of them turns it on later"
        )),
        Switch::On if !elsewhere => warnings.push(format!(
            "{id} is not on in your user or project settings, so it stays off here after this"
        )),
        _ => {}
    }
    warnings.push(RESTART.to_string());

    let mut steps = std::mem::take(&mut outcome.steps);
    steps.push(note(format!(
        "stays: {STAYS}; the setting in your user settings ({}) is not changed",
        state_text(user)
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
    let owned = record.as_ref().and_then(|r| {
        r.settings
            .owned
            .iter()
            .find(|o| o.path == ["enabledPlugins", id])
    });
    let changes = vec![change_row(op, id, &outcome, owned)];
    let result = (!args.dry_run).then(|| ResultV1 {
        applied: true,
        changed: outcome.changed.clone(),
        undo,
        backups: Vec::new(),
    });
    Ok(json!({
        "id": id,
        "scope": "folder",
        "cwd": fsx::display(&cwd),
        "dryRun": args.dry_run,
        "plan": plan,
        "result": result,
        "changes": changes,
        "conflicts": outcome.conflicts,
        "ledger": outcome.ledger,
    }))
}
