//! `toolportctl plugins off|on|ls|show|config|mcp` and `hooks ls`: switch one plugin off or on in
//! a folder (D-097, D-136), read what is installed and which hooks would run, and set a plugin's
//! own switches or deny one of its MCP servers in a folder. Thin renderers over `context::ops`,
//! `context::plugin_config` and `plugins::inventory`.

use super::context::{flagged, plan_text};
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::context::ops as api;
use crate::context::plugin_config::{self, ConfigArgs, McpArgs, McpOp};
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
    flags: &[value("--cwd").needs("a folder"), switch("--refresh")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const SHOW: Spec = Spec {
    flags: &[value("--cwd").needs("a folder")],
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
    let world = World::host(host)
        .ok_or_else(|| CtlError::failed("no_home", "home directory could not be resolved"))?;
    let claude = SystemClaude::from_env();
    run(&world, Some(&claude))
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let data = with_world(host, |world, runner| {
        Ok(inventory::ls(
            world,
            runner,
            flags.one("--cwd").map(Path::new),
            flags.on("--refresh"),
        ))
    })?;
    let human = ls_text(&data);
    Ok(Output::new(data, human))
}

fn ls_text(data: &Value) -> String {
    let mut out = String::new();
    for plugin in data["plugins"].as_array().into_iter().flatten() {
        let brings = &plugin["brings"];
        let update = match plugin["update"]["available"].as_str() {
            Some(version) => format!("  update to {version}"),
            None => String::new(),
        };
        out.push_str(&format!(
            "{:<28} {:<10} {:<4} skills {} agents {} commands {} hooks {} mcp {}{update}\n",
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
    if let Some(why) = data["refreshError"].as_str() {
        out.push_str(&format!("warning: {why}\n"));
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
    for server in data["mcpServers"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "  mcp {} {}\n",
            server["key"].as_str().unwrap_or(""),
            if plugin_denied(&server["denied"]) {
                "(denied)"
            } else {
                ""
            }
        ));
    }
    for option in data["options"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "  option {} = {}\n",
            option["key"].as_str().unwrap_or(""),
            option["current"].as_str().unwrap_or("(not set)")
        ));
    }
    for knob in data["knobs"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "  knob {} = {} ({})\n",
            knob["key"].as_str().unwrap_or(""),
            knob["current"]["value"].as_str().unwrap_or("(not set)"),
            knob["current"]["from"].as_str().unwrap_or("")
        ));
    }
    for hook in data["hooks"].as_array().into_iter().flatten() {
        human.push_str(&hook_line(hook));
    }
    Ok(Output::new(data, human))
}

