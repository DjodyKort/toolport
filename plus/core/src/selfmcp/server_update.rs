//! `servers_update` and `servers_source`: what `toolportctl server update` and `server source`
//! do, for agents. An update is applied only with the hash of the plan the caller read, which
//! covers the version, the registry change and the release file it installs. Several servers
//! (`names`, or `all`) share one plan and one hash, and each reports its own result.

use std::time::Duration;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, flag_or, nonempty_strings, str_trimmed};
use crate::servers::package::source;
use crate::servers::package::update::{self, UpdateArgs};
use crate::servers::repo::update::RestartChoice;
use crate::servers::restart;

type Outcome = Result<Value, ToolError>;

const MAX_WAIT: Duration = Duration::from_secs(60);

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::new("invalid_arguments", message)
}

/// The servers asked for: `name` and `names` together, nothing for `all`.
fn keys_of(args: &Value) -> Result<Vec<String>, ToolError> {
    let mut keys: Vec<String> = nonempty_strings(args, "names").unwrap_or_default();
    if let Some(name) = str_trimmed(args, "name") {
        keys.insert(0, name.to_string());
    }
    if keys
        .iter()
        .any(|key| key.is_empty() || key.starts_with('-'))
    {
        return Err(invalid("invalid server name"));
    }
    if flag(args, "all") != keys.is_empty() {
        return Err(invalid("give name or names, or all=true, not both"));
    }
    Ok(keys)
}

fn args_of(args: &Value, keys: &[String]) -> UpdateArgs {
    let mut ask = UpdateArgs::new(keys.first().map_or("", String::as_str));
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
    ask
}

pub(super) fn run(host: &dyn Host, args: &Value) -> Outcome {
    let keys = keys_of(args)?;
    let ask = args_of(args, &keys);
    let batch = flag(args, "all") || keys.len() > 1;
    if batch && ask.to.is_some() {
        return Err(invalid("to names one version, so it takes one server"));
    }
    if flag(args, "undo") {
        if ask.to.is_some() || ask.plan_hash.is_some() {
            return Err(invalid("undo takes no to or plan_hash"));
        }
        return match (batch, ask.dry_run) {
            (true, true) => Ok(json!({
                "plans": update::undo_plan_batch(host, &keys)?
                    .iter()
                    .map(|plan| plan.to_value())
                    .collect::<Vec<_>>()
            })),
            (true, false) => Ok(update::undo_batch(host, &keys, &ask)?.to_value()),
            (false, true) => Ok(update::undo_plan(host, &ask.key)?.to_value()),
            (false, false) => Ok(update::undo(host, &ask)?.to_value()),
        };
    }
    if !ask.dry_run && ask.plan_hash.is_none() {
        return Err(invalid(
            "plan_hash is required: take it from servers_update with dry_run=true",
        ));
    }
    let hint = "call servers_update with dry_run=false, this planHash as plan_hash, this `to` as to and confirm=true";
    if batch {
        if ask.dry_run {
            let mut plan = update::plan_batch(host, &keys, &ask)?.to_value();
            plan["applyWith"] = json!(
                "call servers_update with dry_run=false, the same names (or all), this planHash as plan_hash and confirm=true"
            );
            return Ok(plan);
        }
        return Ok(update::apply_batch(host, &keys, &ask)?.to_value());
    }
    if ask.dry_run {
        let mut plan = update::plan(host, &ask, true)?.to_value();
        plan["applyWith"] = json!(hint);
        return Ok(plan);
    }
    Ok(update::apply(host, &ask)?.to_value())
}

pub(super) fn sources(host: &dyn Host, args: &Value) -> Outcome {
    let key = str_trimmed(args, "name");
    let rows = source::rows(host, key)?;
    Ok(json!({"sources": rows.iter().map(|row| row.to_value()).collect::<Vec<_>>()}))
}
