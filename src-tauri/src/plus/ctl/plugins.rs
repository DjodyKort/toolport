//! `toolportctl plugins ls|show`: the Claude Code plugins that are installed, what each brings
//! and what it costs (D-074, D-075). Thin renderers over `plus::plugins`.

use super::context_bundle::plan_text;
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, ErrorKind, Output};
use crate::plus::plugins::config::McpOp;
use crate::plus::plugins::{api, report};

const GROUP_USAGE: &str = "usage: plugins ls|show|config|mcp|off|on|disable|enable (ls: [--cwd <dir>] [--refresh]; show: <id> [--cwd <dir>]; config: <id> [--cwd <dir>] [--set <knob>=<value>]... [--unset <knob>]... [--dry-run]; mcp: deny|allow <id> <server> --cwd <dir> [--dry-run]; off|on: <id> --cwd <dir> [--dry-run]; disable|enable: <id> [--dry-run])";
const SHOW_USAGE: &str = "usage: plugins show <id> [--cwd <dir>]";
const CONFIG_USAGE: &str = "usage: plugins config <id> [--cwd <dir>] [--set <knob>=<value>]... [--unset <knob>]... [--dry-run]";
const MCP_USAGE: &str = "usage: plugins mcp deny|allow <id> <server> --cwd <dir> [--dry-run]";

pub(super) const LS: Spec = Spec {
    flags: &[value("--cwd"), switch("--refresh")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const SHOW: Spec = Spec {
    flags: &[value("--cwd")],
    operands: Operands::Max(1, SHOW_USAGE),
    ..LS
};

pub(super) const CONFIG: Spec = Spec {
    flags: &[
        value("--cwd"),
        value("--set"),
        value("--unset"),
        switch("--dry-run"),
    ],
    operands: Operands::Max(1, CONFIG_USAGE),
    ..LS
};

pub(super) const MCP: Spec = Spec {
    flags: &[value("--cwd"), switch("--dry-run")],
    operands: Operands::Max(3, MCP_USAGE),
    ..LS
};

/// The shared operations name a bad argument (`cwd is not a folder`); the command line calls it
/// by its flag.
pub(super) fn flagged(err: CtlError) -> CtlError {
    let named = ["cwd ", "tool ", "owner ", "event "]
        .iter()
        .any(|name| err.message.starts_with(name));
    if err.kind == ErrorKind::Usage && named {
        CtlError::usage(format!("--{}", err.message))
    } else {
        err
    }
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let data = api::ls(flags.one("--cwd"), flags.on("--refresh")).map_err(flagged)?;
    let human = report::ls_text(&data);
    Ok(Output::new(data, human))
}

pub fn show(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SHOW.parse(rest)?;
    let id = flags.single(SHOW_USAGE)?;
    let data = api::show(id, flags.one("--cwd")).map_err(flagged)?;
    let human = report::show_text(&data);
    Ok(Output::new(data, human))
}

pub fn config(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CONFIG.parse(rest)?;
    let id = flags.single(CONFIG_USAGE)?;
    let sets = flags
        .all("--set")
        .into_iter()
        .map(|pair| match pair.split_once('=') {
            Some((knob, value)) if !knob.is_empty() => Ok((knob.to_string(), value.to_string())),
            _ => Err(CtlError::usage(format!(
                "--set needs <knob>=<value>, got '{pair}'"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let data = api::config(id, flags.one("--cwd"), sets, flags.all("--unset"), flags.on("--dry-run"))
        .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn mcp(rest: &[String]) -> Result<Output, CtlError> {
    let flags = MCP.parse(rest)?;
    let [action, id, server] = flags.operands() else {
        return Err(CtlError::usage(MCP_USAGE));
    };
    let op = match action.as_str() {
        "deny" => McpOp::Deny,
        "allow" => McpOp::Allow,
        _ => return Err(CtlError::usage(MCP_USAGE)),
    };
    let data = api::mcp(op, id, server, flags.one("--cwd"), flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}
