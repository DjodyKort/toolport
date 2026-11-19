//! The optional weekly measure: the shipped task that runs `context measure` for one folder, so
//! the Spend page can show how what a session starts with drifts. Turning it on enables that task
//! (its schedule then runs it on its own), and the setup plan lists it as the third, optional step.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::transcript;
use crate::op::OpError;
use crate::tasks::api::{self as task_api, Source};
use crate::tasks::builtin::SPEND_WEEKLY_MEASURE;
use crate::tasks::model::{Step, Task};
use crate::tasks::{approval, store, triggers};

pub const TASK: &str = SPEND_WEEKLY_MEASURE;
const MODEL: &str = "haiku";
const MEASURE_STEP: &str = "measure";

pub const ACTION_ADD: &str = "add";
pub const ACTION_ON: &str = "already on";
pub const ACTION_FOLDER: &str = "needs a folder";
pub const ACTION_OFF: &str = "off";

fn load(host: &dyn Host) -> Result<Task, OpError> {
    let base = triggers::base_of(host)?;
    store::load_task(&base, TASK)
}

/// The folder operand of the measuring step: the argument after `measure`, unless a flag follows.
fn folder_of(task: &Task) -> Option<String> {
    task.steps.iter().find_map(|step| match step {
        Step::Exec { id, args, .. } if id == MEASURE_STEP => {
            let at = args.iter().position(|a| a == "measure")?;
            args.get(at + 1).filter(|a| !a.starts_with("--")).cloned()
        }
        _ => None,
    })
}

fn with_folder(mut task: Task, folder: &str) -> Task {
    for step in &mut task.steps {
        if let Step::Exec { id, args, .. } = step {
            if id != MEASURE_STEP {
                continue;
            }
            let Some(at) = args.iter().position(|a| a == "measure") else {
                continue;
            };
            if args.get(at + 1).is_some_and(|a| !a.starts_with("--")) {
                args[at + 1] = folder.to_string();
            } else {
                args.insert(at + 1, folder.to_string());
            }
        }
    }
    task
}

/// Where the task stands, for the status and the settings row.
pub fn state(host: &dyn Host) -> Value {
    let base = triggers::base_of(host).ok();
    let exists = base
        .as_ref()
        .and_then(|b| store::task_path(b, TASK).ok())
        .is_some_and(|path| path.is_file());
    let task = load(host).ok();
    json!({
        "task": TASK,
        "exists": exists,
        "enabled": task.as_ref().is_some_and(|t| t.enabled),
        "folder": task.as_ref().and_then(folder_of),
        "cron": task
            .as_ref()
            .and_then(|t| t.triggers.schedule.as_ref())
            .map(|s| s.cron.clone()),
        "model": MODEL,
    })
}

/// The folder the transcripts were written in most often; a starting point, never a decision.
fn busiest_folder(roots: &[PathBuf]) -> Option<String> {
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for session in transcript::scan(roots).into_values() {
        if let Some(cwd) = session.cwd.filter(|c| Path::new(c).is_dir()) {
            *count.entry(cwd).or_default() += 1;
        }
    }
    count
        .into_iter()
        .max_by_key(|(folder, n)| (*n, std::cmp::Reverse(folder.clone())))
        .map(|(folder, _)| folder)
}

fn checked(folder: &str) -> Result<&str, OpError> {
    if Path::new(folder).is_absolute() && Path::new(folder).is_dir() {
        Ok(folder)
    } else {
        Err(OpError::usage(format!(
            "--measure-folder: {folder:?} is not an existing folder; give its full path"
        )))
    }
}

/// The third step of the setup plan. `requested` is whether this plan asks for the task.
pub fn plan(
    host: &dyn Host,
    requested: bool,
    folder: Option<&str>,
    roots: &[PathBuf],
) -> Result<Value, OpError> {
    let mut out = state(host);
    let asked = folder.map(checked).transpose()?;
    let current = out["folder"].as_str().map(str::to_string);
    let chosen = asked
        .map(str::to_string)
        .or_else(|| current.clone())
        .or_else(|| busiest_folder(roots));
    let enabled = out["enabled"] == true;
    out["action"] = json!(match (&chosen, requested) {
        (_, false) => ACTION_OFF,
        (None, true) => ACTION_FOLDER,
        (Some(f), true) if enabled && current.as_deref() == Some(f.as_str()) => ACTION_ON,
        (Some(_), true) => ACTION_ADD,
    });
    out["requested"] = json!(requested);
    out["folder"] = json!(chosen);
    Ok(out)
}

/// Turns the task on for `folder` (or the folder it already has) or off. `ask_broker` makes
/// turning it on wait for the person's approval, which is what a self-MCP caller needs; the
/// command line and the app already have the person in front of the plan.
pub fn set(
    host: &dyn Host,
    on: bool,
    folder: Option<&str>,
    ask_broker: bool,
    dry_run: bool,
) -> Result<Value, OpError> {
    if !on {
        let off = task_api::set_enabled(host, TASK, false, dry_run)?;
        return Ok(
            json!({"task": TASK, "enabled": false, "applied": !dry_run, "plan": off["plan"]}),
        );
    }
    let task = load(host)?;
    let wanted = match folder {
        Some(f) => checked(f)?.to_string(),
        None => folder_of(&task).ok_or_else(|| {
            OpError::usage(
                "the weekly measure needs one folder to measure: pass --measure-folder <folder>",
            )
        })?,
    };
    let edit = if folder_of(&task).as_deref() == Some(wanted.as_str()) {
        Value::Null
    } else {
        let text = serde_json::to_string(&with_folder(task, &wanted)).unwrap_or_default();
        task_api::edit_with(host, TASK, Source::Stdin(&text), dry_run)?["plan"].clone()
    };
    let enable = if ask_broker {
        approval::set_enabled(TASK, true, dry_run, host)?
    } else {
        task_api::set_enabled(host, TASK, true, dry_run)?
    };
    Ok(json!({
        "task": TASK,
        "enabled": true,
        "folder": wanted,
        "applied": !dry_run,
        "plan": {"edit": edit, "enable": enable["plan"]},
    }))
}
