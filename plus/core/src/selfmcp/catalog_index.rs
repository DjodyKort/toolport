//! `catalog_status` and `catalog_search` (tier 1), `catalog_sync` (tier 3): the same operations as
//! `toolportctl catalog` over `crate::catalog_index`.

use super::ToolError;
use crate::args::{flag_or, str_nonempty};
use crate::catalog_index::{self, Mode};
use serde_json::{json, Value};
use toolport_plus_api::Host;

/// Pages one call fetches: a first sync lists well over a hundred, and a call that stops here
/// leaves the pass resumable, so the next call carries on where it stopped.
const PAGES_PER_CALL: u32 = 20;

const SEARCH_ROWS: usize = 50;

pub(super) fn status(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(json!(catalog_index::status(host)))
}

pub(super) fn search(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let query = str_nonempty(args, "query")
        .ok_or_else(|| ToolError::new("invalid_arguments", "query is required"))?;
    let hits = catalog_index::search(host, query);
    let rows: Vec<Value> = hits
        .iter()
        .take(SEARCH_ROWS)
        .map(|entry| {
            json!({
                "name": entry.name,
                "transport": entry.transport,
                "source": entry.source,
                "envKeys": entry.env_keys,
                "description": entry.text("description"),
                "installable": crate::servers::find_catalog(host, &entry.name).is_some(),
            })
        })
        .collect();
    Ok(json!({"query": query, "matches": hits.len(), "servers": rows}))
}

pub(super) fn sync(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let mode = if crate::args::flag(args, "full") {
        Mode::Full
    } else {
        Mode::Incremental
    };
    let status = catalog_index::status(host);
    if flag_or(args, "dry_run", true) {
        return Ok(json!({
            "dryRun": true,
            "mode": if mode == Mode::Full { "full" } else if status.resumable { "resume" } else { "incremental" },
            "status": status,
            "note": format!("applying fetches at most {PAGES_PER_CALL} pages of the MCP Registry from the network; an unfinished pass is continued by the next call"),
        }));
    }
    let report = catalog_index::sync_bounded(host, mode, PAGES_PER_CALL, &mut |_| {})?;
    Ok(json!({"dryRun": false, "report": report, "status": catalog_index::status(host)}))
}
