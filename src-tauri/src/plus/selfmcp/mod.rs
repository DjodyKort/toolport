//! Self-management MCP server (D-010, MIG-SELF-3/5): tool and resource registry, confirm-tier
//! enforcement, JSON-RPC dispatch and the backends onto skills, the registry, client sync and
//! encrypted sync.

mod backend;
mod catalog;
mod content;
mod docs;
#[cfg(test)]
mod enable_tests;
mod redact;
pub mod register;
mod servers;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wired_tests;

pub use catalog::{find_resource, find_tool, Gate, ResourceDef, ToolDef, RESOURCES, TOOLS};

use crate::plus::args::flag;
use serde_json::{json, Value};

pub const SERVER_NAME: &str = "toolport-plus-self";
pub const BINARY_NAME: &str = "toolport-selfmcp";
pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub const INSTRUCTIONS: &str = "Toolport+ self-management. Read the mcpm://paths resource first. \
Tier 1 tools only read. Tier 2 tools write generated or additive state. Tier 3 and tier 4 tools \
refuse unless confirm=true; tier 4 touches remote state or removes entries. Secret values are \
never returned.";

#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    pub kind: &'static str,
    pub message: String,
}

impl ToolError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self::new("backend_error", message)
    }

    pub fn not_implemented(what: &str) -> Self {
        Self::new(
            "not_implemented",
            format!("{what} is not available yet: its backing capability lands in a later item"),
        )
    }
}

pub fn tools_list() -> Value {
    Value::Array(TOOLS.iter().map(catalog::tool_descriptor).collect())
}

pub fn resources_list() -> Value {
    Value::Array(
        RESOURCES
            .iter()
            .map(|r| {
                json!({
                    "uri": r.uri,
                    "name": r.name,
                    "description": r.description,
                    "mimeType": r.mime,
                })
            })
            .collect(),
    )
}

fn validate(tool: &ToolDef, args: &Value) -> Result<(), ToolError> {
    let Some(map) = args.as_object() else {
        return Err(ToolError::new(
            "invalid_arguments",
            "arguments must be an object",
        ));
    };
    for key in map.keys() {
        let known = tool.params.iter().any(|p| p.name == key)
            || (key == "confirm" && tool.gate != Gate::None);
        if !known {
            return Err(ToolError::new(
                "invalid_arguments",
                format!("unknown argument: {key}"),
            ));
        }
    }
    for param in tool.params {
        match map.get(param.name) {
            None | Some(Value::Null) if param.required => {
                return Err(ToolError::new(
                    "invalid_arguments",
                    format!("missing required argument: {}", param.name),
                ))
            }
            Some(value) if !value.is_null() && !type_matches(param.ty, value) => {
                return Err(ToolError::new(
                    "invalid_arguments",
                    format!("argument {} has the wrong type", param.name),
                ))
            }
            _ => {}
        }
    }
    if let Some(confirm) = map.get("confirm") {
        if !confirm.is_boolean() {
            return Err(ToolError::new(
                "invalid_arguments",
                "confirm must be a boolean",
            ));
        }
    }
    Ok(())
}

fn type_matches(ty: catalog::Ty, value: &Value) -> bool {
    match ty {
        catalog::Ty::Str => value.is_string(),
        catalog::Ty::Bool => value.is_boolean(),
        catalog::Ty::Obj => value.is_object(),
        catalog::Ty::StrList => value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string)),
    }
}

fn gate_passes(tool: &ToolDef, args: &Value) -> bool {
    let confirmed = flag(args, "confirm");
    match tool.gate {
        Gate::None => true,
        Gate::Always => confirmed,
        Gate::UnlessDryRun => confirmed || flag(args, "dry_run"),
    }
}

fn refusal(tool: &ToolDef) -> ToolError {
    let warning = if tool.tier >= 4 {
        "WARNING: this modifies remote state or removes entries. "
    } else {
        ""
    };
    ToolError::new(
        "refused",
        format!(
            "Refused: {} (tier {}). Pass confirm=true to proceed. {warning}",
            tool.name, tool.tier
        )
        .trim_end()
        .to_string(),
    )
}

pub fn call_tool(name: &str, args: &Value) -> Result<Value, ToolError> {
    let tool = find_tool(name)
        .ok_or_else(|| ToolError::new("unknown_tool", format!("unknown tool: {name}")))?;
    let args = if args.is_null() {
        json!({})
    } else {
        args.clone()
    };
    validate(tool, &args)?;
    if !gate_passes(tool, &args) {
        return Err(refusal(tool));
    }
    let value = backend::run_tool(tool, &args)?;
    Ok(redact::scrub(value))
}

pub fn skills_diff(args: &Value) -> Result<Value, ToolError> {
    content::skills_diff(args)
}

pub fn read_resource(uri: &str) -> Result<(String, &'static str), ToolError> {
    let def = find_resource(uri)
        .ok_or_else(|| ToolError::new("unknown_resource", format!("unknown resource: {uri}")))?;
    let text = backend::read_resource(def)?;
    Ok((redact::scrub_text(text), def.mime))
}

fn tool_result(outcome: Result<Value, ToolError>) -> Value {
    match outcome {
        Ok(value) => json!({
            "content": [{"type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default()}],
            "structuredContent": value,
            "isError": false,
        }),
        Err(error) => json!({
            "content": [{"type": "text", "text": format!("{}: {}", error.kind, error.message)}],
            "structuredContent": {"error": {"kind": error.kind, "message": error.message}},
            "isError": true,
        }),
    }
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn rpc_ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Handles one JSON-RPC message; notifications produce no response.
pub fn handle_message(message: &Value) -> Option<Value> {
    let method = message.get("method").and_then(Value::as_str)?;
    let id = message.get("id").cloned()?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => rpc_ok(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}, "resources": {}},
                "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            }),
        ),
        "ping" => rpc_ok(id, json!({})),
        "tools/list" => rpc_ok(id, json!({"tools": tools_list()})),
        "resources/list" => rpc_ok(id, json!({"resources": resources_list()})),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(rpc_error(id, -32602, "tools/call requires a name"));
            };
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            rpc_ok(id, tool_result(call_tool(name, &args)))
        }
        "resources/read" => {
            let Some(uri) = params.get("uri").and_then(Value::as_str) else {
                return Some(rpc_error(id, -32602, "resources/read requires a uri"));
            };
            match read_resource(uri) {
                Ok((text, mime)) => rpc_ok(
                    id,
                    json!({"contents": [{"uri": uri, "mimeType": mime, "text": text}]}),
                ),
                Err(e) if e.kind == "unknown_resource" => rpc_error(id, -32002, e.message),
                Err(e) => rpc_error(id, -32603, format!("{}: {}", e.kind, e.message)),
            }
        }
        _ => rpc_error(id, -32601, format!("method not found: {method}")),
    })
}

pub fn serve_stdio() -> std::io::Result<()> {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(&message),
            Err(e) => Some(rpc_error(Value::Null, -32700, format!("parse error: {e}"))),
        };
        if let Some(response) = response {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
