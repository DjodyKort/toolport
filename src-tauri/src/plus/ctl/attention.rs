//! `toolportctl attention ls|dismiss`: the one list of what wants a decision (MIG-GUI-11). Thin
//! renderers over `plus::attention`, which the self-MCP tool `attention_ls` calls as well.

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::attention::{self, dismissals, Ctx, Level};
use serde_json::Value;

const GROUP_USAGE: &str = "usage: attention ls|dismiss (ls: [--level needs-you|look|fyi]; dismiss: <id> [--until <YYYY-MM-DD>] [--dry-run])";
const LS_USAGE: &str = "usage: attention ls [--level needs-you|look|fyi]";
const DISMISS_USAGE: &str = "usage: attention dismiss <id> [--until <YYYY-MM-DD>] [--dry-run]";

pub(super) const LS: Spec = Spec {
    flags: &[value("--level").needs("needs-you, look or fyi")],
    inline: Inline::Strict,
    unknown: Unknown::ArgumentUsage(LS_USAGE),
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const DISMISS: Spec = Spec {
    flags: &[value("--until").needs("a date like 2026-10-31"), switch("--dry-run")],
    operands: Operands::Max(1, DISMISS_USAGE),
    unknown: Unknown::ArgumentUsage(DISMISS_USAGE),
    ..LS
};

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn parse_level(text: Option<&str>) -> Result<Option<Level>, CtlError> {
    text.map(|t| Level::parse(t).ok_or_else(|| CtlError::usage(format!("--level must be one of {}", Level::NAMES.join(", ")))))
        .transpose()
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let level = parse_level(flags.one("--level"))?;
    let data = attention::ls_host(level)?;
    let human = attention::human(&data);
    Ok(Output::new(data, human))
}

pub fn dismiss(rest: &[String]) -> Result<Output, CtlError> {
    let flags = DISMISS.parse(rest)?;
    let id = flags.single(DISMISS_USAGE)?;
    let data = dismissals::dismiss(id, flags.one("--until"), Ctx::system().now, flags.on("--dry-run"))?;
    let human = if data["dryRun"] == Value::Bool(true) {
        format!("{}\n(dry run, nothing written)\n", data["plan"]["summary"].as_str().unwrap_or(""))
    } else {
        format!("Done.\nundo: {}\n", data["result"]["undo"].as_str().unwrap_or(""))
    };
    Ok(Output::new(data, human))
}
