//! The MCP part card: tool definitions per session from upstream's savings rows (Counted, with
//! upstream's tokenizer) and their share of the input-side tokens (an Estimate: billing does not
//! split a request into parts).

use serde_json::{json, Value};
use toolport_plus_api::SavingsAccess;

use super::report::{BADGE_COUNTED, BADGE_ESTIMATE};

const KIND_EXPOSURE: &str = "catalog_exposure";
const TOKENIZER: &str = "cl100k_base";

pub fn part(host: &dyn SavingsAccess, since_ms: i64, input_side_tokens: u64) -> Value {
    let rows = match host.catalog_sessions() {
        Ok(rows) => rows,
        Err(error) => return json!({"state": "error", "error": error}),
    };
    let mut sessions = 0u64;
    let mut exposed = 0u64;
    let mut full = 0u64;
    for row in rows
        .iter()
        .filter(|r| r["kind"] == KIND_EXPOSURE)
        .filter(|r| r["ts"].as_i64().unwrap_or(0) >= since_ms)
    {
        sessions += 1;
        exposed += row["exposedTokens"].as_u64().unwrap_or(0);
        full += row["fullTokens"].as_u64().unwrap_or(0);
    }
    let share = (input_side_tokens > 0 && exposed > 0)
        .then(|| (exposed as f64 / input_side_tokens as f64 * 10_000.0).round() / 10_000.0);
    json!({
        "state": if sessions == 0 { "empty" } else { "ready" },
        "toolDefinitions": {
            "sessions": sessions,
            "exposedTokens": exposed,
            "fullTokens": full,
            "tokenizer": TOKENIZER,
            "badge": BADGE_COUNTED,
        },
        "shareOfInputTokens": {"value": share, "badge": BADGE_ESTIMATE},
        "savedTokens": host.catalog_summary().ok().and_then(|s| s.get("tokensSaved").cloned()),
        "note": "Upstream's saved tokens cover tool-definition exposure only and are never added to spend",
    })
}
