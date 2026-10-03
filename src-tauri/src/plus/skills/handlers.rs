//! `plus.skills.*` handlers: thin adapters over the self-MCP skills tools so the desktop, the
//! CLI and the MCP surface share one implementation. Arguments use the tool's snake_case keys.

use crate::plus::selfmcp::{self, ToolError};
use serde_json::Value;

fn finish(result: Result<Value, ToolError>) -> Result<Value, String> {
    result.map_err(|e| e.message)
}

pub fn sync_handler(args: Value) -> Result<Value, String> {
    finish(selfmcp::call_tool("skills_sync", &args))
}

pub fn list_handler(args: Value) -> Result<Value, String> {
    finish(selfmcp::call_tool("skills_list", &args))
}

pub fn lint_handler(args: Value) -> Result<Value, String> {
    finish(selfmcp::call_tool("skills_lint", &args))
}

pub fn diff_handler(args: Value) -> Result<Value, String> {
    finish(selfmcp::skills_diff(&args))
}

pub fn status_handler(args: Value) -> Result<Value, String> {
    finish(selfmcp::call_tool("skills_status", &args))
}
