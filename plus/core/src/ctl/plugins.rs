//! `toolportctl plugins off|on`: switch one plugin off or on in a folder (D-097, D-136). Thin
//! renderers over `context::ops`.

use super::context::{flagged, plan_text};
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::context::ops as api;
use crate::context::plugin_switch::Switch;
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
