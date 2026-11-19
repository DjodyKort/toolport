//! `toolportctl server update [<server>...|--all] [--to <version>] [--fetch] [--dry-run]
//! [--undo]` and `server source [<server>]`: move servers to another version of the package their
//! arguments pin or of the GitHub release they were installed from, and say where each server's
//! code comes from. One server shows its plan and result as one; several servers (or `--all`)
//! share one plan, one confirm and one combined undo, and each keeps its own result.

use std::time::Duration;

use serde_json::json;
use toolport_plus_api::Host;

use super::flags::{switch, value, Spec};
use super::output::{CtlError, Output};
use crate::servers::package::source;
use crate::servers::package::update::{self, UpdateArgs};
use crate::servers::repo::update::RestartChoice;

const USAGE: &str = "usage: server update <id|name>... [--all] [--to <version>] [--fetch] [--dry-run] [--plan-hash <hash>] [--allow-unverified] [--restart-now|--no-restart] [--wait <seconds>] | --undo [--dry-run]; several servers or --all share one plan, one confirm and one undo; --to takes one server";
const SOURCE_USAGE: &str = "usage: server source [<id|name>]";

pub(super) const UPDATE: Spec = Spec {
    flags: &[
        value("--to"),
        switch("--all"),
        switch("--fetch"),
        switch("--dry-run"),
        value("--plan-hash"),
        switch("--allow-unverified"),
        switch("--restart-now"),
        switch("--no-restart"),
        value("--wait"),
        switch("--undo"),
    ],
    ..Spec::PLAIN
};

pub(super) const SOURCE: Spec = Spec::PLAIN;

pub fn run(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UPDATE.parse(rest)?;
    let keys = flags.operands();
    let all = flags.on("--all");
    if (keys.is_empty() && !all) || (all && !keys.is_empty()) {
        return Err(CtlError::usage(USAGE));
    }
    let mut args = UpdateArgs::new(keys.first().map_or("", String::as_str));
    args.dry_run = flags.on("--dry-run");
    args.fetch = flags.on("--fetch");
    args.allow_unverified = flags.on("--allow-unverified");
    args.to = flags.one("--to").map(String::from);
    args.plan_hash = flags.one("--plan-hash").map(String::from);
    if flags.on("--no-restart") && flags.on("--restart-now") {
        return Err(CtlError::usage(USAGE));
    }
    args.restart = if flags.on("--no-restart") {
        RestartChoice::Skip
    } else if flags.on("--restart-now") {
        RestartChoice::Now
    } else {
        RestartChoice::IfIdle
    };
    if let Some(seconds) = flags.number::<u64>("--wait")? {
        args.restart_wait = Duration::from_secs(seconds);
    }
    let batch = all || keys.len() > 1;
    if batch && args.to.is_some() {
        return Err(CtlError::usage(USAGE));
    }
    if flags.on("--undo") {
        if args.to.is_some() || args.plan_hash.is_some() || args.fetch {
            return Err(CtlError::usage(USAGE));
        }
        if batch && args.dry_run {
            let plans = update::undo_plan_batch(host, keys)?;
            let human: String = plans.iter().map(|p| p.human()).collect();
            return Ok(Output::new(
                json!({"plans": plans.iter().map(|p| p.to_value()).collect::<Vec<_>>()}),
                human,
            ));
        }
        if batch {
            let done = update::undo_batch(host, keys, &args)?;
            let mut output = Output::new(done.to_value(), done.human("Put back"));
            output.failed = done.failed();
            return Ok(output);
        }
        if args.dry_run {
            let plan = update::undo_plan(host, &args.key)?;
            return Ok(Output::new(json!({"plan": plan.to_value()}), plan.human()));
        }
        let done = update::undo(host, &args)?;
        let mut output = Output::new(done.to_value(), done.human("Put back"));
        output.failed = done.failed();
        return Ok(output);
    }
    if batch {
        if args.dry_run {
            let plan = update::plan_batch(host, keys, &args)?;
            return Ok(Output::new(json!({"batch": plan.to_value()}), plan.human()));
        }
        let done = update::apply_batch(host, keys, &args)?;
        let mut output = Output::new(done.to_value(), done.human("Updated"));
        output.failed = done.failed();
        return Ok(output);
    }
    if args.dry_run {
        let plan = update::plan(host, &args, true)?;
        return Ok(Output::new(json!({"plan": plan.to_value()}), plan.human()));
    }
    let done = update::apply(host, &args)?;
    let mut output = Output::new(done.to_value(), done.human("Updated"));
    output.failed = done.failed();
    Ok(output)
}

pub fn source_rows(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SOURCE.parse(rest)?;
    let key = match flags.operands() {
        [] => None,
        [key] => Some(key.as_str()),
        _ => return Err(CtlError::usage(SOURCE_USAGE)),
    };
    let rows = source::rows(host, key)?;
    let human = if rows.is_empty() {
        "No local servers.".to_string()
    } else {
        rows.iter()
            .map(|r| r.human())
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(Output::new(
        json!({"sources": rows.iter().map(|r| r.to_value()).collect::<Vec<_>>()}),
        human,
    ))
}
