//! `secret_check`: whether a vault entry exists. The value is never returned, logged or echoed.

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::str_trimmed;
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
