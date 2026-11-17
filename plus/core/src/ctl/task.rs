//! `toolportctl task ...`: named, repeatable jobs (D-067). Thin renderers over
//! `plus::tasks::api`, which the `tasks_*` tools call as well. `task __run <run-id>` is the hidden
//! runner every start spawns as a child process; it is not a registry command.

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::tasks::api::{self, Source};
use crate::tasks::model::WaitKind;
use crate::tasks::{runner, store};
use serde_json::Value;
use std::io::Read;
use std::time::Duration;
use toolport_plus_api::Host;

const SHOW_USAGE: &str = "usage: task show <id>";
const RUN_USAGE: &str = "usage: task run <id> [--dry-run] [--wait] [--yes]";
const RESUME_USAGE: &str = "usage: task resume <run-id>";
const CANCEL_USAGE: &str = "usage: task cancel <run-id>";
const ADD_USAGE: &str =
    "usage: task add <id> (--file <task.json|-> | --from-command <path>) [--dry-run]";
const EDIT_USAGE: &str = "usage: task edit <id> --file <task.json|-> [--dry-run]";
const RM_USAGE: &str = "usage: task rm <id> [--dry-run]";
const ENABLE_USAGE: &str = "usage: task enable <id> [--dry-run]";
const DISABLE_USAGE: &str = "usage: task disable <id> [--dry-run]";
const MISSED_USAGE: &str = "usage: task missed [<id>]";
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
    flags: &[
        value("--file"),
        value("--from-command"),
        switch("--dry-run"),
    ],
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

pub(super) const ENABLE: Spec = Spec {
    flags: &[switch("--dry-run")],
    operands: Operands::Max(1, ENABLE_USAGE),
    ..LS
};

pub(super) const DISABLE: Spec = Spec {
    operands: Operands::Max(1, DISABLE_USAGE),
    ..ENABLE
};

pub(super) const MISSED: Spec = Spec {
    flags: &[],
    operands: Operands::Max(1, MISSED_USAGE),
    ..LS
};

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

fn run_line(run: &Value) -> String {
    let duration = run["durationMs"]
        .as_u64()
        .map(|ms| format!("  {:.1}s", ms as f64 / 1000.0))
        .unwrap_or_default();
    let error = run["error"]
        .as_str()
        .map(|e| format!("  ({e})"))
        .unwrap_or_default();
    format!(
        "{:<34} {:<20} {:<9} {:<10}{duration}{error}\n",
        text(run, "id"),
        text(run, "task"),
        text(run, "status"),
        text(run, "trigger")
    )
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let data = api::ls(host, flags.on("--all"))?;
    let mut human = String::new();
    for t in data["tasks"].as_array().into_iter().flatten() {
        let last = match t["lastRun"]["status"].as_str() {
            Some(s) => format!("last {s}"),
            None => "never run".to_string(),
        };
        let waiting = if t["waiting"] == Value::Bool(true) {
            "  WAITING FOR YOU"
        } else {
            ""
        };
        let next = t["nextRun"]
            .as_str()
            .map(|n| format!("  next {n}"))
            .unwrap_or_default();
        let off = if t["enabled"] == Value::Bool(true) {
            ""
        } else {
            "  (disabled)"
        };
        human.push_str(&format!(
            "{:<24} {}  {last}{next}{waiting}{off}\n",
            text(t, "id"),
            text(t, "title")
        ));
    }
    for t in data["invalid"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{:<24} unreadable: {}\n",
            text(t, "id"),
            text(t, "error")
        ));
    }
    if human.is_empty() {
        let disabled = if flags.on("--all") {
            0
        } else {
            api::ls(host, true)?["tasks"].as_array().map_or(0, Vec::len)
        };
        human.push_str(&match disabled {
            0 => "no tasks (task add <id> --file <task.json>)\n".to_string(),
            n => format!("no enabled tasks ({n} disabled, see `task ls --all`)\n"),
        });
    }
    Ok(Output::new(data, human))
}

