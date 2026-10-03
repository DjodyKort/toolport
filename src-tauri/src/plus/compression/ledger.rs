//! Append-only records of what this machine did: which launches routed through a provider
//! (so `verify` can split transcripts into proxied and plain) and per-provider token
//! savings entries. Neither file is synced; they describe this machine's runtime only.

use super::launch::LedgerEntry;
use super::store::Paths;
use crate::registry;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;

/// How long after a launch a session may start and still be attributed to it.
pub const MATCH_WINDOW_MS: i64 = 90_000;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn format_ts(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, m, d) = crate::usage_report::civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        ms.rem_euclid(1000)
    )
}

fn digits(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Epoch milliseconds for an RFC 3339 timestamp; a missing zone means UTC. Fields out of
/// range (February 31, hour 24, minute 60) are rejected the way Python's `fromisoformat` does.
pub fn parse_ts(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |range: std::ops::Range<usize>| digits(text.get(range)?);
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut rest = &text[19..];
    let mut millis = 0;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
        let padded = format!("{digits:0<3}");
        millis = padded.get(..3)?.parse::<i64>().ok()?;
        rest = &frac[digits.len()..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        tz if tz.len() == 6
            && matches!(tz.as_bytes()[0], b'+' | b'-')
            && tz.as_bytes()[3] == b':' =>
        {
            let (hours, minutes) = (digits(tz.get(1..3)?)?, digits(tz.get(4..6)?)?);
            if hours > 23 || minutes > 59 {
                return None;
            }
            let secs = hours * 3600 + minutes * 60;
            if tz.starts_with('-') {
                -secs
            } else {
                secs
            }
        }
        _ => return None,
    };
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + hour * 3600 + minute * 60 + second - offset) * 1000 + millis)
}

