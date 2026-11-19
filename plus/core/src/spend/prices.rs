//! List prices per million tokens, used only where Claude Code reported no cost (transcripts,
//! a session without a cost metric). Every figure computed here is shown as an estimate.
//!
//! None of these numbers has a source in the repository or the specs: the design says "list
//! prices" and the migration notes call the cache multipliers "confirm before use". Until
//! someone checks them against Anthropic's published pricing and records where, every price is
//! marked unverified in the data and on the page.

use serde_json::{json, Value};

struct Price {
    family: &'static str,
    input: f64,
    output: f64,
    /// Checked against Anthropic's published pricing, with the source named where it is set.
    verified: bool,
}

const PRICES: &[Price] = &[
    Price {
        family: "opus",
        input: 15.0,
        output: 75.0,
        verified: false,
    },
    Price {
        family: "sonnet",
        input: 3.0,
        output: 15.0,
        verified: false,
    },
    Price {
        family: "haiku",
        input: 1.0,
        output: 5.0,
        verified: false,
    },
];
const FALLBACK: Price = Price {
    family: "",
    input: 3.0,
    output: 15.0,
    verified: false,
};
const CACHE_READ_FACTOR: f64 = 0.1;
const CACHE_WRITE_FACTOR: f64 = 1.25;
const FACTORS_VERIFIED: bool = false;
const PER_TOKENS: f64 = 1_000_000.0;

pub fn cost(
    model: Option<&str>,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
) -> f64 {
    let lower = model.unwrap_or("").to_ascii_lowercase();
    let price = PRICES
        .iter()
        .find(|p| lower.contains(p.family))
        .unwrap_or(&FALLBACK);
    (input as f64 * price.input
        + output as f64 * price.output
        + cache_read as f64 * price.input * CACHE_READ_FACTOR
        + cache_write as f64 * price.input * CACHE_WRITE_FACTOR)
        / PER_TOKENS
}

/// The table behind every estimated cost, with whether each part was checked, for the report.
pub fn table() -> Value {
    json!({
        "verified": FACTORS_VERIFIED && PRICES.iter().all(|p| p.verified),
        "unit": "usd per million tokens",
        "families": PRICES.iter().map(|p| json!({
            "family": p.family,
            "inputUsd": p.input,
            "outputUsd": p.output,
            "verified": p.verified,
        })).collect::<Vec<_>>(),
        "cacheReadFactor": CACHE_READ_FACTOR,
        "cacheWriteFactor": CACHE_WRITE_FACTOR,
        "factorsVerified": FACTORS_VERIFIED,
    })
}