pub fn show(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SHOW.parse(rest)?;
    let data = api::show(host, flags.single(SHOW_USAGE)?)?;
    let t = &data["task"];
    let mut human = format!(
        "{}  {}{}\n",
        text(t, "id"),
        text(t, "title"),
        if t["enabled"] == Value::Bool(true) {
            ""
        } else {
            "  (disabled)"
        }
    );
    if !text(t, "description").is_empty() {
        human.push_str(&format!("{}\n", text(t, "description")));
    }
    for (i, s) in t["steps"].as_array().into_iter().flatten().enumerate() {
        human.push_str(&format!(
            "  {}. {} ({})\n",
            i + 1,
            text(s, "title"),
            text(s, "type")
        ));
    }
    for s in t["writesSecrets"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "  may write the secret {}/{}\n",
            text(s, "server"),
            text(s, "key")
        ));
    }
    if !data["runs"].as_array().is_none_or(Vec::is_empty) {
        human.push_str("last runs:\n");
        for r in data["runs"].as_array().into_iter().flatten() {
            human.push_str(&format!("  {}", run_line(r)));
        }
    }
    Ok(Output::new(data, human))
}

pub fn history(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = HISTORY.parse(rest)?;
    let task = flags.operands().first().map(String::as_str);
    let data = api::history(
        host,
        task,
        flags.one("--run"),
        flags.number::<usize>("--limit")?,
    )?;
    let mut human = String::new();
    if data["run"].is_object() {
        let r = &data["run"];
        human.push_str(&run_line(r));
        for s in r["steps"].as_array().into_iter().flatten() {
            human.push_str(&format!(
                "  [{}] {} ({})\n",
                text(s, "status"),
                text(s, "title"),
                text(s, "type")
            ));
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

pub fn missed(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = MISSED.parse(rest)?;
    let task = flags.operands().first().map(String::as_str);
    let data = api::missed(host, task)?;
    let mut human = String::new();
    for m in data["missed"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{:<24} {:<10} {} slot(s), first due {}, last due {}\n",
            text(m, "task"),
            text(m, "outcome"),
            m["slots"],
            text(m, "firstDue"),
            text(m, "lastDue")
        ));
    }
    if human.is_empty() {
        human.push_str("no missed slots\n");
    }
    Ok(Output::new(data, human))
}

fn set_enabled(
    host: &dyn Host,
    rest: &[String],
    spec: &Spec,
    usage: &str,
    enabled: bool,
) -> Result<Output, CtlError> {
    let flags = spec.parse(rest)?;
    let data = api::set_enabled(host, flags.single(usage)?, enabled, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn enable(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    set_enabled(host, rest, &ENABLE, ENABLE_USAGE, true)
}

pub fn disable(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    set_enabled(host, rest, &DISABLE, DISABLE_USAGE, false)
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
        let status = serde_json::to_value(s.status)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
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
                let first = s
                    .output
                    .lines()
                    .next()
                    .map(|l| format!(": {l}"))
                    .unwrap_or_default();
                eprintln!("[{}/{total}] {}: {status}{first}", i + 1, s.title);
            }
        }
    }
}

fn base(host: &dyn Host) -> Result<std::path::PathBuf, CtlError> {
    crate::tasks::triggers::base_of(host)
}

fn wait(base: &std::path::Path, run: store::Run) -> Result<store::Run, CtlError> {
    let task = store::load_task(base, &run.task)?;
    let mut shown = Vec::new();
    let mut run = run;
    loop {
        step_lines(&run, &mut shown, &run.id.clone());
        if run.status.finished() {
            return Ok(run);
        }
        let resume_only = api::step_wait_kind(&task, &run)
            .is_some_and(|k| k.is_none_or(|k| matches!(k, WaitKind::Manual | WaitKind::Url)));
        if run.status == store::Status::Waiting && resume_only {
            return Ok(run);
        }
        std::thread::sleep(Duration::from_millis(200));
        run = store::load_run(base, &run.id)?;
    }
}

pub fn run(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RUN.parse(rest)?;
    let dry = flags.on("--dry-run");
    let mut data = api::run(flags.single(RUN_USAGE)?, "manual", dry, host)?;
    if dry {
        let human = plan_text(&data);
        return Ok(Output::new(data, human));
    }
    let id = text(&data["run"], "id").to_string();
    let mut failed = false;
    let mut human = format!("started run {id}\n");
    if flags.on("--wait") {
        let base = base(host)?;
        let done = wait(&base, store::load_run(&base, &id)?)?;
        failed = done.status == store::Status::Failed;
        data["run"] = api::run_view(&done, false);
        let status = text(&data["run"], "status").to_string();
        human = format!(
            "run {id} {status}{}\n",
            if done.status == store::Status::Waiting {
                format!(": continue with toolportctl task resume {id}")
            } else {
                String::new()
            }
        );
    }
    let mut out = Output::new(data, human);
    out.failed = failed;
    Ok(out)
}

