//! `toolportctl client direct add|rm|ls` and `toolportctl direct run`: renderers over
//! `plus::direct`, which owns the writes, the registry record and the launcher.

use super::flags::{switch, value, Flags, Spec};
use super::output::{CtlError, Output};
use super::profile::ctl_error;
use super::skills_repo::{no_operands, operand, spec};
use crate::plus::direct;

const ADD_USAGE: &str = "usage: client direct add <server> --client <id> [--force] [--dry-run]";
const RM_USAGE: &str = "usage: client direct rm <server> --client <id> [--force] [--dry-run]";
const LS_USAGE: &str = "usage: client direct ls [--client <id>]";
const RUN_USAGE: &str = "usage: direct run <server id>";

pub const GROUP_USAGE: &str = "usage: client direct add|rm|ls (add, rm: <server> --client <id> \
     [--force] [--dry-run]; ls: [--client <id>])";
pub const RUN_GROUP_USAGE: &str = "usage: direct run <server id> (the stdio launcher a direct \
     client entry starts; added with `client direct add`)";

pub(super) const ADD: Spec = spec(
    &[value("--client"), switch("--force"), switch("--dry-run")],
    ADD_USAGE,
);
pub(super) const RM: Spec = spec(
    &[value("--client"), switch("--force"), switch("--dry-run")],
    RM_USAGE,
);
pub(super) const LS: Spec = spec(&[value("--client")], LS_USAGE);
pub(super) const RUN: Spec = spec(&[], RUN_USAGE);

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn run_group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(RUN_GROUP_USAGE))
}

fn client_of<'a>(flags: &'a Flags, usage: &str) -> Result<&'a str, CtlError> {
    flags
        .one("--client")
        .ok_or_else(|| CtlError::usage(format!("missing --client <id>\n{usage}")))
}

pub fn add(rest: &[String]) -> Result<Output, CtlError> {
    let flags = ADD.parse(rest)?;
    let server = operand(&flags, "server", ADD_USAGE)?;
    let client = client_of(&flags, ADD_USAGE)?;
    let outcome = direct::add(server, client, flags.on("--force"), flags.on("--dry-run"))
        .map_err(ctl_error)?;
    Ok(Output::new(outcome.to_value(), outcome.text()))
}

pub fn rm(rest: &[String]) -> Result<Output, CtlError> {
    let flags = RM.parse(rest)?;
    let server = operand(&flags, "server", RM_USAGE)?;
    let client = client_of(&flags, RM_USAGE)?;
    let outcome = direct::remove(server, client, flags.on("--force"), flags.on("--dry-run"))
        .map_err(ctl_error)?;
    Ok(Output::new(outcome.to_value(), outcome.text()))
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    no_operands(&flags, LS_USAGE)?;
    let rows = direct::list(flags.one("--client")).map_err(ctl_error)?;
    Ok(Output::new(
        direct::list_value(&rows),
        direct::list_text(&rows),
    ))
}

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let flags = RUN.parse(rest)?;
    let key = operand(&flags, "server", RUN_USAGE)?;
    let failure = direct::launcher::run(key);
    if failure.exit == 1 {
        return Err(CtlError::failed("direct_run", failure.message));
    }
    eprintln!("toolportctl: {}", failure.message);
    std::process::exit(failure.exit)
}
