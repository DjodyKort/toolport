//! `toolportctl client direct add|rm|ls|check`: direct client entries, rendered over `direct`,
//! which owns the plan, the write and the registry record.

use toolport_plus_api::Host;

use super::flags::{switch, value, Flags, Spec};
use super::output::{CtlError, Output};
use crate::direct::{self, Args};

const ADD_USAGE: &str =
    "usage: client direct add <server> --client <id> [--force] [--dry-run] [--plan-hash <hash>]";
const RM_USAGE: &str =
    "usage: client direct rm <server> --client <id> [--force] [--dry-run] [--plan-hash <hash>]";
const LS_USAGE: &str = "usage: client direct ls [--client <id>]";
const CHECK_USAGE: &str = "usage: client direct check [--client <id>]";

pub(super) const WRITE: Spec = Spec {
    flags: &[
        value("--client"),
        switch("--force"),
        switch("--dry-run"),
        value("--plan-hash"),
    ],
    ..Spec::PLAIN
};
pub(super) const READ: Spec = Spec {
    flags: &[value("--client")],
    ..Spec::PLAIN
};

fn no_operands(flags: &Flags, usage: &str) -> Result<(), CtlError> {
    match flags.operands() {
        [] => Ok(()),
        _ => Err(CtlError::usage(usage)),
    }
}

fn args_of<'a>(flags: &'a Flags, usage: &str) -> Result<Args<'a>, CtlError> {
    let server = flags.single(usage)?;
    let client = flags
        .one("--client")
        .ok_or_else(|| CtlError::usage(format!("missing --client <id>\n{usage}")))?;
    Ok(Args {
        server,
        client,
        force: flags.on("--force"),
        dry_run: flags.on("--dry-run"),
        plan_hash: flags.one("--plan-hash"),
    })
}

pub fn add(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = WRITE.parse(rest)?;
    let done = direct::add(host, &args_of(&flags, ADD_USAGE)?)?;
    let human = direct::text_of(&done);
    Ok(Output::new(done, human))
}

pub fn rm(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = WRITE.parse(rest)?;
    let done = direct::remove(host, &args_of(&flags, RM_USAGE)?)?;
    let human = direct::text_of(&done);
    Ok(Output::new(done, human))
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = READ.parse(rest)?;
    no_operands(&flags, LS_USAGE)?;
    let rows = direct::list(host, flags.one("--client"))?;
    Ok(Output::new(
        direct::list_value(&rows),
        direct::list_text(&rows),
    ))
}

pub fn check(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = READ.parse(rest)?;
    no_operands(&flags, CHECK_USAGE)?;
    let (rows, healthy) = direct::check(host, flags.one("--client"))?;
    let mut data = direct::list_value(&rows);
    data["healthy"] = serde_json::json!(healthy);
    let human = if healthy {
        format!(
            "{}\nAll direct entries are in sync.",
            direct::list_text(&rows)
        )
    } else {
        let hints: Vec<String> = rows
            .iter()
            .filter_map(|row| {
                row.state.action().map(|action| {
                    format!(
                        "{} in {}: {} ({action})",
                        row.entry,
                        row.client,
                        row.state.as_str()
                    )
                })
            })
            .collect();
        format!("{}\n{}", direct::list_text(&rows), hints.join("\n"))
    };
    let mut output = Output::new(data, human);
    output.failed = !healthy;
    Ok(output)
}
