//! List prices per million tokens, used only where Claude Code reported no cost (transcripts,
//! a session without a cost metric). Every figure computed here is shown as an estimate.

struct Price {
    family: &'static str,
    input: f64,
    output: f64,
}

const PRICES: &[Price] = &[
    Price {
        family: "opus",
        input: 15.0,
        output: 75.0,
    },
    Price {
        family: "sonnet",
        input: 3.0,
        output: 15.0,
    },
    Price {
        family: "haiku",
        input: 1.0,
        output: 5.0,
    },
];
const FALLBACK: Price = Price {
    family: "",
    input: 3.0,
    output: 15.0,
};
const CACHE_READ_FACTOR: f64 = 0.1;
const CACHE_WRITE_FACTOR: f64 = 1.25;
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
