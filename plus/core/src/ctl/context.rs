//! `toolportctl context bundle ...` and `context use`: context bundles (D-064, D-066). Thin
//! renderers over `context::ops`, which the `context_bundle_*` tools call as well.

use super::flags::{switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::context::bundle::Edit;
use crate::context::leave_out::{Op, Target};
use crate::context::ops as api;
use crate::op::ErrorKind;
use serde_json::Value;
use toolport_plus_api::Host;

const SHOW_USAGE: &str = "usage: context bundle show <name>";
const SAVE_USAGE: &str = "usage: context bundle add|edit <name> [--from-folder <dir>] [--description <t>] [--skills-off <csv>] [--skills-name-only <csv>] [--skills-allow <csv>] [--plugins-off <csv>] [--layers-add <csv>] [--layers-exclude <csv>] [--agents-off <csv>] [--servers <profile>] [--bind <csv>] [--dry-run]";
const RM_USAGE: &str = "usage: context bundle rm <name> [--force] [--dry-run]";
const APPLY_USAGE: &str = "usage: context bundle apply <name> [--cwd <dir>] [--dry-run]";
const LAUNCH_USAGE: &str = "usage: context bundle launch <name> [--cwd <dir>]";
const USE_USAGE: &str = "usage: context use <name> [--cwd <dir>] [--dry-run] | context use --none [--cwd <dir>] [--dry-run]";

pub(super) const LS: Spec = Spec {
    flags: &[],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const SHOW: Spec = Spec {
    operands: Operands::Max(1, SHOW_USAGE),
    ..LS
};

pub(super) const ADD: Spec = Spec {
    flags: &[
        value("--from-folder"),
        value("--description"),
        value("--skills-off"),
        value("--skills-name-only"),
        value("--skills-allow"),
        value("--plugins-off"),
        value("--layers-add"),
        value("--layers-exclude"),
        value("--agents-off"),
        value("--servers"),
        value("--bind"),
        switch("--dry-run"),
    ],
    operands: Operands::Max(1, SAVE_USAGE),
    ..LS
};

pub(super) const EDIT: Spec = Spec {
    flags: &[
        value("--description"),
        value("--skills-off"),
        value("--skills-name-only"),
        value("--skills-allow"),
        value("--plugins-off"),
        value("--layers-add"),
        value("--layers-exclude"),
        value("--agents-off"),
        value("--servers"),
        value("--bind"),
        switch("--dry-run"),
    ],
    ..ADD
};

pub(super) const RM: Spec = Spec {
    flags: &[switch("--force"), switch("--dry-run")],
    operands: Operands::Max(1, RM_USAGE),
    ..LS
};

pub(super) const APPLY: Spec = Spec {
    flags: &[value("--cwd"), switch("--dry-run")],
    operands: Operands::Max(1, APPLY_USAGE),
    ..LS
};

pub(super) const UNDO: Spec = Spec {
    operands: Operands::Reject,
    ..APPLY
};

pub(super) const STATUS: Spec = Spec {
    flags: &[value("--cwd")],
    operands: Operands::Reject,
    ..LS
};

pub(super) const LAUNCH: Spec = Spec {
    flags: &[value("--cwd")],
    operands: Operands::Max(1, LAUNCH_USAGE),
    ..LS
};

pub(super) const CONFIG: Spec = Spec {
    flags: &[value("--auto-apply")],
    ..LS
};

pub(super) const USE: Spec = Spec {
    flags: &[value("--cwd"), switch("--dry-run"), switch("--none")],
    operands: Operands::Max(1, USE_USAGE),
    ..LS
};

/// The shared operations name a bad argument (`cwd is not a folder`); the command line calls it
/// by its flag.
pub(super) fn flagged(err: CtlError) -> CtlError {
    if err.kind == ErrorKind::Usage && err.message.starts_with("cwd ") {
        CtlError::usage(format!("--{}", err.message))
    } else {
        err
    }
}

fn csv(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn edit_of(flags: &Flags) -> Edit {
    let list = |name: &str| flags.one(name).map(csv);
    Edit {
        description: flags.one("--description").map(String::from),
        servers: flags.one("--servers").map(String::from),
        skills_off: list("--skills-off"),
        skills_name_only: list("--skills-name-only"),
        skills_allow: list("--skills-allow"),
        plugins_off: list("--plugins-off"),
        layers_add: list("--layers-add"),
        layers_exclude: list("--layers-exclude"),
        agents_off: list("--agents-off"),
        bind: list("--bind"),
    }
}

fn texts(v: &Value) -> Vec<&str> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

pub(super) fn plan_text(data: &Value) -> String {
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
    if let (Some(before), Some(after)) = (
        plan["effects"]["tokens"]["before"].as_u64(),
        plan["effects"]["tokens"]["after"].as_u64(),
    ) {
        human.push_str(&format!(
            "  about {before} -> {after} tokens at start ({})\n",
            plan["effects"]["tokens"]["basis"]
                .as_str()
                .unwrap_or("estimate")
        ));
    }
    for warning in texts(&plan["warnings"]) {
        human.push_str(&format!("warning: {warning}\n"));
    }
    if data["dryRun"] == Value::Bool(true) {
        human.push_str("(dry run, nothing written)\n");
    } else if let Some(undo) = data["result"]["undo"].as_str() {
        human.push_str(&format!("undo: {undo}\n"));
    }
    let conflicts = texts(&data["conflicts"]);
    if !conflicts.is_empty() {
        human.push_str(&format!(
            "left as they are, changed since: {}\n",
            conflicts.join(", ")
        ));
    }
    human
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    LS.parse(rest)?;
    let data = api::ls(host)?;
    let mut human = String::new();
    for b in data["bundles"].as_array().into_iter().flatten() {
        let applied = b["appliedTo"].as_array().map_or(0, Vec::len);
        human.push_str(&format!(
            "{:<22} skills -{}/~{}/={}  plugins -{}  agents -{}  applied in {applied}{}\n",
            b["name"].as_str().unwrap_or(""),
            b["skills"]["off"],
            b["skills"]["nameOnly"],
            b["skills"]["allow"],
            texts(&b["plugins"]["off"]).len(),
            texts(&b["agents"]["off"]).len(),
            b["error"]
                .as_str()
                .map(|e| format!("  ({e})"))
                .unwrap_or_default(),
        ));
    }
    if human.is_empty() {
        human.push_str("no bundles (profiles/<name>.yaml in the skills repository)\n");
    }
    Ok(Output::new(data, human))
}

pub fn show(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SHOW.parse(rest)?;
    let data = api::show(host, flags.single(SHOW_USAGE)?)?;
    let mut human = format!(
        "{}\n{}",
        data["path"].as_str().unwrap_or(""),
        data["yaml"].as_str().unwrap_or("")
    );
    for issue in data["issues"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{}: {} {}\n",
            issue["level"].as_str().unwrap_or(""),
            issue["key"].as_str().unwrap_or(""),
            issue["message"].as_str().unwrap_or("")
        ));
    }
    Ok(Output::new(data, human))
}

fn save(host: &dyn Host, rest: &[String], spec: &Spec, create: bool) -> Result<Output, CtlError> {
    let flags = spec.parse(rest)?;
    let name = flags.single(SAVE_USAGE)?;
    let data = api::save(
        host,
        name,
        edit_of(&flags),
        flags.one("--from-folder"),
        create,
        flags.on("--dry-run"),
    )
    .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn add(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    save(host, rest, &ADD, true)
}

pub fn edit(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    save(host, rest, &EDIT, false)
}

pub fn rm(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RM.parse(rest)?;
    let data = api::rm(
        host,
        flags.single(RM_USAGE)?,
        flags.on("--force"),
        flags.on("--dry-run"),
    )?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn apply(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = APPLY.parse(rest)?;
    let data = api::apply(
        host,
        flags.single(APPLY_USAGE)?,
        flags.one("--cwd"),
        flags.on("--dry-run"),
    )
    .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNDO.parse(rest)?;
    let data = api::undo(host, flags.one("--cwd"), flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn context_status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CONTEXT_STATUS.parse(rest)?;
    let data = api::folder_status(host, flags.one("--cwd")).map_err(flagged)?;
    let folder = data["folder"].as_str().unwrap_or("");
    let human = match data["applied"].as_object() {
        None => format!("no bundle is applied in {folder}\n"),
        Some(a) => {
            let changed = texts(&a["changedKeys"]);
            format!(
                "bundle {} applied in {folder} at {}{}\n",
                a["bundle"].as_str().unwrap_or(""),
                a["appliedAt"].as_str().unwrap_or(""),
                if changed.is_empty() {
                    String::new()
                } else {
                    format!("; drift in {}", changed.join(", "))
                }
            )
        }
    };
    Ok(Output::new(data, human))
}

pub fn launch(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LAUNCH.parse(rest)?;
    let data =
        api::launch(host, flags.single(LAUNCH_USAGE)?, flags.one("--cwd")).map_err(flagged)?;
    let mut human = format!("{}\n", data["command"].as_str().unwrap_or(""));
    for note in texts(&data["notes"]) {
        human.push_str(&format!("note: {note}\n"));
    }
    Ok(Output::new(data, human))
}

pub fn config(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CONFIG.parse(rest)?;
    let auto = match flags.one("--auto-apply") {
        None => None,
        Some("on") => Some(true),
        Some("off") => Some(false),
        Some(_) => return Err(CtlError::usage("--auto-apply takes on or off")),
    };
    let data = api::config(host, auto)?;
    let human = format!(
        "auto-apply: {}\n",
        if data["autoApply"] == Value::Bool(true) {
            "on"
        } else {
            "off"
        }
    );
    Ok(Output::new(data, human))
}

pub fn use_bundle(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = USE.parse(rest)?;
    let none = flags.on("--none");
    let name = flags.operands().first().map(String::as_str);
    match (name, none) {
        (Some(_), true) => return Err(CtlError::usage("--none takes no name")),
        (None, false) => return Err(CtlError::usage(USE_USAGE)),
        _ => {}
    }
    let data = api::use_bundle(host, name, flags.one("--cwd"), none, flags.on("--dry-run"))
        .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

const LEAVE_OUT_USAGE: &str = "usage: context leave-out add|rm <path> --cwd <dir> [--dry-run] | add|rm --memory --cwd <dir> [--dry-run]";

pub(super) const LEAVE_OUT: Spec = Spec {
    flags: &[value("--cwd"), switch("--memory"), switch("--dry-run")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Max(2, LEAVE_OUT_USAGE),
    ..Spec::PLAIN
};

pub fn leave_out(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LEAVE_OUT.parse(rest)?;
    let (action, target) = match (flags.operands(), flags.on("--memory")) {
        ([action], true) => (action, Target::Memory),
        ([action, path], false) => (action, Target::File(path.clone())),
        _ => return Err(CtlError::usage(LEAVE_OUT_USAGE)),
    };
    let op = match action.as_str() {
        "add" => Op::Add,
        "rm" => Op::Rm,
        _ => return Err(CtlError::usage(LEAVE_OUT_USAGE)),
    };
    let data = api::leave_out(host, op, target, flags.one("--cwd"), flags.on("--dry-run"))
        .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

const LOADS_USAGE: &str = "usage: context loads [--cwd <dir>] [--measured]";
const STATUS_USAGE: &str = "usage: context status [--cwd <dir>]";
const COMPOSE_USAGE: &str = "usage: context compose [--cwd <dir>]";
const SYNC_USAGE: &str = "usage: context sync [--dry-run]";

pub(super) const LOADS: Spec = Spec {
    flags: &[value("--cwd"), switch("--measured")],
    operands: Operands::Max(0, LOADS_USAGE),
    ..LS
};

pub(super) const COMPOSE: Spec = Spec {
    flags: &[value("--cwd")],
    operands: Operands::Max(0, COMPOSE_USAGE),
    ..LS
};

pub(super) const CONTEXT_STATUS: Spec = Spec {
    flags: &[value("--cwd")],
    operands: Operands::Max(0, STATUS_USAGE),
    ..LS
};

pub(super) const SYNC: Spec = Spec {
    flags: &[switch("--dry-run")],
    operands: Operands::Max(0, SYNC_USAGE),
    ..LS
};

pub fn loads(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LOADS.parse(rest)?;
    let data = api::loads(host, flags.one("--cwd"), flags.on("--measured")).map_err(flagged)?;
    let mut human = String::new();
    for item in data["items"].as_array().into_iter().flatten() {
        let state = match (item["loaded"].as_bool(), item["lazy"].as_bool()) {
            (Some(true), _) => "loads",
            (_, Some(true)) => "on demand",
            _ => "left out",
        };
        human.push_str(&format!(
            "{:<10} {:>7}  {:<8} {}\n",
            state,
            item["tokens"],
            item["kind"].as_str().unwrap_or(""),
            item["path"].as_str().unwrap_or(""),
        ));
    }
    human.push_str(&format!(
        "about {} tokens in the rows above ({}); {}\n",
        data["totalTokens"],
        data["tokenizer"].as_str().unwrap_or(""),
        data["notItemised"].as_str().unwrap_or("")
    ));
    Ok(Output::new(data, human))
}

pub fn status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    let data = api::status(host, flags.one("--cwd")).map_err(flagged)?;
    let mut human = format!("{}\n", data["cwd"].as_str().unwrap_or(""));
    human.push_str(&format!(
        "  bundle: {}\n",
        data["bundle"]["applied"]["bundle"]
            .as_str()
            .unwrap_or("none")
    ));
    human.push_str(&format!(
        "  left out: {} file(s){}\n",
        data["leftOut"]["files"].as_array().map_or(0, Vec::len),
        if data["leftOut"]["memoryOff"] == Value::Bool(true) {
            ", auto memory off"
        } else {
            ""
        }
    ));
    human.push_str(&format!(
        "  plugins off: {}\n",
        data["pluginsOff"].as_array().map_or(0, Vec::len)
    ));
    human.push_str(&format!(
        "  estimate: about {} tokens in the instruction rows\n",
        data["estimate"]["totalTokens"]
    ));
    match data["measured"]["total"].as_u64() {
        Some(total) => human.push_str(&format!("  measured: {total} tokens\n")),
        None => human.push_str("  measured: not yet\n"),
    }
    Ok(Output::new(data, human))
}

pub fn compose(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = COMPOSE.parse(rest)?;
    let data = api::compose(host, flags.one("--cwd")).map_err(flagged)?;
    let mut human = String::new();
    for part in data["parts"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "# {}\n{}\n",
            part["path"].as_str().unwrap_or(""),
            part["text"].as_str().unwrap_or("")
        ));
    }
    Ok(Output::new(data, human))
}

pub fn sync(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SYNC.parse(rest)?;
    let data = api::sync(host, flags.on("--dry-run"))?;
    let human = match data["bundles"].as_array() {
        Some(rows) => format!("{} bound bundle folder(s) handled\n", rows.len()),
        None => "bundle auto-apply is off\n".to_string(),
    };
    Ok(Output::new(data, human))
}

const MEASURE_USAGE: &str = "usage: context measure [--cwd <dir>] [--without plugin:<id>|skill:<name>]... [--bundle <name>] [--model <m>] [--force] [--yes]";

pub(super) const MEASURE: Spec = Spec {
    flags: &[
        value("--cwd"),
        value("--model"),
        value("--without"),
        value("--bundle"),
        switch("--force"),
        switch("--yes"),
    ],
    operands: Operands::Max(0, MEASURE_USAGE),
    ..LS
};

pub fn measure(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = MEASURE.parse(rest)?;
    let data = api::measure(
        host,
        flags.one("--cwd"),
        &api::MeasureOpts {
            model: flags.one("--model"),
            force: flags.on("--force"),
            confirm: flags.on("--yes"),
            without: flags.all("--without"),
            bundle: flags.one("--bundle").map(String::from),
        },
    )
    .map_err(flagged)?;
    let human = if data["ran"] == Value::Bool(false) {
        format!(
            "Not measured yet. {}\nRun again with --yes to start the request.\n",
            data["plan"]["cost"].as_str().unwrap_or("")
        )
    } else {
        let mut text = format!(
            "{} tokens measured ({}% of the window), {} of them itemised as estimates, {} not itemised{}\n",
            data["total"],
            data["windowPercent"],
            data["itemised"]["tokens"],
            data["notItemised"]["tokens"],
            if data["stale"] == Value::Bool(true) { " (stale)" } else { "" }
        );
        for variant in data["variants"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                "  {}: {} tokens ({:+}, {:+}%)\n",
                variant["label"].as_str().unwrap_or(""),
                variant["total"],
                variant["deltaTokens"].as_i64().unwrap_or(0),
                variant["deltaPercent"].as_f64().unwrap_or(0.0)
            ));
        }
        if let Some(factor) = data["calibration"]["factor"].as_f64() {
            text.push_str(&format!(
                "  calibration: x{factor} (measured / estimated)\n"
            ));
        }
        text
    };
    Ok(Output::new(data, human))
}
