use super::flags::{switch, Inline, Operands, Spec, Unknown};
use super::output::{no_args, CtlError, Output};
use crate::catalog_index::{self, SyncMode};
use serde_json::json;

const GROUP_USAGE: &str = "usage: catalog sync|status (sync: [--full])";
const SYNC_USAGE: &str = "usage: catalog sync [--full]";

pub(super) const SYNC: Spec = Spec {
    flags: &[switch("--full")],
    inline: Inline::Strict,
    unknown: Unknown::ArgumentUsage(SYNC_USAGE),
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let data = catalog_index::status();
    let human = catalog_index::status_text(&data);
    Ok(Output::new(data, human))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SYNC.parse(rest)?;
    let mode = if flags.on("--full") {
        SyncMode::Full
    } else {
        SyncMode::Incremental
    };
    let report = catalog_index::sync(mode, &mut |progress| {
        eprintln!(
            "page {}: {} servers in the index",
            progress.pages, progress.count
        );
    })?;
    let human = format!(
        "Synced the registry index ({}, {} page(s), {} server(s) listed): {} servers in the index.",
        report.kind, report.pages, report.fetched, report.count
    );
    Ok(Output::new(json!(report), human))
}
