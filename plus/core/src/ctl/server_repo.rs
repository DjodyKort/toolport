//! `toolportctl server repo status|check|update|undo`: local servers that run from a git
//! checkout. `status` and `check` only read and fetch; `update` fast-forwards the tracked branch
//! after a plan, runs the agreed build command and restarts the server; `undo` takes it back.

use std::time::Duration;

use serde_json::json;
use toolport_plus_api::Host;

use super::flags::{switch, value, Flags, Spec};
use super::output::{CtlError, Output};
use crate::servers::repo::switch;
use crate::servers::repo::update::{self, RestartChoice, UpdateArgs};
use crate::servers::repo::{self, fetch_tracked};

const STATUS_USAGE: &str = "usage: server repo status [<id|name>]";
const CHECK_USAGE: &str = "usage: server repo check";
const UPDATE_USAGE: &str = "usage: server repo update <id|name> [--dry-run] [--plan-hash <hash>] [--build <command>|--no-build] [--restart-now|--no-restart] [--wait <seconds>]";
const SWITCH_USAGE: &str = "usage: server repo switch <id|name> [--machine <name>] [--fetch] [--dry-run] [--plan-hash <hash>] [--build <command>|--no-build] [--restart-now|--no-restart] [--wait <seconds>] | --undo [--dry-run]";
const UNDO_USAGE: &str = "usage: server repo undo <id|name> [--dry-run] [--no-build] [--restart-now|--no-restart] [--wait <seconds>]";

pub(super) const STATUS: Spec = Spec::PLAIN;
pub(super) const CHECK: Spec = Spec::NONE;
pub(super) const UPDATE: Spec = Spec {
    flags: &[
        switch("--dry-run"),
        value("--plan-hash"),
        value("--build"),
        switch("--no-build"),
        switch("--restart-now"),
        switch("--no-restart"),
        value("--wait"),
        switch("--fetch"),
    ],
    ..Spec::PLAIN
};
pub(super) const SWITCH: Spec = Spec {
    flags: &[
        switch("--dry-run"),
        switch("--undo"),
        switch("--fetch"),
        value("--machine"),
        value("--plan-hash"),
        value("--build"),
        switch("--no-build"),
        switch("--restart-now"),
        switch("--no-restart"),
        value("--wait"),
    ],
    ..Spec::PLAIN
};
pub(super) const UNDO: Spec = Spec {
    flags: &[
        switch("--dry-run"),
        switch("--no-build"),
        switch("--restart-now"),
        switch("--no-restart"),
        value("--wait"),
    ],
    ..Spec::PLAIN
};

fn args_of(flags: &Flags, key: &str, usage: &str) -> Result<UpdateArgs, CtlError> {
    let mut args = UpdateArgs::new(key);
    args.dry_run = flags.on("--dry-run");
    args.no_build = flags.on("--no-build");
    if flags.on("--no-restart") && flags.on("--restart-now") {
        return Err(CtlError::usage(usage));
    }
    if flags.on("--no-build") && flags.one("--build").is_some() {
        return Err(CtlError::usage(usage));
    }
    args.restart = if flags.on("--no-restart") {
        RestartChoice::Skip
    } else if flags.on("--restart-now") {
        RestartChoice::Now
    } else {
        RestartChoice::IfIdle
    };
    args.build = flags.one("--build").map(String::from);
    args.plan_hash = flags.one("--plan-hash").map(String::from);
    args.fetch = flags.on("--fetch");
    args.machine = flags.one("--machine").map(String::from);
    if let Some(seconds) = flags.number::<u64>("--wait")? {
        args.restart_wait = Duration::from_secs(seconds);
    }
    Ok(args)
}

pub fn status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    let key = match flags.operands() {
        [] => None,
        [key] => Some(key.as_str()),
        _ => return Err(CtlError::usage(STATUS_USAGE)),
    };
    let repos = repo::repos_for(host, key)?;
    Ok(Output::new(
        repo::status_value(&repos),
        repo::status_human(&repos),
    ))
}

pub fn check(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    CHECK
        .parse(rest)
        .map_err(|_| CtlError::usage(CHECK_USAGE))?;
    let report = repo::check(host, host.now_unix(), &fetch_tracked)?;
    let mut output = Output::new(report.to_value(), report.human());
    output.failed = report.repos.iter().any(|(_, result)| result.is_err());
    Ok(output)
}

pub fn update(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UPDATE.parse(rest)?;
    let key = flags.single(UPDATE_USAGE)?;
    let args = args_of(&flags, key, UPDATE_USAGE)?;
    if args.dry_run {
        let plan = update::plan(host, &args, true)?;
        return Ok(Output::new(json!({"plan": plan.to_value()}), plan.human()));
    }
    let done = update::apply(host, &args)?;
    let mut output = Output::new(done.to_value(), done.human("Updated"));
    output.failed = done.failed();
    Ok(output)
}

pub fn undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNDO.parse(rest)?;
    let key = flags.single(UNDO_USAGE)?;
    let args = args_of(&flags, key, UNDO_USAGE)?;
    if args.dry_run {
        let plan = update::undo_plan(host, key)?;
        return Ok(Output::new(json!({"plan": plan.to_value()}), plan.human()));
    }
    let done = update::undo(host, &args)?;
    let mut output = Output::new(done.to_value(), done.human("Moved back"));
    output.failed = done.failed();
    Ok(output)
}

pub fn switch_branch(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SWITCH.parse(rest)?;
    let key = flags.single(SWITCH_USAGE)?;
    let args = args_of(&flags, key, SWITCH_USAGE)?;
    if flags.on("--undo") {
        if args.dry_run {
            let plan = switch::back_plan(host, key)?;
            return Ok(Output::new(json!({"plan": plan.to_value()}), plan.human()));
        }
        let done = switch::back(host, &args)?;
        let mut output = Output::new(done.to_value(), done.human("Switched back"));
        output.failed = done.failed();
        return Ok(output);
    }
    if args.dry_run {
        let plan = switch::plan(host, &args)?;
        return Ok(Output::new(json!({"plan": plan.to_value()}), plan.human()));
    }
    let (done, from_branch) = switch::apply(host, &args)?;
    let mut value = done.to_value();
    value["fromBranch"] = json!(from_branch);
    let mut output = Output::new(value, done.human("Switched"));
    output.failed = done.failed();
    Ok(output)
}
