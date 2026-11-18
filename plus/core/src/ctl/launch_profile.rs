//! `toolportctl context launch-profile ls|add|rm|policy`: named starts of Claude Code with their
//! own client id and access set. Thin renderers over `context::launch_profile`.

use super::context::{flagged, plan_text};
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::context::launch_profile::{self as profiles, AddArgs};
use serde_json::Value;
use toolport_plus_api::Host;

const LS_USAGE: &str = "usage: context launch-profile ls [<name>] [--cwd <dir>]";
const ADD_USAGE: &str =
    "usage: context launch-profile add <name> [--set <access set>] [--deny-direct on|off] [--dry-run]";
const RM_USAGE: &str = "usage: context launch-profile rm <name> [--dry-run]";
const POLICY_USAGE: &str = "usage: context launch-profile policy <name> --cwd <dir> [--dry-run]";

pub(super) const LS: Spec = Spec {
    flags: &[value("--cwd")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Max(1, LS_USAGE),
    ..Spec::PLAIN
};

pub(super) const ADD: Spec = Spec {
    flags: &[value("--set"), value("--deny-direct"), switch("--dry-run")],
    operands: Operands::Max(1, ADD_USAGE),
    ..LS
};

pub(super) const RM: Spec = Spec {
    flags: &[switch("--dry-run")],
    operands: Operands::Max(1, RM_USAGE),
    ..LS
};

pub(super) const POLICY: Spec = Spec {
    flags: &[value("--cwd"), switch("--dry-run")],
    operands: Operands::Max(1, POLICY_USAGE),
    ..LS
};

fn texts(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn profile_line(profile: &Value) -> String {
    let set = profile["accessSet"]["name"]
        .as_str()
        .unwrap_or("(set missing)");
    let mut line = format!(
        "{:<18} {:<26} set {set}  {}{}\n",
        profile["name"].as_str().unwrap_or(""),
        profile["clientId"].as_str().unwrap_or(""),
        profile["state"].as_str().unwrap_or(""),
        if profile["denyDirect"] == Value::Bool(true) {
            "  policy on"
        } else {
            ""
        }
    );
    for problem in texts(&profile["problems"]) {
        line.push_str(&format!("  problem: {problem}\n"));
    }
    for warning in profile["folderWarnings"].as_array().into_iter().flatten() {
        line.push_str(&format!(
            "  warning: {}\n",
            warning["text"].as_str().unwrap_or("")
        ));
    }
    line
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let name = flags.operands().first().map(String::as_str);
    let data = profiles::ls(host, name, flags.one("--cwd")).map_err(flagged)?;
    let mut human = String::new();
    for profile in data["profiles"].as_array().into_iter().flatten() {
        human.push_str(&profile_line(profile));
        if name.is_some() {
            human.push_str(&format!(
                "  start: {}\n",
                profile["command"].as_str().unwrap_or("")
            ));
        }
    }
    if human.is_empty() {
        human.push_str(
            "no launch profiles (context launch-profile add <name> --set <access set>)\n",
        );
    }
    Ok(Output::new(data, human))
}

pub fn add(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = ADD.parse(rest)?;
    let deny_direct = match flags.one("--deny-direct") {
        None => None,
        Some("on") => Some(true),
        Some("off") => Some(false),
        Some(_) => return Err(CtlError::usage("--deny-direct takes on or off")),
    };
    let data = profiles::add(
        host,
        &AddArgs {
            name: flags.single(ADD_USAGE)?,
            set: flags.one("--set"),
            deny_direct,
            dry_run: flags.on("--dry-run"),
        },
    )?;
    let mut human = plan_text(&data);
    if data["dryRun"] == Value::Bool(false) {
        human.push_str(&format!(
            "start: {}\n",
            data["profile"]["command"].as_str().unwrap_or("")
        ));
    }
    Ok(Output::new(data, human))
}

pub fn rm(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RM.parse(rest)?;
    let data = profiles::rm(host, flags.single(RM_USAGE)?, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn policy(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = POLICY.parse(rest)?;
    let name = flags.single(POLICY_USAGE)?;
    let cwd = flags
        .one("--cwd")
        .ok_or_else(|| CtlError::usage(POLICY_USAGE))?;
    let data = profiles::policy(host, name, cwd, flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}
