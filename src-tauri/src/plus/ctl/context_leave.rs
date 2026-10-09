//! `toolportctl context leave-out add|rm`: leave one CLAUDE.md or rule out of a folder, or turn
//! auto memory off there. A thin renderer over `plus::context::leave_out`.

use super::context_bundle::plan_text;
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use super::plugins::flagged;
use crate::plus::context::leave_out::{api, Op, Target};

const USAGE: &str = "usage: context leave-out add|rm <path> --cwd <dir> [--dry-run] | add|rm --memory --cwd <dir> [--dry-run]";

pub(super) const LEAVE_OUT: Spec = Spec {
    flags: &[value("--cwd"), switch("--memory"), switch("--dry-run")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Max(2, USAGE),
    ..Spec::PLAIN
};

pub fn leave_out(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LEAVE_OUT.parse(rest)?;
    let (action, target) = match (flags.operands(), flags.on("--memory")) {
        ([action], true) => (action, Target::Memory),
        ([action, path], false) => (action, Target::File(path.clone())),
        _ => return Err(CtlError::usage(USAGE)),
    };
    let op = match action.as_str() {
        "add" => Op::Add,
        "rm" => Op::Rm,
        _ => return Err(CtlError::usage(USAGE)),
    };
    let data = api(op, target, flags.one("--cwd"), flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}
