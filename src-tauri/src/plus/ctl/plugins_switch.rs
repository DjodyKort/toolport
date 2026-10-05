//! `toolportctl plugins off|on|disable|enable` (D-097): switch one plugin off or on in a folder,
//! or at user scope through Claude Code. Thin renderers over `plus::plugins::switch`.

use super::context_bundle::plan_text;
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use super::plugins::flagged;
use crate::plus::plugins::switch::{disable_enable, off_on, Switch, UserOp};

const OFF_USAGE: &str = "usage: plugins off <id> --cwd <dir> [--dry-run]";
const ON_USAGE: &str = "usage: plugins on <id> --cwd <dir> [--dry-run]";
const DISABLE_USAGE: &str = "usage: plugins disable <id> [--dry-run]";
const ENABLE_USAGE: &str = "usage: plugins enable <id> [--dry-run]";

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

pub(super) const DISABLE: Spec = Spec {
    flags: &[switch("--dry-run")],
    operands: Operands::Max(1, DISABLE_USAGE),
    ..OFF
};

pub(super) const ENABLE: Spec = Spec {
    operands: Operands::Max(1, ENABLE_USAGE),
    ..DISABLE
};

fn folder(rest: &[String], spec: &Spec, usage: &str, op: Switch) -> Result<Output, CtlError> {
    let flags = spec.parse(rest)?;
    let id = flags.single(usage)?;
    let data = off_on(op, id, flags.one("--cwd"), flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

fn user(rest: &[String], spec: &Spec, usage: &str, op: UserOp) -> Result<Output, CtlError> {
    let flags = spec.parse(rest)?;
    let id = flags.single(usage)?;
    let data = disable_enable(op, id, flags.on("--dry-run")).map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn off(rest: &[String]) -> Result<Output, CtlError> {
    folder(rest, &OFF, OFF_USAGE, Switch::Off)
}

pub fn on(rest: &[String]) -> Result<Output, CtlError> {
    folder(rest, &ON, ON_USAGE, Switch::On)
}

pub fn disable(rest: &[String]) -> Result<Output, CtlError> {
    user(rest, &DISABLE, DISABLE_USAGE, UserOp::Disable)
}

pub fn enable(rest: &[String]) -> Result<Output, CtlError> {
    user(rest, &ENABLE, ENABLE_USAGE, UserOp::Enable)
}
