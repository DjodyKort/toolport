//! The per-server mode: whether a server's tools are listed to clients that get the full tool
//! list, or only found through `toolport_search_tools` and called by name. The value is a string
//! under the server's `serverMode` key so that a later `listed` is one more entry in `MODES`.

use serde_json::{json, Map, Value};
use toolport_plus_api::{ForkServerFields, Host, ServerRecord};

use super::ServerError;

pub const AUTO: &str = "auto";
pub const SEARCH: &str = "search";
pub const MODES: [&str; 2] = [AUTO, SEARCH];

const SEARCH_TOOL: &str = "toolport_search_tools";
const SEARCH_NOTE: &str = "Some servers are search-only: their tools are not listed. Find them with this tool and call them with toolport_call_tool.";

const RETIRED: [&str; 4] = ["direct", "router", "legacy", "bridge"];

/// The canonical mode for a name given by a caller, or why it is refused.
pub fn parse(raw: &str) -> Result<&'static str, String> {
    let raw = raw.trim();
    if let Some(mode) = MODES.iter().find(|mode| **mode == raw) {
        return Ok(mode);
    }
    if RETIRED.contains(&raw) {
        return Err(format!(
            "mode '{raw}' no longer exists: it was an mcpm transport shape. A server's mode is {}",
            choices()
        ));
    }
    Err(format!("mode must be {}", choices()))
}

fn choices() -> String {
    format!("{} or {}", AUTO, SEARCH)
}

/// The mode behind a stored value; anything unreadable counts as `auto`.
pub fn normalize(stored: Option<&str>) -> &'static str {
    match stored {
        Some(SEARCH) => SEARCH,
        _ => AUTO,
    }
}

/// The mode stored on a server entry's unknown fields.
pub fn of(unknown_fields: &Map<String, Value>) -> &'static str {
    normalize(
        unknown_fields
            .get(ForkServerFields::SERVER_MODE_KEY)
            .and_then(Value::as_str),
    )
}

pub fn is_search_only(unknown_fields: &Map<String, Value>) -> bool {
    of(unknown_fields) == SEARCH
}

fn name_of(tool: &Value) -> Option<&str> {
    tool.get("name").and_then(Value::as_str)
}

/// A full tool list without the tools `is_search_only` names. The search and call helpers go in
/// behind the two floor tools when the list lacks them, and the search helper's description says
/// that some servers are only found through it.
pub fn listing(
    mut tools: Vec<Value>,
    is_search_only: impl Fn(&str) -> bool,
    helpers: [Value; 2],
) -> Vec<Value> {
    tools.retain(|tool| !name_of(tool).is_some_and(&is_search_only));
    let mut at = tools.len().min(2);
    for helper in helpers {
        if tools.iter().any(|tool| name_of(tool) == name_of(&helper)) {
            continue;
        }
        tools.insert(at, helper);
        at += 1;
    }
    let search = tools
        .iter_mut()
        .find(|tool| name_of(tool) == Some(SEARCH_TOOL));
    if let Some(description) = search.and_then(|tool| tool.get_mut("description")) {
        if let Some(text) = description.as_str() {
            *description = Value::String(format!("{text} {SEARCH_NOTE}"));
        }
    }
    tools
}

/// Stores a parsed mode on the server; `auto` removes the key so a default server stays untouched.
pub fn apply(
    host: &dyn Host,
    server: &ServerRecord,
    mode: &'static str,
) -> Result<Value, ServerError> {
    host.set_server_mode(&server.id, (mode != AUTO).then_some(mode))
        .map_err(ServerError::Rejected)?;
    Ok(json!({
        "id": server.id,
        "name": server.name,
        "mode": mode,
        "changed": mode != normalize(server.fork.server_mode.as_deref()),
    }))
}

#[cfg(test)]
mod tests;
