//! `servers_repo_check`, `servers_repo_update`, `servers_repo_switch` and `servers_repo_undo`:
//! what `toolportctl server repo` does, for agents. An update or switch is applied only with the
//! hash of the plan the caller read, which covers the commits it installs and the build command
//! it runs.

use std::time::Duration;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, flag_or, str_arg, str_trimmed};
use crate::servers::repo::update::{self, RestartChoice, UpdateArgs};
use crate::servers::repo::{self, fetch_tracked, switch};
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
    ask.build = str_trimmed(args, "build").map(String::from);
    ask.no_build = flag(args, "no_build");
    if ask.no_build && ask.build.is_some() {
        return Err(invalid("give build or no_build=true, not both"));
    }
    ask.restart = if flag(args, "restart_now") {
        RestartChoice::Now
    } else {
        RestartChoice::IfIdle
    };
    ask.restart_wait = restart::DEFAULT_WAIT.min(MAX_WAIT);
    ask.fetch = flag(args, "fetch");
    ask.machine = str_trimmed(args, "machine").map(String::from);
    ask.plan_hash = str_trimmed(args, "plan_hash").map(String::from);
    Ok(ask)
}

fn needs_hash(ask: &UpdateArgs, tool: &str) -> Result<(), ToolError> {
    if ask.dry_run || ask.plan_hash.is_some() {
        return Ok(());
    }
    Err(invalid(format!(
        "plan_hash is required: take it from {tool} with dry_run=true"
    )))
}

fn with_apply_hint(mut plan: Value, tool: &str) -> Value {
    plan["applyWith"] = json!(format!(
        "call {tool} with dry_run=false, this planHash as plan_hash and confirm=true"
    ));
    plan
}

fn finished(done: &update::Done) -> Value {
    done.to_value()
}

pub(super) fn check(host: &dyn Host, _: &Value) -> Outcome {
    Ok(repo::check(host, host.now_unix(), &fetch_tracked)?.to_value())
}

pub(super) fn update(host: &dyn Host, args: &Value) -> Outcome {
    let ask = args_of(args)?;
    needs_hash(&ask, "servers_repo_update")?;
    if ask.dry_run {
        let plan = update::plan(host, &ask, true)?;
        return Ok(with_apply_hint(plan.to_value(), "servers_repo_update"));
    }
    Ok(finished(&update::apply(host, &ask)?))
}

pub(super) fn switch_branch(host: &dyn Host, args: &Value) -> Outcome {
    let ask = args_of(args)?;
    needs_hash(&ask, "servers_repo_switch")?;
    if ask.dry_run {
        let plan = switch::plan(host, &ask)?;
        return Ok(with_apply_hint(plan.to_value(), "servers_repo_switch"));
    }
    let (done, from_branch) = switch::apply(host, &ask)?;
    let mut value = finished(&done);
    value["fromBranch"] = json!(from_branch);
    Ok(value)
}

pub(super) fn undo(host: &dyn Host, args: &Value) -> Outcome {
    let ask = args_of(args)?;
    let switch_back = match str_arg(args, "of").map(str::trim) {
        None | Some("update") => false,
        Some("switch") => true,
        Some(_) => return Err(invalid("of is update or switch")),
    };
    if ask.dry_run {
        return Ok(if switch_back {
            switch::back_plan(host, &ask.key)?.to_value()
        } else {
            update::undo_plan(host, &ask.key)?.to_value()
        });
    }
    Ok(finished(&if switch_back {
        switch::back(host, &ask)?
    } else {
        update::undo(host, &ask)?
    }))
}
