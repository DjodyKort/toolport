//! `secret_check`, `secret_set` and `secret_rm`: a vault entry's presence, and its writes. A write
//! applies only after the person approves that call; `secret_set` takes the value only to apply
//! and no result, error or approval request carries it.

use serde_json::{json, Value};
use toolport_plus_api::{Host, ServerRecord, HTTP_AUTH_KEY};

use super::catalog::ToolDef;
use super::{approval, ToolError};
use crate::args::{flag_or, str_trimmed};
use crate::servers;

pub(super) fn check(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let invalid = |message: &str| ToolError::new("invalid_arguments", message);
    let name = str_trimmed(args, "name")
        .filter(|name| !name.starts_with('-'))
        .ok_or_else(|| invalid("name is required"))?;
    let key = args
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| servers::valid_secret_key(key))
        .ok_or_else(|| invalid("key is required and must be a valid secret key"))?;
    let all = servers::read_servers(host)?;
    let server = servers::find(&all, name)
        .map(|record| record.id.clone())
        .ok_or_else(|| ToolError::new("not_found", format!("server not found: {name}")))?;
    let set = host
        .read_vault_secret(&server, key)
        .map_err(|error| ToolError::new("vault_error", error))?
        .is_some();
    Ok(json!({"server": server, "key": key, "set": set}))
}

const MAX_VALUE_BYTES: usize = 64 * 1024;

/// What the call carries that must not come back in any output.
pub(super) fn supplied_values<'a>(tool: &ToolDef, args: &'a Value) -> Vec<&'a str> {
    if tool.name != "secret_set" {
        return Vec::new();
    }
    args.get("value")
        .and_then(Value::as_str)
        .into_iter()
        .collect()
}

struct Target {
    server: String,
    key: String,
    record: Option<ServerRecord>,
}

fn target(host: &dyn Host, args: &Value, must_exist: bool) -> Result<Target, ToolError> {
    let invalid = |message: &str| ToolError::new("invalid_arguments", message);
    let name = str_trimmed(args, "name")
        .filter(|name| !name.starts_with('-'))
        .ok_or_else(|| invalid("name is required"))?;
    let key = args
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| servers::valid_secret_key(key))
        .ok_or_else(|| invalid("key is required and must be a valid secret key"))?;
    let all = servers::read_servers(host)?;
    let record = servers::find(&all, name).cloned();
    if record.is_none() && must_exist {
        return Err(ToolError::new(
            "not_found",
            format!("server not found: {name}"),
        ));
    }
    Ok(Target {
        server: record
            .as_ref()
            .map_or_else(|| name.to_string(), |record| record.id.clone()),
        key: key.to_string(),
        record,
    })
}

fn is_set(host: &dyn Host, target: &Target) -> Result<bool, ToolError> {
    host.read_vault_secret(&target.server, &target.key)
        .map(|value| value.is_some())
        .map_err(|error| ToolError::new("vault_error", error))
}

fn plan(summary: String, op: &str, detail: String, undo: &str, warnings: Vec<String>) -> Value {
    json!({
        "summary": summary,
        "steps": [{"op": op, "detail": detail}],
        "undo": undo,
        "warnings": warnings,
    })
}

fn approval_steps(plan: &Value) -> Vec<String> {
    plan["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["detail"].as_str().map(String::from))
        .collect()
}

pub(super) fn set(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let invalid = |message: &str| ToolError::new("invalid_arguments", message);
    let target = target(host, args, true)?;
    let record = target.record.as_ref().expect("must_exist found a record");
    if target.key == HTTP_AUTH_KEY && !servers::is_remote(record) {
        return Err(invalid(&format!(
            "{HTTP_AUTH_KEY} is the bearer token of a remote server; {} is not one",
            target.server
        )));
    }
    let dry_run = flag_or(args, "dry_run", true);
    let value = args.get("value").and_then(Value::as_str);
    if dry_run && value.is_some() {
        return Err(invalid(
            "value is only accepted with dry_run=false; a preview never needs it",
        ));
    }
    let replaces = is_set(host, &target)?;
    let plan = plan(
        format!("Store {} for {}", target.key, target.server),
        if replaces { "replace" } else { "store" },
        format!(
            "{} the vault entry {} of server {}; the value is sent with the apply call and is never shown again",
            if replaces { "replace" } else { "store" },
            target.key,
            target.server
        ),
        "secret_rm removes the entry; a replaced value cannot be restored",
        vec!["every apply call asks the person for approval in the Toolport app".into()],
    );
    if dry_run {
        return Ok(json!({
            "dryRun": true,
            "server": target.server,
            "key": target.key,
            "replaces": replaces,
            "plan": plan,
        }));
    }
    let value = value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("value is required to apply"))?;
    if value.len() > MAX_VALUE_BYTES {
        return Err(invalid("value is too large"));
    }
    approval::ask(
        host,
        &format!("secret_set {}/{}", target.server, target.key),
        approval_steps(&plan),
        json!([{"server": target.server, "key": target.key}]),
    )?;
    servers::store_secret(host, record, &target.key, value)
        .map_err(|error| ToolError::new("vault_error", error))?;
    Ok(json!({
        "dryRun": false,
        "server": target.server,
        "key": target.key,
        "stored": true,
        "replaced": replaces,
    }))
}

pub(super) fn rm(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let target = target(host, args, false)?;
    let exists = is_set(host, &target)?;
    let dry_run = flag_or(args, "dry_run", true);
    let plan = plan(
        format!("Remove {} of {}", target.key, target.server),
        "remove",
        format!(
            "delete the vault entry {} of server {}",
            target.key, target.server
        ),
        "none: set the value again with secret_set",
        {
            let mut warnings = vec![
                "every apply call asks the person for approval in the Toolport app".to_string(),
            ];
            if !exists {
                warnings.push("the entry is not set; nothing would be removed".into());
            }
            warnings
        },
    );
    if dry_run {
        return Ok(json!({
            "dryRun": true,
            "server": target.server,
            "key": target.key,
            "exists": exists,
            "plan": plan,
        }));
    }
    if !exists {
        return Err(ToolError::new(
            "not_found",
            format!("{} is not set for {}", target.key, target.server),
        ));
    }
    approval::ask(
        host,
        &format!("secret_rm {}/{}", target.server, target.key),
        approval_steps(&plan),
        json!([{"server": target.server, "key": target.key}]),
    )?;
    servers::remove_secret(host, &target.server, target.record.as_ref(), &target.key)
        .map_err(|error| ToolError::new("vault_error", error))?;
    Ok(json!({
        "dryRun": false,
        "server": target.server,
        "key": target.key,
        "removed": true,
    }))
}
