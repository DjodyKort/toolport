//! `toolportctl hooks ls`: every hook Claude Code would start in a folder, read from files and
//! never run (D-074). Thin renderer over `plus::plugins::hooks`.

use super::flags::{value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::plugins::{api, hooks};

const GROUP_USAGE: &str =
    "usage: hooks ls [--cwd <dir>] [--tool Bash|Edit|Write|Read] [--event <name>] [--owner <kind>]";

pub(super) const LS: Spec = Spec {
    flags: &[
        value("--cwd"),
        value("--tool"),
        value("--event"),
        value("--owner"),
    ],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let filter = hooks::Filter {
        tool: flags.one("--tool").map(str::to_string),
        event: flags.one("--event").map(str::to_string),
        owner: flags.one("--owner").map(str::to_string),
    };
    let data = api::hooks_ls(flags.one("--cwd"), &filter).map_err(super::plugins::flagged)?;
    let human = hooks::to_text(&data);
    Ok(Output::new(data, human))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}
