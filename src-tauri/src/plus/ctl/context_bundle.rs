//! `toolportctl context bundle ...` and `context use`: context bundles (D-064, D-066). Thin
//! renderers over `plus::context::bundle_api`, which the `context_bundle_*` tools call as well.

use super::flags::{switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use super::plugins::flagged;
use crate::plus::context::bundle::Edit;
use crate::plus::context::bundle_api as api;
use serde_json::Value;

const GROUP_USAGE: &str = "usage: context bundle ls|show|add|edit|rm|apply|undo|status|launch|config (show|rm|apply|launch: <name>; add|edit: <name> [--description <t>] [--skills-off <csv>] [--skills-name-only <csv>] [--skills-allow <csv>] [--plugins-off <csv>] [--layers-add <csv>] [--layers-exclude <csv>] [--agents-off <csv>] [--servers <profile>] [--bind <csv>] [--dry-run]; apply|undo: --cwd <dir> [--dry-run]; config: [--auto-apply on|off])";
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

fn csv(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
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
    v.as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

pub(super) fn plan_text(data: &Value) -> String {
    let plan = &data["plan"];
    let mut human = format!("{}\n", plan["summary"].as_str().unwrap_or(""));
    for step in plan["steps"].as_array().into_iter().flatten() {
        let path = step["path"].as_str().map(|p| format!("  {p}")).unwrap_or_default();
        human.push_str(&format!("  {:<7} {}{path}\n", step["op"].as_str().unwrap_or(""), step["detail"].as_str().unwrap_or("")));
    }
    if let (Some(before), Some(after)) = (plan["effects"]["tokens"]["before"].as_u64(), plan["effects"]["tokens"]["after"].as_u64()) {
        human.push_str(&format!(
            "  about {before} -> {after} tokens at start ({})\n",
            plan["effects"]["tokens"]["basis"].as_str().unwrap_or("estimate")
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
        human.push_str(&format!("left as they are, changed since: {}\n", conflicts.join(", ")));
    }
    human
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    LS.parse(rest)?;
    let data = api::ls()?;
    let mut human = String::new();
    for b in data["bundles"].as_array().into_iter().flatten() {
        let applied = b["appliedTo"].as_array().map_or(0, Vec::len);
        human.push_str(&format!(
            "{:<22} skills -{}/~{}/={}  plugins -{}  agents -{}  applied in {applied}{}\n",
            b["name"].as_str().unwrap_or(""),
            b["skills"]["off"], b["skills"]["nameOnly"], b["skills"]["allow"],
            texts(&b["plugins"]["off"]).len(), texts(&b["agents"]["off"]).len(),
            b["error"].as_str().map(|e| format!("  ({e})")).unwrap_or_default(),
        ));
    }
    if human.is_empty() {
        human.push_str("no bundles (profiles/<name>.yaml in the skills repository)\n");
    }
    Ok(Output::new(data, human))
}

pub fn show(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SHOW.parse(rest)?;
    let data = api::show(flags.single(SHOW_USAGE)?)?;
    let mut human = format!("{}\n{}", data["path"].as_str().unwrap_or(""), data["yaml"].as_str().unwrap_or(""));
    for issue in data["issues"].as_array().into_iter().flatten() {
        human.push_str(&format!("{}: {} {}\n", issue["level"].as_str().unwrap_or(""), issue["key"].as_str().unwrap_or(""), issue["message"].as_str().unwrap_or("")));
    }
    Ok(Output::new(data, human))
}

fn save(rest: &[String], spec: &Spec, create: bool) -> Result<Output, CtlError> {
    let flags = spec.parse(rest)?;
    let name = flags.single(SAVE_USAGE)?;
    let data = api::save(name, edit_of(&flags), flags.one("--from-folder"), create, flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn add(rest: &[String]) -> Result<Output, CtlError> {
    save(rest, &ADD, true)
}

pub fn edit(rest: &[String]) -> Result<Output, CtlError> {
    save(rest, &EDIT, false)
}

pub fn rm(rest: &[String]) -> Result<Output, CtlError> {
    let flags = RM.parse(rest)?;
    let data = api::rm(flags.single(RM_USAGE)?, flags.on("--force"), flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn apply(rest: &[String]) -> Result<Output, CtlError> {
    let flags = APPLY.parse(rest)?;
    let data = api::apply(flags.single(APPLY_USAGE)?, flags.one("--cwd"), flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn undo(rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNDO.parse(rest)?;
    let data = api::undo(flags.one("--cwd"), flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    let data = api::status(flags.one("--cwd")).map_err(flagged)?;
    let folder = data["folder"].as_str().unwrap_or("");
    let human = match data["applied"].as_object() {
        None => format!("no bundle is applied in {folder}\n"),
        Some(a) => {
            let changed = texts(&a["changedKeys"]);
            format!(
                "bundle {} applied in {folder} at {}{}\n",
                a["bundle"].as_str().unwrap_or(""),
                a["appliedAt"].as_str().unwrap_or(""),
                if changed.is_empty() { String::new() } else { format!("; drift in {}", changed.join(", ")) }
            )
        }
    };
    Ok(Output::new(data, human))
}

pub fn launch(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LAUNCH.parse(rest)?;
    let data = api::launch(flags.single(LAUNCH_USAGE)?, flags.one("--cwd")).map_err(flagged)?;
    let mut human = format!("{}\n", data["command"].as_str().unwrap_or(""));
    for note in texts(&data["notes"]) {
        human.push_str(&format!("note: {note}\n"));
    }
    Ok(Output::new(data, human))
}

pub fn config(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CONFIG.parse(rest)?;
    let auto = match flags.one("--auto-apply") {
        None => None,
        Some("on") => Some(true),
        Some("off") => Some(false),
        Some(_) => return Err(CtlError::usage("--auto-apply takes on or off")),
    };
    let data = api::config(auto)?;
    let human = format!("auto-apply: {}\n", if data["autoApply"] == Value::Bool(true) { "on" } else { "off" });
    Ok(Output::new(data, human))
}

pub fn use_bundle(rest: &[String]) -> Result<Output, CtlError> {
    let flags = USE.parse(rest)?;
    let none = flags.on("--none");
    let name = flags.operands().first().map(String::as_str);
    match (name, none) {
        (Some(_), true) => return Err(CtlError::usage("--none takes no name")),
        (None, false) => return Err(CtlError::usage(USE_USAGE)),
        _ => {}
    }
    let data = api::use_bundle(name, flags.one("--cwd"), none, flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}
