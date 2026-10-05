//! `toolportctl task ...`: named, repeatable jobs (D-067). Thin renderers over
//! `plus::tasks::api`, which the `tasks_*` tools call as well. `task __run <run-id>` is the hidden
//! runner every start spawns as a child process; it is not a registry command.

use super::context_bundle::plan_text;
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::tasks::api::{self, Source};
use crate::plus::tasks::host::RealHost;
use crate::plus::tasks::model::WaitKind;
use crate::plus::tasks::{runner, store};
use serde_json::Value;
use std::time::Duration;

const GROUP_USAGE: &str = "usage: task ls|show|run|resume|cancel|add|edit|rm|history (ls: [--all]; show|rm: <id>; run: <id> [--dry-run] [--wait] [--yes]; resume|cancel: <run-id>; add: <id> (--file <task.json> | --from-command <path>) [--dry-run]; edit: <id> --file <task.json> [--dry-run]; history: [<id>] [--run <run-id>] [--limit <n>])";
const SHOW_USAGE: &str = "usage: task show <id>";
const RUN_USAGE: &str = "usage: task run <id> [--dry-run] [--wait] [--yes]";
const RESUME_USAGE: &str = "usage: task resume <run-id>";
const CANCEL_USAGE: &str = "usage: task cancel <run-id>";
const ADD_USAGE: &str = "usage: task add <id> (--file <task.json> | --from-command <path>) [--dry-run]";
const EDIT_USAGE: &str = "usage: task edit <id> --file <task.json> [--dry-run]";
const RM_USAGE: &str = "usage: task rm <id> [--dry-run]";
const HISTORY_USAGE: &str = "usage: task history [<id>] [--run <run-id>] [--limit <n>]";

