//! `servers_update`: what `toolportctl server update` does, for agents. An update is applied only
//! with the hash of the plan the caller read, which covers the version, the registry change and
//! the release file it installs.

use std::time::Duration;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, flag_or, str_trimmed};
use crate::servers::package::update::{self, UpdateArgs};
use crate::servers::repo::update::RestartChoice;
use crate::servers::restart;

type Outcome = Result<Value, ToolError>;

const MAX_WAIT: Duration = Duration::from_secs(60);

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::new("invalid_arguments", message)
}

fn args_of(args: &Value) -> Result<UpdateArgs, ToolError> {
    let key = str_trimmed(args, "name")
        .filter(|name| !name.starts_with('-'))
        .ok_or_else(|| invalid("invalid server name"))?;
    let mut ask = UpdateArgs::new(key);
    ask.dry_run = flag_or(args, "dry_run", true);
    ask.to = str_trimmed(args, "to").map(String::from);
    ask.fetch = flag(args, "fetch");
    ask.allow_unverified = flag(args, "allow_unverified");
    ask.plan_hash = str_trimmed(args, "plan_hash").map(String::from);
    ask.restart = if flag(args, "restart_now") {
        RestartChoice::Now
    } else {
        RestartChoice::IfIdle
    };
    ask.restart_wait = restart::DEFAULT_WAIT.min(MAX_WAIT);
    Ok(ask)
}

pub(super) fn run(host: &dyn Host, args: &Value) -> Outcome {
    let ask = args_of(args)?;
    if flag(args, "undo") {
        if ask.to.is_some() || ask.plan_hash.is_some() {
            return Err(invalid("undo takes no to or plan_hash"));
        }
        if ask.dry_run {
            return Ok(update::undo_plan(host, &ask.key)?.to_value());
        }
        return Ok(update::undo(host, &ask)?.to_value());
    }
    if !ask.dry_run && ask.plan_hash.is_none() {
        return Err(invalid(
            "plan_hash is required: take it from servers_update with dry_run=true",
        ));
    }
    if ask.dry_run {
        let mut plan = update::plan(host, &ask, true)?.to_value();
        plan["applyWith"] = json!(
            "call servers_update with dry_run=false, this planHash as plan_hash, this `to` as to and confirm=true"
        );
        return Ok(plan);
    }
    Ok(update::apply(host, &ask)?.to_value())
}
