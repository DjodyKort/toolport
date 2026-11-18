//! The MCP part card: tool definitions per session from upstream's savings rows (Counted, with
//! upstream's tokenizer) and their share of the input-side tokens (an Estimate: billing does not
//! split a request into parts).

use std::path::Path;

use serde_json::{json, Value};
use toolport_plus_api::SavingsAccess;

use crate::compression::{hooks, ledger, store::Paths};
use crate::context::measure;

use super::report::{BADGE_COUNTED, BADGE_ESTIMATE, BADGE_MEASURED};

const KIND_EXPOSURE: &str = "catalog_exposure";
const TOKENIZER: &str = "cl100k_base";

fn median(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

/// Hook calls the wrapper counted, and the tokens the savings ledger records as cut. A hook
/// cannot see the size of what a rewrite saves, so without ledger entries only calls are shown.
fn compression(base: &Path) -> Value {
    let paths = Paths::new(base);
    let (mut hooks_seen, mut calls) = (0u64, 0u64);
    let (mut first, mut last): (Option<String>, Option<String>) = (None, None);
    for provider in hooks::HOOK_PROVIDERS {
        let summary = hooks::summarize(&paths, provider.id);
        if summary.calls == 0 {
            continue;
        }
        hooks_seen += 1;
        calls += summary.calls as u64;
        first = [first.take(), summary.first].into_iter().flatten().min();
        last = [last.take(), summary.last].into_iter().flatten().max();
    }
    let savings = ledger::read_savings(&paths);
    let cut: i64 = savings.iter().map(|e| e.saved().max(0)).sum();
    json!({
        "hooks": hooks_seen,
        "calls": calls,
        "firstCall": first,
        "lastCall": last,
        "tokensCut": (!savings.is_empty()).then_some(cut),
        "badge": BADGE_COUNTED,
        "note": hooks::CANNOT_TELL,
    })
}

pub fn part(host: &dyn SavingsAccess, base: &Path, since_ms: i64, input_side_tokens: u64) -> Value {
    let rows = match host.catalog_sessions() {
        Ok(rows) => rows,
        Err(error) => return json!({"state": "error", "error": error}),
    };
    let mut sessions = 0u64;
    let mut exposed = 0u64;
    let mut full = 0u64;
    let (mut exposed_each, mut full_each) = (Vec::new(), Vec::new());
    for row in rows
        .iter()
        .filter(|r| r["kind"] == KIND_EXPOSURE)
        .filter(|r| r["ts"].as_i64().unwrap_or(0) >= since_ms)
    {
        sessions += 1;
        let (e, f) = (
            row["exposedTokens"].as_u64().unwrap_or(0),
            row["fullTokens"].as_u64().unwrap_or(0),
        );
        exposed += e;
        full += f;
        exposed_each.push(e);
        full_each.push(f);
    }
    let share = (input_side_tokens > 0 && exposed > 0)
        .then(|| (exposed as f64 / input_side_tokens as f64 * 10_000.0).round() / 10_000.0);
    json!({
        "state": if sessions == 0 { "empty" } else { "ready" },
        "toolDefinitions": {
            "sessions": sessions,
            "exposedTokens": exposed,
            "fullTokens": full,
            "medianExposedTokens": median(&mut exposed_each),
            "medianFullTokens": median(&mut full_each),
            "tokenizer": TOKENIZER,
            "badge": BADGE_COUNTED,
        },
        "shareOfInputTokens": {"value": share, "badge": BADGE_ESTIMATE},
        "savedTokens": host.catalog_summary().ok().and_then(|s| s.get("tokensSaved").cloned()),
        "compression": compression(base),
        "firstRequest": measure::newest_measured(base).map(|mut m| {
            m["badge"] = json!(BADGE_MEASURED);
            m
        }),
        "note": "Upstream's saved tokens cover tool-definition exposure only and are never added to spend",
    })
}
