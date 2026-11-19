//! `toolportctl server update <server> [--to <version>] [--fetch] [--dry-run] [--undo]`: move a
//! server to another version of the package its arguments pin, or of the GitHub release it was
//! installed from. The plan shows the exact registry change; applying it rewrites the pin,
//! restarts the server and records what is needed to put it back.

use std::time::Duration;

use serde_json::json;
use toolport_plus_api::Host;

use super::flags::{switch, value, Spec};
use super::output::{CtlError, Output};
use crate::servers::package::update::{self, UpdateArgs};
use crate::servers::repo::update::RestartChoice;

const USAGE: &str = "usage: server update <id|name> [--to <version>] [--fetch] [--dry-run] [--plan-hash <hash>] [--allow-unverified] [--restart-now|--no-restart] [--wait <seconds>] | --undo [--dry-run]";

pub(super) const UPDATE: Spec = Spec {
    flags: &[
        value("--to"),
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

pub fn run(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UPDATE.parse(rest)?;
    let key = flags.single(USAGE)?;
    let mut args = UpdateArgs::new(key);
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
    if flags.on("--undo") {
        if args.to.is_some() || args.plan_hash.is_some() || args.fetch {
            return Err(CtlError::usage(USAGE));
        }
        if args.dry_run {
            let plan = update::undo_plan(host, key)?;
            return Ok(Output::new(json!({"plan": plan.to_value()}), plan.human()));
        }
        let done = update::undo(host, &args)?;
        let mut output = Output::new(done.to_value(), done.human("Put back"));
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