fn plugin_denied(denied: &Value) -> bool {
    ["user", "project", "local", "managed"]
        .iter()
        .any(|scope| denied[*scope] == true)
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

const CONFIG_USAGE: &str =
    "usage: plugins config <id> [--cwd <dir>] [--set <knob>=<value>]... [--unset <knob>]... [--dry-run]";

pub(super) const CONFIG: Spec = Spec {
    flags: &[
        value("--cwd").needs("a folder"),
        value("--set").needs("<knob>=<value>"),
        value("--unset").needs("a knob"),
        switch("--dry-run"),
    ],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Max(1, CONFIG_USAGE),
    ..Spec::PLAIN
};

fn pairs(flags: &super::flags::Flags) -> Result<Vec<(String, String)>, CtlError> {
    flags
        .all("--set")
        .iter()
        .map(|pair| {
            pair.split_once('=')
                .filter(|(knob, _)| !knob.is_empty())
                .map(|(knob, value)| (knob.to_string(), value.to_string()))
                .ok_or_else(|| CtlError::usage(format!("--set takes <knob>=<value>, not {pair:?}")))
        })
        .collect()
}

pub fn config(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CONFIG.parse(rest)?;
    let id = flags.single(CONFIG_USAGE)?;
    let set = pairs(&flags)?;
    let unset = flags.all("--unset");
    let cwd = flags.one("--cwd").map(Path::new);
    if set.is_empty() && unset.is_empty() {
        if flags.on("--dry-run") {
            return Err(CtlError::usage("--dry-run needs --set or --unset"));
        }
        let data = with_world(host, |world, runner| {
            plugin_config::read(world, runner, id, cwd).map_err(flagged)
        })?;
        let mut human = String::new();
        for option in data["options"].as_array().into_iter().flatten() {
            human.push_str(&format!(
                "option {:<20} {}\n",
                option["key"].as_str().unwrap_or(""),
                option["current"].as_str().unwrap_or("(not set)")
            ));
        }
        for knob in data["knobs"].as_array().into_iter().flatten() {
            human.push_str(&format!(
                "knob   {:<20} {} ({})\n",
                knob["key"].as_str().unwrap_or(""),
                knob["current"]["value"].as_str().unwrap_or("(not set)"),
                knob["current"]["from"].as_str().unwrap_or("")
            ));
        }
        if human.is_empty() {
            human.push_str("This plugin has no options and no adapter knobs.\n");
        }
        return Ok(Output::new(data, human));
    }
    let data = with_world(host, |world, runner| {
        plugin_config::run(
            world,
            runner,
            &ConfigArgs {
                id,
                cwd,
                set: &set,
                unset: &unset,
                dry_run: flags.on("--dry-run"),
            },
        )
        .map_err(flagged)
    })?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

const MCP_LS_USAGE: &str = "usage: plugins mcp ls <id> [--cwd <dir>]";
const MCP_DENY_USAGE: &str = "usage: plugins mcp deny <id> <server> --cwd <dir> [--dry-run]";
const MCP_ALLOW_USAGE: &str = "usage: plugins mcp allow <id> <server> --cwd <dir> [--dry-run]";

pub(super) const MCP_LS: Spec = Spec {
    flags: &[value("--cwd").needs("a folder")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Max(1, MCP_LS_USAGE),
    ..Spec::PLAIN
};

pub(super) const MCP_DENY: Spec = Spec {
    flags: &[value("--cwd").needs("a folder"), switch("--dry-run")],
    operands: Operands::Max(2, MCP_DENY_USAGE),
    ..MCP_LS
};

pub(super) const MCP_ALLOW: Spec = Spec {
    operands: Operands::Max(2, MCP_ALLOW_USAGE),
    ..MCP_DENY
};

pub fn mcp_ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = MCP_LS.parse(rest)?;
    let id = flags.single(MCP_LS_USAGE)?;
    let data = with_world(host, |world, runner| {
        plugin_config::mcp_read(world, runner, id, flags.one("--cwd").map(Path::new))
            .map_err(flagged)
    })?;
    let mut human = String::new();
    for server in data["servers"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{:<40} {}\n",
            server["key"].as_str().unwrap_or(""),
            if plugin_denied(&server["denied"]) {
                "denied"
            } else {
                "starts"
            }
        ));
    }
    if human.is_empty() {
        human.push_str("This plugin starts no MCP server.\n");
    }
    Ok(Output::new(data, human))
}

fn mcp_write(
    host: &dyn Host,
    rest: &[String],
    spec: &Spec,
    usage: &str,
    op: McpOp,
) -> Result<Output, CtlError> {
    let flags = spec.parse(rest)?;
    let [id, server] = flags.operands() else {
        return Err(CtlError::usage(usage));
    };
    let cwd = flags
        .one("--cwd")
        .ok_or_else(|| CtlError::usage("--cwd is required: a server is denied per folder"))?;
    let data = with_world(host, |world, runner| {
        plugin_config::mcp(
            world,
            runner,
            &McpArgs {
                op,
                id,
                server,
                cwd: Path::new(cwd),
                dry_run: flags.on("--dry-run"),
            },
        )
        .map_err(flagged)
    })?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn mcp_deny(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    mcp_write(host, rest, &MCP_DENY, MCP_DENY_USAGE, McpOp::Deny)
}

pub fn mcp_allow(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    mcp_write(host, rest, &MCP_ALLOW, MCP_ALLOW_USAGE, McpOp::Allow)
}
