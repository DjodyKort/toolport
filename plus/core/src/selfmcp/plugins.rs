//! `plugins_ls`, `plugins_show` and `hooks_ls`: the read-only inventory behind `toolportctl
//! plugins ls|show` and `hooks ls`. Switching or configuring a plugin is not offered (D-097).

use std::path::Path;

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{str_nonempty, str_trimmed};
use crate::plugins::inventory::{self, HookFilter, World};
use crate::plugins::{ClaudeRunner, SystemClaude};

fn with_world<R>(
    host: &dyn Host,
    run: impl FnOnce(&World, Option<&dyn ClaudeRunner>) -> Result<R, ToolError>,
) -> Result<R, ToolError> {
    let home = host
        .home_dir()
        .ok_or_else(|| ToolError::new("no_home", "home directory could not be resolved"))?;
    let claude_home = crate::fsx::claude_dir_in(&home);
    let claude = SystemClaude::from_env();
    run(
        &World {
            home: &home,
            claude_home: &claude_home,
        },
        Some(&claude),
    )
}

pub(super) fn ls(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_world(host, |world, runner| {
        Ok(inventory::ls(
            world,
            runner,
            str_nonempty(args, "cwd").map(Path::new),
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
