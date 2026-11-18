//! `toolportctl client reach`: which access sets and clients reach each server. Read-only; it
//! renders `reach`, which resolves the sets the way the gateway does.

use toolport_plus_api::Host;

use super::flags::{value, Spec};
use super::output::{CtlError, Output};
use crate::reach::{self, Filter};

const USAGE: &str = "usage: client reach [--server <id|name>] [--client <id>]";

pub(super) const REACH: Spec = Spec {
    flags: &[value("--server"), value("--client")],
    ..Spec::PLAIN
};

pub fn run(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = REACH.parse(rest)?;
    if !flags.operands().is_empty() {
        return Err(CtlError::usage(USAGE));
    }
    let report = reach::assess(
        host,
        Filter {
            server: flags.one("--server"),
            client: flags.one("--client"),
        },
    )?;
    Ok(Output::new(report.to_value(), report.to_text()))
}
