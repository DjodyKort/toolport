//! Model spend from Claude Code's OpenTelemetry export (S10). Everything here is fork-owned:
//! upstream has no OTLP code. Upstream's savings rows and tokenizer are read through the host
//! traits only.

pub mod config;
pub mod event;
pub mod mcp_part;
pub mod otlp;
pub mod prices;
pub mod receiver;
pub mod report;
pub mod runtime;
pub mod sample;
pub mod setup;
pub mod store;
pub mod transcript;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 4318;
pub const DEFAULT_RETENTION_DAYS: u32 = 30;
pub const DEFAULT_MAX_MB: u32 = 64;
pub const RETENTION_DAYS_RANGE: (u32, u32) = (7, 90);
pub const MAX_MB_RANGE: (u32, u32) = (16, 256);
pub const MS_PER_DAY: i64 = 86_400_000;

pub fn spend_dir(base: &Path) -> PathBuf {
    base.join("plus").join("spend")
}

pub fn now_ms() -> i64 {
    crate::compression::ledger::now_ms()
}

/// `2026-11-18` for a Unix time in milliseconds, in UTC.
pub fn day_of(ms: i64) -> String {
    let (y, m, d) = crate::tasks::cron::civil_from_days(ms.div_euclid(MS_PER_DAY));
    format!("{y:04}-{m:02}-{d:02}")
}

pub use crate::compression::ledger::{format_ts, parse_ts};

pub fn day_start_ms(day: &str) -> Option<i64> {
    let mut parts = day.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    Some(crate::tasks::cron::days_from_civil(y, m, d) * MS_PER_DAY)
}
