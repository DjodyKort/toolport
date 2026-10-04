//! `toolportctl plugins ls|show`: the Claude Code plugins that are installed, what each brings
//! and what it costs (D-074, D-075). Thin renderers over `plus::plugins`.

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, ErrorKind, Output};
use crate::plus::plugins::{api, report};

const GROUP_USAGE: &str =
    "usage: plugins ls|show (ls: [--cwd <dir>] [--refresh]; show: <id> [--cwd <dir>])";
const SHOW_USAGE: &str = "usage: plugins show <id> [--cwd <dir>]";

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

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}
