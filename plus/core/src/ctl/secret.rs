//! `toolportctl secret set|get|rm`: the vault entries of a server. The vault, its backends and the
//! registry declaration are upstream's; this layer picks the right upstream operation per key and
//! keeps the value out of everything but stdin and an explicit `get --reveal`.

use std::io::{IsTerminal, Read};

use serde_json::json;
use toolport_plus_api::{Host, ServerRecord, HTTP_AUTH_KEY};

use super::flags::{switch, value, Spec};
use super::output::{CtlError, Output};
use crate::servers;

const MAX_VALUE_BYTES: u64 = 64 * 1024;
const SET_USAGE: &str = "usage: secret set <server> <KEY> [--value-env <VAR>] (value on stdin)";
const GET_USAGE: &str = "usage: secret get <server> <KEY> [--reveal]";
const RM_USAGE: &str = "usage: secret rm <server> <KEY>";

pub(super) const SET: Spec = Spec {
    flags: &[value("--value-env").needs("a variable name")],
    ..Spec::PLAIN
};
pub(super) const GET: Spec = Spec {
    flags: &[switch("--reveal")],
    ..Spec::PLAIN
};

struct Target {
    server: String,
    key: String,
    /// The server's registry entry; `None` for a vault entry whose server is gone.
    record: Option<ServerRecord>,
}

fn resolve_target(
    host: &dyn Host,
    operands: &[String],
    usage: &str,
    must_exist: bool,
) -> Result<Target, CtlError> {
    let [server, key] = operands else {
        return Err(CtlError::usage(usage));
    };
    if key.is_empty()
        || key.trim() != key
        || key.contains(['=', '\0'])
        || key.contains("::")
        || (key.starts_with("__") && key.ends_with("__") && key != HTTP_AUTH_KEY)
    {
        return Err(CtlError::usage(format!("invalid secret key: {key}")));
    }
    let all = match host
        .read_view()
        .map_err(|error| CtlError::failed("registry_error", error))?
    {
        Some(view) => view
            .servers()
            .map_err(|error| CtlError::failed("registry_error", error))?,
        None => Vec::new(),
    };
    let record = servers::find(&all, server).cloned();
    if record.is_none() && must_exist {
        return Err(CtlError::not_found(format!("unknown server: {server}")));
    }
    Ok(Target {
        server: record
            .as_ref()
            .map_or_else(|| server.clone(), |record| record.id.clone()),
        key: key.clone(),
        record,
    })
}

fn is_remote(record: &ServerRecord) -> bool {
    matches!(record.text("transport"), Some("http" | "sse"))
}

/// Launch inputs are vaulted without declaring an environment variable; every other key is an
/// environment secret.
fn is_launch_secret(record: &ServerRecord, key: &str) -> bool {
    record
        .extra
        .get("launch")
        .and_then(|launch| launch.get("inputs"))
        .and_then(|inputs| inputs.as_array())
        .is_some_and(|inputs| {
            inputs.iter().any(|input| {
                input.get("key").and_then(|k| k.as_str()) == Some(key)
                    && input.get("secret").and_then(|s| s.as_bool()) == Some(true)
            })
        })
}

fn strip_line_ending(mut value: String) -> String {
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    value
}

fn read_value(
    host: &dyn Host,
    value_env: Option<&str>,
    reader: &mut dyn Read,
    interactive: bool,
) -> Result<String, CtlError> {
    let raw = match value_env {
        Some(name) => host.env_var(name).ok_or_else(|| {
            CtlError::failed("input", format!("environment variable {name} is not set"))
        })?,
        None => {
            if interactive {
                return Err(CtlError::usage(
                    "pipe the value on stdin or use --value-env <VAR>",
                ));
            }
            let mut text = String::new();
            reader
                .take(MAX_VALUE_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|e| CtlError::failed("input", format!("cannot read value: {e}")))?;
            if text.len() as u64 > MAX_VALUE_BYTES {
                return Err(CtlError::failed("input", "value is too large"));
            }
            text
        }
    };
    let value = strip_line_ending(raw);
    if value.is_empty() {
        return Err(CtlError::failed("input", "value is empty"));
    }
    Ok(value)
}

pub fn set(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    set_from(host, rest, &mut stdin.lock(), interactive)
}

fn set_from(
    host: &dyn Host,
    rest: &[String],
    reader: &mut dyn Read,
    interactive: bool,
) -> Result<Output, CtlError> {
    let flags = SET.parse(rest)?;
    let target = resolve_target(host, flags.operands(), SET_USAGE, true)?;
    let record = target
        .record
        .as_ref()
        .expect("must_exist resolved a record");
    let access_token = target.key == HTTP_AUTH_KEY;
    if access_token && !is_remote(record) {
        return Err(CtlError::usage(format!(
            "{HTTP_AUTH_KEY} is the bearer token of a remote server; {} is not one",
            target.server
        )));
    }
    let value = read_value(host, flags.one("--value-env"), reader, interactive)?;
    let stored = if access_token {
        host.store_access_token(&target.server, &value)
    } else if is_launch_secret(record, &target.key) {
        host.store_launch_secret(&target.server, &target.key, &value)
    } else {
        host.store_env_secret(&target.server, &target.key, &value)
    };
    stored.map_err(|e| CtlError::failed("vault", e))?;
    Ok(Output::new(
        json!({"server": target.server, "key": target.key, "stored": true}),
        format!("Stored {} for {}", target.key, target.server),
    ))
}

pub fn get(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = GET.parse(rest)?;
    let target = resolve_target(host, flags.operands(), GET_USAGE, false)?;
    if target.key == HTTP_AUTH_KEY && flags.on("--reveal") {
        return Err(CtlError::usage(format!(
            "{HTTP_AUTH_KEY} is an access token and is never printed"
        )));
    }
    let value = host
        .read_vault_secret(&target.server, &target.key)
        .map_err(|e| CtlError::failed("vault", e))?;
    let Some(value) = value else {
        return Err(CtlError::not_found(format!(
            "{} is not set for {}",
            target.key, target.server
        )));
    };
    let mut data = json!({"server": target.server, "key": target.key, "set": true});
    let human = if flags.on("--reveal") {
        data["value"] = json!(value);
        value
    } else {
        format!("{} is set for {}", target.key, target.server)
    };
    Ok(Output::new(data, human))
}

pub fn rm(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = Spec::PLAIN.parse(rest)?;
    let target = resolve_target(host, flags.operands(), RM_USAGE, false)?;
    let removed = match &target.record {
        Some(_) if target.key == HTTP_AUTH_KEY => host.clear_access_token(&target.server),
        Some(_) => host.remove_env_secret(&target.server, &target.key),
        None => host.delete_server_secret(&target.server, &target.key),
    };
    removed.map_err(|e| CtlError::failed("vault", e))?;
    Ok(Output::new(
        json!({"server": target.server, "key": target.key, "removed": true}),
        format!("Removed {} for {}", target.key, target.server),
    ))
}

#[cfg(test)]
mod tests;
