//! `plugins disable|enable|update`: the three things Claude Code does itself for an installed
//! plugin, run as `claude plugin <verb> <id> --scope <scope>` after a plan. Nothing is written
//! by Toolport: the plan names the command, and `undo` names the command that puts it back.

use std::time::Duration;

use serde_json::{json, Value};

use super::claude::{ClaudeRunner, NOT_FOUND};
use super::detail;
use super::installed::{self, Installed};
use super::inventory::{layers, World};
use crate::context::plan::{Effects, PlanV1, ResultV1, Step};
use crate::context::plugin_config::resolve;
use crate::context::plugin_switch::RESTART;
use crate::op::OpError;

const TIMEOUT: Duration = Duration::from_secs(120);
const SCOPES: [&str; 4] = ["user", "project", "local", "managed"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Disable,
    Enable,
    Update,
}

impl Op {
    fn verb(self) -> &'static str {
        match self {
            Op::Disable => "disable",
            Op::Enable => "enable",
            Op::Update => "update",
        }
    }
}

pub struct Args<'a> {
    pub op: Op,
    pub id: &'a str,
    pub dry_run: bool,
}

fn exec_step(shown: &str) -> Step {
    Step {
        op: "exec",
        path: None,
        detail: shown.to_string(),
        keys: None,
        diff: None,
    }
}

fn note(detail: String) -> Step {
    Step {
        op: "note",
        path: None,
        detail,
        keys: None,
        diff: None,
    }
}

/// The scope Claude Code installed the plugin at; disable and enable always act on user scope.
fn scope_of(plugin: &Installed, op: Op) -> &'static str {
    if op != Op::Update {
        return "user";
    }
    SCOPES
        .into_iter()
        .find(|s| plugin.scope.as_deref() == Some(*s))
        .unwrap_or("user")
}

struct Decision {
    run: bool,
    note: String,
    from: Option<String>,
    to: Option<String>,
}

fn decide(world: &World, plugin: &Installed, op: Op) -> Result<Decision, OpError> {
    let layers = layers(world, None);
    let enabled = layers.enabled(&plugin.id);
    let id = &plugin.id;
    let skip = |note: String| Decision {
        run: false,
        note,
        from: plugin.version.clone(),
        to: None,
    };
    match op {
        Op::Disable | Op::Enable => {
            if enabled.managed.is_some() {
                return Err(OpError::conflict(format!(
                    "{id} is decided by your administrator's managed settings; it cannot be switched here"
                )));
            }
            let want = op == Op::Enable;
            Ok(if enabled.user == Some(want) {
                skip(format!(
                    "{id} is already {} in your user settings; nothing is run",
                    if want { "on" } else { "off" }
                ))
            } else {
                Decision {
                    run: true,
                    note: format!(
                        "your folder settings still apply on top: {}",
                        if want {
                            "a folder that turned the plugin off keeps it off"
                        } else {
                            "a folder that turned the plugin on keeps it on"
                        }
                    ),
                    from: None,
                    to: None,
                }
            })
        }
        Op::Update => {
            let state = detail::update(&world.claude_home, &layers, plugin);
            match state["state"].as_str() {
                Some("blocked") => Err(OpError::conflict(format!(
                    "{id} cannot be updated here: {}",
                    state["reason"].as_str().unwrap_or("blocked")
                ))),
                Some("current") => Ok(skip(format!("{id} is up to date; nothing is run"))),
                _ => Ok(Decision {
                    run: true,
                    note: match state["available"].as_str() {
                        Some(to) => format!("the marketplace offers {to}"),
                        None => "the marketplace clone does not say which version it offers".into(),
                    },
                    from: plugin.version.clone(),
                    to: state["available"].as_str().map(String::from),
                }),
            }
        }
    }
}

/// Plan, and unless `dry_run` run, one of the three commands.
pub fn run(
    world: &World,
    runner: Option<&dyn ClaudeRunner>,
    args: &Args,
) -> Result<Value, OpError> {
    let verb = args.op.verb();
    let runner = runner.filter(|r| r.present()).ok_or_else(|| {
        OpError::conflict(format!("{NOT_FOUND}: `claude plugin {verb}` needs it"))
    })?;
    let plugin = resolve(world, Some(runner), args.id)?;
    let decision = decide(world, &plugin, args.op)?;
    let scope = scope_of(&plugin, args.op);
    let shown = format!("claude plugin {verb} {} --scope {scope}", plugin.id);
    let undo = match args.op {
        Op::Disable => format!("toolportctl plugins enable {}", plugin.id),
        Op::Enable => format!("toolportctl plugins disable {}", plugin.id),
        Op::Update => format!(
            "no undo: the version before the update was {}",
            plugin.version.as_deref().unwrap_or("unknown")
        ),
    };
    let mut steps = Vec::new();
    if decision.run {
        steps.push(exec_step(&shown));
    }
    steps.push(note(decision.note.clone()));
    let mut warnings = Vec::new();
    if decision.run {
        warnings.push(RESTART.to_string());
    }
    let mut to = decision.to.clone();
    let mut changed = Vec::new();
    let mut applied = false;
    if decision.run && !args.dry_run {
        let out = runner
            .run(&["plugin", verb, &plugin.id, "--scope", scope], TIMEOUT)
            .map_err(OpError::conflict)?;
        if !out.ok() {
            return Err(OpError::conflict(installed::failure(&out, &shown)));
        }
        applied = true;
        changed.push(format!("{} ({scope} scope)", plugin.id));
        if args.op == Op::Update {
            let after = installed::list(Some(runner), &world.claude_home);
            to = after
                .plugins
                .iter()
                .find(|p| p.id == plugin.id)
                .and_then(|p| p.version.clone())
                .or(to);
        }
    }
    let past = match args.op {
        Op::Disable => "Disable",
        Op::Enable => "Enable",
        Op::Update => "Update",
    };
    let plan = PlanV1 {
        summary: format!("{past} {} ({scope} scope)", plugin.id),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = (!args.dry_run).then(|| ResultV1 {
        applied,
        changed: changed.clone(),
        undo,
        backups: Vec::new(),
    });
    Ok(json!({
        "id": plugin.id,
        "scope": scope,
        "cwd": null,
        "dryRun": args.dry_run,
        "plan": plan,
        "result": result,
        "changes": [{
            "scope": scope,
            "action": if decision.run { verb } else { "none" },
            "command": decision.run.then(|| shown.split(' ').map(String::from).collect::<Vec<_>>()),
            "from": decision.from,
            "to": to,
        }],
        "conflicts": Vec::<String>::new(),
        "ledger": null,
        "restartRequired": decision.run,
    }))
}