fn run_state(data: Value, verb: &str) -> Output {
    let human = format!(
        "run {} {verb}: {}\n",
        text(&data["run"], "id"),
        text(&data["run"], "status")
    );
    Output::new(data, human)
}

pub fn resume(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RESUME.parse(rest)?;
    Ok(run_state(
        api::resume(host, flags.single(RESUME_USAGE)?)?,
        "resumed",
    ))
}

pub fn cancel(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CANCEL.parse(rest)?;
    Ok(run_state(
        api::cancel(host, flags.single(CANCEL_USAGE)?)?,
        "cancelled",
    ))
}

fn definition_source<'a>(
    file: &'a str,
    stdin: &'a mut Option<String>,
    input: &mut dyn Read,
) -> Result<Source<'a>, CtlError> {
    if file != "-" {
        return Ok(Source::File(file));
    }
    let mut text = String::new();
    input.read_to_string(&mut text).map_err(|e| {
        CtlError::failed(
            "stdin",
            format!("cannot read the definition from stdin: {e}"),
        )
    })?;
    if text.trim().is_empty() {
        return Err(CtlError::failed(
            "invalid_task",
            "nothing on stdin: pipe the task definition in (--file - < task.json)",
        ));
    }
    Ok(Source::Stdin(stdin.insert(text)))
}

pub fn add(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    add_from(host, rest, &mut std::io::stdin().lock())
}

pub fn add_from(
    host: &dyn Host,
    rest: &[String],
    input: &mut dyn Read,
) -> Result<Output, CtlError> {
    let flags = ADD.parse(rest)?;
    let id = flags.single(ADD_USAGE)?;
    let mut stdin = None;
    let source = match (flags.one("--file"), flags.one("--from-command")) {
        (Some(f), None) => definition_source(f, &mut stdin, input)?,
        (None, Some(c)) => Source::Command(c),
        _ => return Err(CtlError::usage(ADD_USAGE)),
    };
    let data = api::add(host, id, source, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn edit(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    edit_from(host, rest, &mut std::io::stdin().lock())
}

pub fn edit_from(
    host: &dyn Host,
    rest: &[String],
    input: &mut dyn Read,
) -> Result<Output, CtlError> {
    let flags = EDIT.parse(rest)?;
    let id = flags.single(EDIT_USAGE)?;
    let file = flags
        .one("--file")
        .ok_or_else(|| CtlError::usage(EDIT_USAGE))?;
    let mut stdin = None;
    let data = api::edit_with(
        host,
        id,
        definition_source(file, &mut stdin, input)?,
        flags.on("--dry-run"),
    )?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn rm(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RM.parse(rest)?;
    let data = api::rm(host, flags.single(RM_USAGE)?, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn is_runner(positional: &[String]) -> bool {
    matches!(positional, [a, b, _] if a == "task" && b == "__run")
}

pub fn runner_main(host: &dyn Host, positional: &[String]) -> i32 {
    let Ok(base) = base(host) else {
        eprintln!("toolportctl: the data directory could not be resolved");
        return 1;
    };
    match runner::execute(host, &base, &positional[2]) {
        Ok(run) if run.status == store::Status::Failed => 1,
        Ok(_) => 0,
        Err(e) => {
            eprintln!("toolportctl: {}", e.message);
            1
        }
    }
}

fn plan_text(data: &Value) -> String {
    let plan = &data["plan"];
    let mut human = format!("{}\n", plan["summary"].as_str().unwrap_or(""));
    for step in plan["steps"].as_array().into_iter().flatten() {
        let path = step["path"]
            .as_str()
            .map(|p| format!("  {p}"))
            .unwrap_or_default();
        human.push_str(&format!(
            "  {:<7} {}{path}\n",
            step["op"].as_str().unwrap_or(""),
            step["detail"].as_str().unwrap_or("")
        ));
    }
    for warning in plan["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        human.push_str(&format!("warning: {warning}\n"));
    }
    if data["dryRun"] == Value::Bool(true) {
        human.push_str("(dry run, nothing written)\n");
    } else if let Some(undo) = data["result"]["undo"].as_str() {
        human.push_str(&format!("undo: {undo}\n"));
    }
    human
}