pub(super) const LS: Spec = Spec {
    flags: &[switch("--all")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const SHOW: Spec = Spec {
    flags: &[],
    operands: Operands::Max(1, SHOW_USAGE),
    ..LS
};

pub(super) const RUN: Spec = Spec {
    flags: &[switch("--dry-run"), switch("--wait"), switch("--yes")],
    operands: Operands::Max(1, RUN_USAGE),
    ..LS
};

pub(super) const RESUME: Spec = Spec {
    flags: &[],
    operands: Operands::Max(1, RESUME_USAGE),
    ..LS
};

pub(super) const CANCEL: Spec = Spec {
    operands: Operands::Max(1, CANCEL_USAGE),
    ..RESUME
};

pub(super) const ADD: Spec = Spec {
    flags: &[value("--file"), value("--from-command"), switch("--dry-run")],
    operands: Operands::Max(1, ADD_USAGE),
    ..LS
};

pub(super) const EDIT: Spec = Spec {
    flags: &[value("--file"), switch("--dry-run")],
    operands: Operands::Max(1, EDIT_USAGE),
    ..LS
};

pub(super) const RM: Spec = Spec {
    flags: &[switch("--dry-run")],
    operands: Operands::Max(1, RM_USAGE),
    ..LS
};

pub(super) const HISTORY: Spec = Spec {
    flags: &[value("--run"), value("--limit")],
    operands: Operands::Max(1, HISTORY_USAGE),
    ..LS
};

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

fn run_line(run: &Value) -> String {
    let duration = run["durationMs"].as_u64().map(|ms| format!("  {:.1}s", ms as f64 / 1000.0)).unwrap_or_default();
    let error = run["error"].as_str().map(|e| format!("  ({e})")).unwrap_or_default();
    format!("{:<34} {:<20} {:<9} {:<10}{duration}{error}\n", text(run, "id"), text(run, "task"), text(run, "status"), text(run, "trigger"))
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let data = api::ls(flags.on("--all"))?;
    let mut human = String::new();
    for t in data["tasks"].as_array().into_iter().flatten() {
        let last = match t["lastRun"]["status"].as_str() {
            Some(s) => format!("last {s}"),
            None => "never run".to_string(),
        };
        let waiting = if t["waiting"] == Value::Bool(true) { "  WAITING FOR YOU" } else { "" };
        let next = t["nextRun"].as_str().map(|n| format!("  next {n}")).unwrap_or_default();
        let off = if t["enabled"] == Value::Bool(true) { "" } else { "  (disabled)" };
        human.push_str(&format!("{:<24} {}  {last}{next}{waiting}{off}\n", text(t, "id"), text(t, "title")));
    }
    for t in data["invalid"].as_array().into_iter().flatten() {
        human.push_str(&format!("{:<24} unreadable: {}\n", text(t, "id"), text(t, "error")));
    }
    if human.is_empty() {
        human.push_str("no tasks (task add <id> --file <task.json>)\n");
    }
    Ok(Output::new(data, human))
}

pub fn show(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SHOW.parse(rest)?;
    let data = api::show(flags.single(SHOW_USAGE)?)?;
    let t = &data["task"];
    let mut human = format!("{}  {}{}\n", text(t, "id"), text(t, "title"), if t["enabled"] == Value::Bool(true) { "" } else { "  (disabled)" });
    if !text(t, "description").is_empty() {
        human.push_str(&format!("{}\n", text(t, "description")));
    }
    for (i, s) in t["steps"].as_array().into_iter().flatten().enumerate() {
        human.push_str(&format!("  {}. {} ({})\n", i + 1, text(s, "title"), text(s, "type")));
    }
    for s in t["writesSecrets"].as_array().into_iter().flatten() {
        human.push_str(&format!("  may write the secret {}/{}\n", text(s, "server"), text(s, "key")));
    }
    if !data["runs"].as_array().is_none_or(Vec::is_empty) {
        human.push_str("last runs:\n");
        for r in data["runs"].as_array().into_iter().flatten() {
            human.push_str(&format!("  {}", run_line(r)));
        }
    }
    Ok(Output::new(data, human))
}

pub fn history(rest: &[String]) -> Result<Output, CtlError> {
    let flags = HISTORY.parse(rest)?;
    let task = flags.operands().first().map(String::as_str);
    let data = api::history(task, flags.one("--run"), flags.number::<usize>("--limit")?)?;
    let mut human = String::new();
    if data["run"].is_object() {
        let r = &data["run"];
        human.push_str(&run_line(r));
        for s in r["steps"].as_array().into_iter().flatten() {
            human.push_str(&format!("  [{}] {} ({})\n", text(s, "status"), text(s, "title"), text(s, "type")));
            for line in text(s, "output").lines() {
                human.push_str(&format!("      {line}\n"));
            }
        }
    } else {
        for r in data["runs"].as_array().into_iter().flatten() {
            human.push_str(&run_line(r));
        }
        if human.is_empty() {
            human.push_str("no runs yet\n");
        }
    }
    Ok(Output::new(data, human))
}

fn step_lines(run: &store::Run, shown: &mut Vec<store::Status>, run_id: &str) {
    let total = run.steps.len();
    for (i, s) in run.steps.iter().enumerate() {
        if shown.get(i) == Some(&s.status) {
            continue;
        }
        if shown.len() <= i {
            shown.resize(i + 1, store::Status::Pending);
        }
        shown[i] = s.status;
        let status = serde_json::to_value(s.status).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
        match s.status {
            store::Status::Pending => {}
            store::Status::Waiting => {
                eprintln!("[{}/{total}] {}: waiting for you", i + 1, s.title);
                if let Some(text) = &s.instructions {
                    for line in text.lines() {
                        eprintln!("      {line}");
                    }
                }
                eprintln!("      continue with: toolportctl task resume {run_id}");
            }
            _ => {
                let first = s.output.lines().next().map(|l| format!(": {l}")).unwrap_or_default();
                eprintln!("[{}/{total}] {}: {status}{first}", i + 1, s.title);
            }
        }
    }
}

fn wait(run: store::Run) -> Result<store::Run, CtlError> {
    let task = store::load_task(&run.task)?;
    let mut shown = Vec::new();
    let mut run = run;
    loop {
        step_lines(&run, &mut shown, &run.id.clone());
        if run.status.finished() {
            return Ok(run);
        }
        let resume_only = api::step_wait_kind(&task, &run).is_some_and(|k| k.is_none_or(|k| matches!(k, WaitKind::Manual | WaitKind::Url)));
        if run.status == store::Status::Waiting && resume_only {
            return Ok(run);
        }
        std::thread::sleep(Duration::from_millis(200));
        run = store::load_run(&run.id)?;
    }
}

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let flags = RUN.parse(rest)?;
    let dry = flags.on("--dry-run");
    let mut data = api::run(flags.single(RUN_USAGE)?, "manual", dry, &RealHost)?;
    if dry {
        let human = plan_text(&data);
        return Ok(Output::new(data, human));
    }
    let id = text(&data["run"], "id").to_string();
    let mut failed = false;
    let mut human = format!("started run {id}\n");
    if flags.on("--wait") {
        let done = wait(store::load_run(&id)?)?;
        failed = done.status == store::Status::Failed;
        data["run"] = api::run_view(&done, false);
        let status = text(&data["run"], "status").to_string();
        human = format!("run {id} {status}{}\n", if done.status == store::Status::Waiting { format!(": continue with toolportctl task resume {id}") } else { String::new() });
    }
    let mut out = Output::new(data, human);
    out.failed = failed;
    Ok(out)
}

fn run_state(data: Value, verb: &str) -> Output {
    let human = format!("run {} {verb}: {}\n", text(&data["run"], "id"), text(&data["run"], "status"));
    Output::new(data, human)
}

pub fn resume(rest: &[String]) -> Result<Output, CtlError> {
    let flags = RESUME.parse(rest)?;
    Ok(run_state(api::resume(flags.single(RESUME_USAGE)?)?, "resumed"))
}

pub fn cancel(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CANCEL.parse(rest)?;
    Ok(run_state(api::cancel(flags.single(CANCEL_USAGE)?)?, "cancelled"))
}

pub fn add(rest: &[String]) -> Result<Output, CtlError> {
    let flags = ADD.parse(rest)?;
    let id = flags.single(ADD_USAGE)?;
    let source = match (flags.one("--file"), flags.one("--from-command")) {
        (Some(f), None) => Source::File(f),
        (None, Some(c)) => Source::Command(c),
        _ => return Err(CtlError::usage(ADD_USAGE)),
    };
    let data = api::add(id, source, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn edit(rest: &[String]) -> Result<Output, CtlError> {
    let flags = EDIT.parse(rest)?;
    let id = flags.single(EDIT_USAGE)?;
    let file = flags.one("--file").ok_or_else(|| CtlError::usage(EDIT_USAGE))?;
    let data = api::edit(id, file, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn rm(rest: &[String]) -> Result<Output, CtlError> {
    let flags = RM.parse(rest)?;
    let data = api::rm(flags.single(RM_USAGE)?, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn is_runner(positional: &[String]) -> bool {
    matches!(positional, [a, b, _] if a == "task" && b == "__run")
}

pub fn runner_main(positional: &[String]) -> i32 {
    match runner::execute(&positional[2], &RealHost) {
        Ok(run) if run.status == store::Status::Failed => 1,
        Ok(_) => 0,
        Err(e) => {
            eprintln!("toolportctl: {}", e.message);
            1
        }
    }
}
