//! `toolportctl plugins off|on|ls|show` and `hooks ls`: switch one plugin off or on in a folder
//! (D-097, D-136) and read what is installed and which hooks would run. Thin renderers over
//! `context::ops` and `plugins::inventory`.

use super::context::{flagged, plan_text};
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::context::ops as api;
use crate::context::plugin_switch::Switch;
use crate::plugins::inventory::{self, HookFilter, World};
use crate::plugins::{ClaudeRunner, SystemClaude};
use serde_json::Value;
use std::path::Path;
use toolport_plus_api::Host;

const OFF_USAGE: &str = "usage: plugins off <id> --cwd <dir> [--dry-run]";
const ON_USAGE: &str = "usage: plugins on <id> --cwd <dir> [--dry-run]";

pub(super) const OFF: Spec = Spec {
    flags: &[value("--cwd"), switch("--dry-run")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Max(1, OFF_USAGE),
    ..Spec::PLAIN
};

pub(super) const ON: Spec = Spec {
    operands: Operands::Max(1, ON_USAGE),
    ..OFF
};

fn folder(
    host: &dyn Host,
    rest: &[String],
    spec: &Spec,
    usage: &str,
    op: Switch,
) -> Result<Output, CtlError> {
    let flags = spec.parse(rest)?;
    let id = flags.single(usage)?;
    let data = api::plugin_switch(host, op, id, flags.one("--cwd"), flags.on("--dry-run"))
        .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn off(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    folder(host, rest, &OFF, OFF_USAGE, Switch::Off)
}

pub fn on(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    folder(host, rest, &ON, ON_USAGE, Switch::On)
}

const SHOW_USAGE: &str = "usage: plugins show <id> [--cwd <dir>]";

pub(super) const LS: Spec = Spec {
    flags: &[value("--cwd").needs("a folder")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const SHOW: Spec = Spec {
    operands: Operands::Max(1, SHOW_USAGE),
    ..LS
};

pub(super) const HOOKS_LS: Spec = Spec {
    flags: &[
        value("--cwd").needs("a folder"),
        value("--tool").needs("a tool name"),
        value("--event").needs("an event name"),
        value("--owner").needs("an owner kind"),
    ],
    ..LS
};

fn with_world<R>(
    host: &dyn Host,
    run: impl FnOnce(&World, Option<&dyn ClaudeRunner>) -> Result<R, CtlError>,
) -> Result<R, CtlError> {
    let home = host
        .home_dir()
        .ok_or_else(|| CtlError::failed("no_home", "home directory could not be resolved"))?;
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

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let data = with_world(host, |world, runner| {
        Ok(inventory::ls(
            world,
            runner,
            flags.one("--cwd").map(Path::new),
        ))
    })?;
    let human = ls_text(&data);
    Ok(Output::new(data, human))
}

fn ls_text(data: &Value) -> String {
    let mut out = String::new();
    for plugin in data["plugins"].as_array().into_iter().flatten() {
        let brings = &plugin["brings"];
        out.push_str(&format!(
            "{:<28} {:<10} {:<4} skills {} agents {} commands {} hooks {} mcp {}\n",
            plugin["id"].as_str().unwrap_or(""),
            plugin["version"].as_str().unwrap_or("-"),
            if plugin["enabled"]["effective"] == true {
                "on"
            } else {
                "off"
            },
            brings["skills"],
            brings["agents"],
            brings["commands"],
            brings["hooks"],
            brings["mcpServers"],
        ));
    }
    if out.is_empty() {
        out.push_str("No plugins installed.\n");
    }
    out
}

pub fn show(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SHOW.parse(rest)?;
    let id = flags.single(SHOW_USAGE)?;
    let data = with_world(host, |world, runner| {
        inventory::show(world, runner, id, flags.one("--cwd").map(Path::new))
    })?;
    let mut human = ls_text(&serde_json::json!({"plugins": [data.clone()]}));
    if let Some(description) = data["description"].as_str() {
        human.push_str(&format!("{description}\n"));
    }
    for hook in data["hooks"].as_array().into_iter().flatten() {
        human.push_str(&hook_line(hook));
    }
    Ok(Output::new(data, human))
}

fn hook_line(hook: &Value) -> String {
    format!(
        "  {:<12} {:<18} {:<8} {}\n",
        hook["owner"]["kind"].as_str().unwrap_or(""),
        hook["event"].as_str().unwrap_or(""),
        hook["matcher"].as_str().unwrap_or(""),
        hook["command"].as_str().unwrap_or(""),
    )
}

pub fn hooks_ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = HOOKS_LS.parse(rest)?;
    let data = with_world(host, |world, runner| {
        Ok(inventory::hooks(
            world,
            runner,
            flags.one("--cwd").map(Path::new),
            &HookFilter {
                tool: flags.one("--tool"),
                event: flags.one("--event"),
                owner: flags.one("--owner"),
            },
        ))
    })?;
    let mut human = String::new();
    if data["disabledAll"] == true {
        human.push_str("disableAllHooks is on: no hook below runs\n");
    }
    for hook in data["hooks"].as_array().into_iter().flatten() {
        human.push_str(&hook_line(hook));
    }
    if human.is_empty() {
        human.push_str("No hooks.\n");
    }
    Ok(Output::new(data, human))
}