/// Both sides of a cwd match must spell the directory the same way, symlinks resolved.
pub fn norm_cwd(path: &str) -> String {
    match std::fs::canonicalize(path) {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => {
            let trimmed = path.trim_end_matches('/');
            if trimmed.is_empty() && path.starts_with('/') {
                "/".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchRecord {
    pub ts: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub routed: bool,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub pin: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavingsEntry {
    pub ts: String,
    pub provider: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub session: Option<String>,
    pub tokens_before: u64,
    pub tokens_after: u64,
}

fn token_gap(before: u64, after: u64) -> i64 {
    (i128::from(before) - i128::from(after)).clamp(i128::from(i64::MIN), i128::from(i64::MAX))
        as i64
}

impl SavingsEntry {
    pub fn saved(&self) -> i64 {
        token_gap(self.tokens_before, self.tokens_after)
    }
}

fn append_line<T: Serialize>(path: &Path, record: &T) -> Result<(), String> {
    let mut line = serde_json::to_string(record).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = registry::open_append_private(path).map_err(|e| e.to_string())?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

/// Tolerates a torn final line and any record that does not parse.
fn read_lines<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str::<T>(line).ok())
        .collect()
}

pub fn append_launch(paths: &Paths, entry: &LedgerEntry, now: i64) -> Result<(), String> {
    let record = LaunchRecord {
        ts: format_ts(now),
        cwd: norm_cwd(&entry.cwd),
        provider: entry.provider.as_str().to_string(),
        preset: entry.preset.clone(),
        routed: entry.routed,
        port: entry.port,
        pin: Some(entry.pin.clone()),
    };
    append_line(&paths.launches(), &record)
}

pub fn read_launches(paths: &Paths) -> Vec<LaunchRecord> {
    read_lines::<LaunchRecord>(&paths.launches())
        .into_iter()
        .filter(|r| !r.ts.is_empty())
        .collect()
}

pub fn append_savings(paths: &Paths, entry: &SavingsEntry) -> Result<(), String> {
    append_line(&paths.savings(), entry)
}

pub fn read_savings(paths: &Paths) -> Vec<SavingsEntry> {
    read_lines(&paths.savings())
}

/// The launch a session belongs to, or `None`: a session started some other way is
/// unattributed, never guessed into a bucket that would flatter the result.
pub fn attribute<'a>(
    start_ms: Option<i64>,
    cwd: Option<&str>,
    launches: &'a [LaunchRecord],
) -> Option<&'a LaunchRecord> {
    Attributor::new(launches).attribute(start_ms, cwd)
}

/// [`attribute`] over many sessions: launch directories are normalized once and grouped,
/// and each distinct session directory is normalized once.
pub struct Attributor<'a> {
    launches: &'a [LaunchRecord],
    by_cwd: Option<HashMap<String, Vec<(i64, &'a LaunchRecord)>>>,
    targets: HashMap<String, String>,
}

impl<'a> Attributor<'a> {
    pub fn new(launches: &'a [LaunchRecord]) -> Self {
        Attributor {
            launches,
            by_cwd: None,
            targets: HashMap::new(),
        }
    }

    pub fn attribute(&mut self, start_ms: Option<i64>, cwd: Option<&str>) -> Option<&'a LaunchRecord> {
        let (start, cwd) = (start_ms?, cwd.filter(|c| !c.is_empty())?);
        let launches = self.launches;
        let by_cwd = self.by_cwd.get_or_insert_with(|| {
            let mut index: HashMap<String, Vec<(i64, &LaunchRecord)>> = HashMap::new();
            for rec in launches {
                if let Some(ts) = parse_ts(&rec.ts) {
                    index.entry(norm_cwd(&rec.cwd)).or_default().push((ts, rec));
                }
            }
            index
        });
        let target = self
            .targets
            .entry(cwd.to_string())
            .or_insert_with(|| norm_cwd(cwd));
        by_cwd
            .get(target.as_str())?
            .iter()
            .filter(|(ts, _)| (0..=MATCH_WINDOW_MS).contains(&(start - ts)))
            .min_by_key(|(ts, _)| std::cmp::Reverse(*ts))
            .map(|(_, rec)| *rec)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub provider: String,
    pub launches: u64,
    pub routed: u64,
    pub plain: u64,
    pub savings_entries: u64,
    pub tokens_before: u64,
    pub tokens_after: u64,
}

impl ProviderSummary {
    pub fn saved(&self) -> i64 {
        token_gap(self.tokens_before, self.tokens_after)
    }

    pub fn saved_percent(&self) -> Option<f64> {
        (self.tokens_before > 0).then(|| self.saved() as f64 * 100.0 / self.tokens_before as f64)
    }
}

pub fn summarize(
    launches: &[LaunchRecord],
    savings: &[SavingsEntry],
    provider: Option<&str>,
    since_ms: Option<i64>,
) -> Vec<ProviderSummary> {
    let keep = |ts: &str, name: &str| {
        provider.is_none_or(|p| p == name)
            && since_ms.is_none_or(|s| parse_ts(ts).is_some_and(|t| t >= s))
    };
    let mut rows: BTreeMap<String, ProviderSummary> = BTreeMap::new();
    fn slot<'a>(
        rows: &'a mut BTreeMap<String, ProviderSummary>,
        name: &str,
    ) -> &'a mut ProviderSummary {
        rows.entry(name.to_string())
            .or_insert_with(|| ProviderSummary {
                provider: name.to_string(),
                ..ProviderSummary::default()
            })
    }
    for rec in launches.iter().filter(|r| keep(&r.ts, &r.provider)) {
        let s = slot(&mut rows, &rec.provider);
        s.launches += 1;
        if rec.routed {
            s.routed += 1;
        } else {
            s.plain += 1;
        }
    }
    for entry in savings.iter().filter(|e| keep(&e.ts, &e.provider)) {
        let s = slot(&mut rows, &entry.provider);
        s.savings_entries += 1;
        s.tokens_before = s.tokens_before.saturating_add(entry.tokens_before);
        s.tokens_after = s.tokens_after.saturating_add(entry.tokens_after);
    }
    rows.into_values().collect()
}
