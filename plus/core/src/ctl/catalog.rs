//! `toolportctl catalog status|sync`: the local index of the MCP Registry (D-118). Thin renderers
//! over `crate::catalog_index`, which the self-MCP tool `catalog_status` calls as well.

use super::flags::{switch, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::catalog_index::{self, Mode, Status};
use serde_json::json;
use toolport_plus_api::Host;

pub(super) const STATUS: Spec = Spec {
    flags: &[],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const SYNC: Spec = Spec {
    flags: &[switch("--full")],
    ..STATUS
};

pub(crate) fn human(status: &Status) -> String {
    let synced = match &status.last_synced_at {
        Some(at) => format!("synced {at}"),
        None => "never synced".to_string(),
    };
    let mut out = format!(
        "Registry index: {} servers, {synced} ({})\n",
        status.count, status.state
    );
    if status.resumable {
        out.push_str("An earlier sync stopped part-way; the next one continues it\n");
    }
    if let Some(error) = &status.error {
        out.push_str(&format!(
            "Last attempt failed ({}): {}\n",
            error.kind, error.message
        ));
    }
    if !status.background {
        out.push_str("Background sync is off (TOOLPORT_CATALOG_INDEX_SYNC)\n");
    }
    out
}

pub fn status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    STATUS.parse(rest)?;
    let status = catalog_index::status(host);
    let human = human(&status);
    Ok(Output::new(json!(status), human))
}

pub fn sync(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SYNC.parse(rest)?;
    let mode = if flags.on("--full") {
        Mode::Full
    } else {
        Mode::Incremental
    };
    let mut pages = 0;
    let report = catalog_index::sync(host, mode, &mut |report| {
        pages += 1;
        if pages % 10 == 0 {
            eprintln!("{} pages, {} servers", report.pages, report.count);
        }
    })?;
    let human = format!(
        "Synced the registry index ({}): {} pages, {} servers\n",
        report.kind, report.pages, report.count
    );
    let status = catalog_index::status(host);
    Ok(Output::new(
        json!({"report": report, "status": status}),
        human,
    ))
}
