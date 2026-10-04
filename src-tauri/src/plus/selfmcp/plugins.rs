//! `plugins_ls`, `plugins_show` and `hooks_ls`: the read-only inventory of Claude Code plugins and
//! hooks, adapters over `plus::plugins::api` that `toolportctl plugins|hooks` render as well.

use super::ToolError;
use crate::plus::args::{flag, str_nonempty};
use crate::plus::plugins::{api, hooks};
use serde_json::Value;

pub(super) fn ls(args: &Value) -> Result<Value, ToolError> {
    Ok(api::ls(str_nonempty(args, "cwd"), flag(args, "refresh"))?)
}

pub(super) fn show(args: &Value) -> Result<Value, ToolError> {
    let id = str_nonempty(args, "id")
        .ok_or_else(|| ToolError::new("invalid_arguments", "id is required"))?;
    Ok(api::show(id, str_nonempty(args, "cwd"))?)
}

pub(super) fn hooks_ls(args: &Value) -> Result<Value, ToolError> {
    let own = |key: &str| str_nonempty(args, key).map(String::from);
    let filter = hooks::Filter {
        tool: own("tool"),
        event: own("event"),
        owner: own("owner"),
    };
    Ok(api::hooks_ls(str_nonempty(args, "cwd"), &filter)?)
}
