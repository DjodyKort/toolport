//! `toolportctl attention ls`: everything that needs the person right now, from `crate::attention`,
//! which the self-MCP tool `attention_list` and the app's footer bell read as well.

use toolport_plus_api::Host;

use super::flags::{Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::attention;

pub(super) const LS: Spec = Spec {
    flags: &[],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    LS.parse(rest)?;
    let snapshot = attention::snapshot(host);
    Ok(Output::new(snapshot.to_json(), attention::human(&snapshot)))
}
