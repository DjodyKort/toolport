//! `plugins_ls`, `plugins_show`, `plugins_config`, `plugins_mcp` and `hooks_ls`: the read-only
//! inventory behind `toolportctl plugins ls|show|config|mcp ls` and `hooks ls`. Switching a
//! plugin, setting a knob and denying a server are not offered here (D-097, D-154).

use std::path::Path;

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, str_nonempty, str_trimmed};
use crate::context::plugin_config;
use crate::plugins::inventory::{self, HookFilter, World};
use crate::plugins::{ClaudeRunner, SystemClaude};

fn with_world<R>(
    host: &dyn Host,
    run: impl FnOnce(&World, Option<&dyn ClaudeRunner>) -> Result<R, ToolError>,
) -> Result<R, ToolError> {
    let world = World::host(host)
        .ok_or_else(|| ToolError::new("no_home", "home directory could not be resolved"))?;
    let claude = SystemClaude::from_env();
    run(&world, Some(&claude))
}

pub(super) fn ls(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_world(host, |world, runner| {
        Ok(inventory::ls(
            world,
            runner,
            str_nonempty(args, "cwd").map(Path::new),
            flag(args, "refresh"),
        ))
    })
}

pub(super) fn show(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let id = str_trimmed(args, "id")
        .ok_or_else(|| ToolError::new("invalid_arguments", "id is required"))?;
    with_world(host, |world, runner| {
        Ok(inventory::show(
            world,
            runner,
            id,
            str_nonempty(args, "cwd").map(Path::new),
        )?)
    })
}

pub(super) fn hooks(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_world(host, |world, runner| {
        Ok(inventory::hooks(
            world,
            runner,
            str_nonempty(args, "cwd").map(Path::new),
            &HookFilter {
                tool: str_nonempty(args, "tool"),
                event: str_nonempty(args, "event"),
                owner: str_nonempty(args, "owner"),
            },
        ))
    })
}

fn id_of(args: &Value) -> Result<&str, ToolError> {
    str_trimmed(args, "id").ok_or_else(|| ToolError::new("invalid_arguments", "id is required"))
}

pub(super) fn config(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let id = id_of(args)?;
    with_world(host, |world, runner| {
        Ok(plugin_config::read(
            world,
            runner,
            id,
            str_nonempty(args, "cwd").map(Path::new),
        )?)
    })
}

pub(super) fn mcp(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let id = id_of(args)?;
    with_world(host, |world, runner| {
        Ok(plugin_config::mcp_read(
            world,
            runner,
            id,
            str_nonempty(args, "cwd").map(Path::new),
        )?)
    })
}
