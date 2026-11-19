//! The `client_direct_*` tools: what `toolportctl client direct` does, for agents.

use serde_json::{json, Value};
use toolport_plus_api::{Host, ScopeKind};

use super::ToolError;
use crate::args::{flag, flag_or, str_arg};
use crate::direct::{self, Args};

type Outcome = Result<Value, ToolError>;

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_arg(args, key)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.starts_with('-'))
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

fn args_of(args: &Value) -> Result<Args<'_>, ToolError> {
    Ok(Args {
        server: required(args, "name")?,
        client: required(args, "client")?,
        force: flag(args, "force"),
        dry_run: flag_or(args, "dry_run", true),
        plan_hash: str_arg(args, "plan_hash"),
    })
}

pub(super) fn ls(host: &dyn Host, args: &Value) -> Outcome {
    let rows = direct::list(host, str_arg(args, "client"))?;
    Ok(direct::list_value(&rows))
}

pub(super) fn check(host: &dyn Host, args: &Value) -> Outcome {
    let (rows, healthy) = direct::check(host, str_arg(args, "client"))?;
    let mut data = direct::list_value(&rows);
    data["healthy"] = json!(healthy);
    Ok(data)
}

pub(super) fn add(host: &dyn Host, args: &Value) -> Outcome {
    Ok(direct::add(host, &args_of(args)?)?)
}

pub(super) fn rm(host: &dyn Host, args: &Value) -> Outcome {
    Ok(direct::remove(host, &args_of(args)?)?)
}

pub(super) fn list(host: &dyn Host, _args: &Value) -> Outcome {
    let view = host
        .read_view()
        .map_err(|error| ToolError::new("registry_error", error))?;
    let scopes = view.as_ref().map(|view| view.scope_reach());
    let rows: Vec<Value> = host
        .detect_clients()
        .iter()
        .map(|client| {
            let scope = scopes.iter().flatten().find(|scope| {
                scope.kind == ScopeKind::Client && scope.key == client.id && !scope.reference.is_empty()
            });
            json!({
                "id": client.id,
                "name": client.name,
                "configPath": client.config_path,
                "configExists": client.config_exists,
                "appPresent": client.app_present,
                "usesConnectors": client.uses_connectors,
                "gatewayInstalled": client.gateway_installed,
                "servers": client.entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
                "accessSet": scope.map(|scope| scope.reference.as_str()),
                "discovery": view.as_ref().and_then(|view| view.client_discovery(&client.id)),
                "error": client.error,
                "directUnsupported": client.direct_unsupported,
            })
        })
        .collect();
    Ok(json!({"clients": rows}))
}
