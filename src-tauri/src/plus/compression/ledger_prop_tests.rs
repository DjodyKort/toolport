//! Seeded randomized and edge-case tests for the compression ledgers: RFC 3339 timestamps,
//! torn and corrupt JSONL files, launch attribution and the per-provider summary.

use super::launch::LedgerEntry;
use super::ledger::*;
use super::model::ProviderName;
use super::store::Paths;
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;

const MIN_MS: i64 = -62_167_219_200_000;
const MAX_MS: i64 = 253_402_300_799_999;

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn naive_days(y: i64, m: i64, d: i64) -> i64 {
    let mut days = 0;
    if y >= 1970 {
        for year in 1970..y {
            days += if is_leap(year) { 366 } else { 365 };
        }
    } else {
        for year in y..1970 {
            days -= if is_leap(year) { 366 } else { 365 };
        }
    }
    for month in 1..m {
        days += days_in_month(y, month);
    }
    days + d - 1
}

#[test]
fn timestamps_round_trip_and_keep_a_fixed_width() {
    let shape = Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$").unwrap();
    run_cases("ledger-ts-roundtrip", 6000, |_, rng| {
        let ms = match rng.below(4) {
            0 => MIN_MS + rng.below(1_000_000_000) as i64,
            1 => MAX_MS - rng.below(1_000_000_000) as i64,
            2 => rng.below(4_000_000_000_000) as i64,
            _ => MIN_MS + (rng.next_u64() % (MAX_MS - MIN_MS) as u64) as i64,
        };
        let text = format_ts(ms);
        assert!(shape.is_match(&text), "{text}");
        assert_eq!(parse_ts(&text), Some(ms), "{text}");
        let later = (ms + 1 + rng.below(100_000_000) as i64).min(MAX_MS);
        if later > ms {
            assert!(format_ts(later) > text, "lexical order follows time");
        }
    });
    assert_eq!(format_ts(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(format_ts(-1), "1969-12-31T23:59:59.999Z");
    assert_eq!(format_ts(1_767_225_600_000), "2026-01-01T00:00:00.000Z");
    assert_eq!(format_ts(1_709_210_096_123), "2024-02-29T12:34:56.123Z");
    assert_eq!(format_ts(MIN_MS), "0000-01-01T00:00:00.000Z");
    assert_eq!(format_ts(MAX_MS), "9999-12-31T23:59:59.999Z");
}

#[test]
fn extreme_millisecond_values_do_not_panic() {
    for ms in [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX] {
        let text = format_ts(ms);
        let _ = parse_ts(&text);
    }
    assert_eq!(parse_ts(&format_ts(i64::MAX)), None, "year beyond 9999");
}

#[test]
fn parsing_matches_a_loop_based_calendar_model() {
    run_cases("ledger-ts-naive", 4000, |_, rng| {
        let year = 1900 + rng.below(301) as i64;
        let month = 1 + rng.below(12) as i64;
        let day = 1 + rng.below(days_in_month(year, month) as usize) as i64;
        let (hour, minute, second) = (
            rng.below(24) as i64,
            rng.below(60) as i64,
            rng.below(60) as i64,
        );
        let separator = *rng.pick(&["T", "t", " "]);
        let (fraction, millis) = match rng.below(6) {
            0 => (String::new(), 0),
            1 => (".5".to_string(), 500),
            2 => (".12".to_string(), 120),
            3 => (".123".to_string(), 123),
            4 => (".123456789".to_string(), 123),
            _ => (".007".to_string(), 7),
        };
        let (zone, offset) = match rng.below(5) {
            0 => (String::new(), 0),
            1 => ("Z".to_string(), 0),
            2 => ("z".to_string(), 0),
            _ => {
                let (h, m) = (rng.below(24) as i64, rng.below(60) as i64);
                let sign = if rng.chance(50) { 1 } else { -1 };
                (
                    format!("{}{h:02}:{m:02}", if sign > 0 { '+' } else { '-' }),
                    sign * (h * 3600 + m * 60),
                )
            }
        };
        let text = format!(
            "{year:04}-{month:02}-{day:02}{separator}{hour:02}:{minute:02}:{second:02}{fraction}{zone}"
        );
        let secs =
            naive_days(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
        assert_eq!(parse_ts(&text), Some(secs * 1000 + millis), "{text}");
    });
}

#[test]
fn lenient_acceptance_is_recorded_not_endorsed() {
    assert_eq!(parse_ts("2026-01-01T00:00:00.Z"), Some(1_767_225_600_000));
    assert_eq!(
        parse_ts("2026-02-31T00:00:00Z"),
        parse_ts("2026-03-03T00:00:00Z"),
        "day 31 of February rolls over"
    );
    assert_eq!(
        parse_ts("2026-01-01T24:00:00Z"),
        parse_ts("2026-01-02T00:00:00Z")
    );
    assert!(parse_ts("2026-01-01T99:99:99Z").is_some());
    assert_eq!(
        parse_ts("2026-01-01T00:00:00+05x30"),
        parse_ts("2026-01-01T00:00:00+05:30")
    );
    for bad in [
        "",
        "2026",
        "2026-13-01T00:00:00Z",
        "2026-00-01T00:00:00Z",
        "2026-01-00T00:00:00Z",
        "2026-01-32T00:00:00Z",
        "2026-01-01X00:00:00Z",
        "2026/01/01T00:00:00Z",
        "2026-01-01T00:00:00+0530",
        "2026-01-01T00:00:00 UTC",
        "2026-01-01T00:00:00Zjunk",
        "2026-01-01T00:00",
        "20260101T000000Z",
    ] {
        assert_eq!(parse_ts(bad), None, "{bad:?}");
    }
}

#[test]
fn parsing_garbage_and_multibyte_injection_never_panics() {
    run_cases("ledger-ts-garbage", 6000, |_, rng| {
        let base = format_ts(rng.below(4_000_000_000_000) as i64);
        let text = match rng.below(5) {
            0 => rng.garbage(40),
            1 => {
                let at = rng.below(base.len() + 1);
                let mut s = base.clone();
                while !s.is_char_boundary(at.min(s.len())) {
                    s.pop();
                }
                s.insert_str(
                    at.min(s.len()),
                    rng.pick(&["é", "日", "\u{1f600}", "\u{301}", "\u{feff}"]),
                );
                s
            }
            2 => {
                let chars: Vec<char> = base.chars().collect();
                let mut s: String = chars[..rng.below(chars.len() + 1)].iter().collect();
                s.push_str(&rng.garbage(6));
                s
            }
            3 => {
                let mut bytes = base.into_bytes();
                let at = rng.below(bytes.len());
                bytes[at] = b"0123456789-+:.TZ x"[rng.below(18)];
                String::from_utf8_lossy(&bytes).into_owned()
            }
            _ => format!(
                "{}{}",
                rng.string("0123456789", 4),
                rng.tokens(&["-", "T", ":", ".", "Z", "+", " ", "9", "0", "é"], 20)
            ),
        };
        let _ = parse_ts(&text);
    });
}

fn work_dir(rng: &mut Rng) -> String {
    rng.pick(&[
        "/work/a",
        "/work/b",
        "/work/a/",
        "/",
        "//",
        "/work/é",
        "",
        "relative/none",
    ])
    .to_string()
}

#[test]
fn launch_and_savings_records_round_trip_through_the_files() {
    let dir = ScratchDir::new("ledger-roundtrip");
    run_cases("ledger-roundtrip", 200, |_, rng| {
        dir.reset();
        let paths = Paths::new(dir.path());
        let mut launches = Vec::new();
        let mut savings = Vec::new();
        for _ in 0..rng.range(0, 6) {
            let now = rng.below(4_000_000_000_000) as i64;
            let entry = LedgerEntry {
                cwd: work_dir(rng),
                provider: *rng.pick(&ProviderName::ALL),
                preset: rng.garbage(8),
                routed: rng.chance(50),
                port: rng.chance(50).then(|| rng.below(65_536) as u16),
                pin: rng.garbage(6),
            };
            append_launch(&paths, &entry, now).unwrap();
            launches.push(LaunchRecord {
                ts: format_ts(now),
                cwd: norm_cwd(&entry.cwd),
                provider: entry.provider.as_str().to_string(),
                preset: entry.preset.clone(),
                routed: entry.routed,
                port: entry.port,
                pin: Some(entry.pin.clone()),
            });
            let record = SavingsEntry {
                ts: format_ts(now),
                provider: entry.provider.as_str().to_string(),
                source: rng.garbage(6),
                session: rng.chance(50).then(|| rng.garbage(6)),
                tokens_before: rng.next_u64(),
                tokens_after: rng.next_u64(),
            };
            append_savings(&paths, &record).unwrap();
            savings.push(record);
        }
        assert_eq!(read_launches(&paths), launches);
        assert_eq!(read_savings(&paths), savings);
    });
}

#[test]
fn appending_into_a_missing_directory_is_an_error_not_a_panic() {
    let dir = ScratchDir::new("ledger-missing-dir");
    let paths = Paths::new(dir.path().join("absent"));
    let entry = LedgerEntry {
        cwd: "/work/a".into(),
        provider: ProviderName::Headroom,
        preset: "interactive".into(),
        routed: true,
        port: Some(8787),
        pin: "1.2.3".into(),
    };
    assert!(append_launch(&paths, &entry, 0).is_err());
    assert!(read_launches(&paths).is_empty());
    assert!(read_savings(&paths).is_empty());
}

fn launch_json(rng: &mut Rng, ts: &str) -> (String, LaunchRecord) {
    let record = LaunchRecord {
        ts: ts.to_string(),
        cwd: work_dir(rng),
        provider: rng.pick(&["headroom", "rtk-only", "none", ""]).to_string(),
        preset: rng.garbage(5),
        routed: rng.chance(50),
        port: rng.chance(50).then(|| rng.below(65_536) as u16),
        pin: rng.chance(50).then(|| rng.garbage(4)),
    };
    let mut value = serde_json::to_value(&record).unwrap();
    if rng.chance(30) {
        value["extra"] = json!({"nested": [1, 2, 3]});
    }
    (value.to_string(), record)
}

#[test]
fn launch_files_with_torn_and_corrupt_lines_keep_every_valid_record() {
    let dir = ScratchDir::new("ledger-torn");
    let (mut kept, mut dropped) = (0, 0);
    run_cases("ledger-torn", 500, |_, rng| {
        dir.reset();
        let paths = Paths::new(dir.path());
        let mut bytes: Vec<u8> = Vec::new();
        let mut want: Vec<LaunchRecord> = Vec::new();
        for _ in 0..rng.range(0, 8) {
            match rng.below(9) {
                0 => bytes.extend_from_slice(rng.garbage(30).replace('\n', " ").as_bytes()),
                1 => bytes.extend_from_slice(b"{}"),
                2 => bytes.extend_from_slice(b"[1,2]"),
                3 => bytes.extend_from_slice(br#"{"ts": ""}"#),
                4 => bytes.extend_from_slice(br#"{"ts": "2026-01-01T00:00:00Z", "routed": "yes"}"#),
                5 => bytes.extend_from_slice(&[0xff, 0xfe, b'{', 0x80]),
                6 => {}
                _ => {
                    let ts = format_ts(rng.below(4_000_000_000_000) as i64);
                    let (line, record) = launch_json(rng, &ts);
                    bytes.extend_from_slice(line.as_bytes());
                    want.push(record);
                    kept += 1;
                    if rng.chance(30) {
                        bytes.push(b'\r');
                    }
                }
            }
            bytes.push(b'\n');
        }
        if rng.chance(50) {
            let (line, record) = launch_json(rng, "2026-01-01T00:00:00.000Z");
            let cut = rng.below(line.len() + 1);
            bytes.extend_from_slice(&line.as_bytes()[..cut]);
            if cut == line.len() {
                want.push(record);
            } else {
                dropped += 1;
            }
        }
        fs::write(paths.launches(), &bytes).unwrap();
        assert_eq!(read_launches(&paths), want);
    });
    assert!(kept > 300 && dropped > 50, "{kept} {dropped}");
}

#[test]
fn savings_files_with_torn_and_corrupt_lines_keep_every_valid_record() {
    let dir = ScratchDir::new("ledger-savings-torn");
    run_cases("ledger-savings-torn", 400, |_, rng| {
        dir.reset();
        let paths = Paths::new(dir.path());
        let mut text = String::new();
        let mut want = Vec::new();
        for _ in 0..rng.range(0, 8) {
            match rng.below(7) {
                0 => text.push_str(&rng.garbage(20).replace('\n', " ")),
                1 => text
                    .push_str(r#"{"ts":"x","provider":"p","tokens_before":-1,"tokens_after":2}"#),
                2 => text
                    .push_str(r#"{"ts":"x","provider":"p","tokens_before":1.5,"tokens_after":2}"#),
                3 => text.push_str(r#"{"ts":"x","provider":"p","tokens_before":1}"#),
                _ => {
                    let entry = SavingsEntry {
                        ts: rng.garbage(6),
                        provider: rng.garbage(4),
                        source: rng.garbage(4),
                        session: rng.chance(50).then(|| rng.garbage(4)),
                        tokens_before: rng.next_u64(),
                        tokens_after: rng.next_u64(),
                    };
                    text.push_str(&serde_json::to_string(&entry).unwrap());
                    want.push(entry);
                }
            }
            text.push('\n');
        }
        fs::write(paths.savings(), &text).unwrap();
        assert_eq!(read_savings(&paths), want);
    });
}

#[test]
fn norm_cwd_is_idempotent_and_tolerates_anything() {
    let dir = ScratchDir::new("ledger-cwd");
    let real = dir.path().join("real");
    fs::create_dir_all(&real).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, dir.path().join("link")).unwrap();
    let want = norm_cwd(real.to_str().unwrap());
    #[cfg(unix)]
    assert_eq!(norm_cwd(dir.path().join("link").to_str().unwrap()), want);
    assert_eq!(norm_cwd(&format!("{}/", real.display())), want);
    assert_eq!(norm_cwd("/"), "/");
    assert_eq!(norm_cwd("///"), "/");
    assert_eq!(norm_cwd("/no/such/dir/"), "/no/such/dir");
    run_cases("ledger-norm-cwd", 1500, |_, rng| {
        let text = rng.garbage(24);
        let once = norm_cwd(&text);
        assert_eq!(norm_cwd(&once), once, "{text:?}");
    });
}

fn launch_at(ts: i64, cwd: &str, routed: bool, tag: &str) -> LaunchRecord {
    LaunchRecord {
        ts: format_ts(ts),
        cwd: cwd.to_string(),
        provider: "headroom".into(),
        preset: tag.to_string(),
        routed,
        port: None,
        pin: None,
    }
}

#[test]
fn attribution_picks_the_latest_launch_inside_the_window() {
    let (mut hits, mut misses) = (0, 0);
    run_cases("ledger-attribute", 4000, |_, rng| {
        let base = 1_800_000_000_000i64;
        let cwds = ["/work/a", "/work/a/", "/work/b", "/"];
        let mut launches = Vec::new();
        for i in 0..rng.range(0, 6) {
            let ts = base + rng.below(200_000) as i64 - 50_000;
            let rec = launch_at(ts, rng.pick(&cwds), rng.chance(50), &format!("p{i}"));
            launches.push(if rng.chance(8) {
                LaunchRecord {
                    ts: rng.garbage(8),
                    ..rec
                }
            } else {
                rec
            });
        }
        let start = base + rng.below(200_000) as i64 - 50_000;
        let cwd = if rng.chance(10) {
            None
        } else {
            Some(rng.pick(&cwds).to_string())
        };
        let got = attribute(Some(start), cwd.as_deref(), &launches);

        let mut best: Option<(i64, &LaunchRecord)> = None;
        if let Some(cwd) = cwd.as_deref().filter(|c| !c.is_empty()) {
            for rec in &launches {
                if norm_cwd(&rec.cwd) != norm_cwd(cwd) {
                    continue;
                }
                let Some(ts) = parse_ts(&rec.ts) else {
                    continue;
                };
                let delta = start - ts;
                if (0..=90_000).contains(&delta) && best.is_none_or(|(b, _)| ts > b) {
                    best = Some((ts, rec));
                }
            }
        }
        assert_eq!(got, best.map(|(_, r)| r), "start {start} cwd {cwd:?}");
        if got.is_some() {
            hits += 1;
        } else {
            misses += 1;
        }
        assert_eq!(attribute(None, cwd.as_deref(), &launches), None);
        assert_eq!(attribute(Some(start), Some(""), &launches), None);
        assert_eq!(attribute(Some(start), None, &launches), None);
    });
    assert!(hits > 500 && misses > 500, "{hits} {misses}");
}

#[test]
fn attribution_window_edges_and_equal_timestamps() {
    let t = 1_800_000_000_000i64;
    let one = [launch_at(t, "/work/a", true, "x")];
    assert!(attribute(Some(t), Some("/work/a"), &one).is_some());
    assert!(attribute(Some(t + 90_000), Some("/work/a"), &one).is_some());
    assert!(attribute(Some(t + 90_001), Some("/work/a"), &one).is_none());
    assert!(attribute(Some(t - 1), Some("/work/a"), &one).is_none());

    let tie = [
        launch_at(t, "/work/a", true, "first"),
        launch_at(t, "/work/a", false, "second"),
    ];
    let got = attribute(Some(t + 1), Some("/work/a"), &tie).unwrap();
    assert_eq!(
        got.preset, "first",
        "mcpm keeps the first of equal timestamps"
    );
}

fn token_value(rng: &mut Rng) -> u64 {
    match rng.below(4) {
        0 => 0,
        1 => rng.below(1_000) as u64,
        _ => rng.next_u64() % (1 << 40),
    }
}

fn ts_in_2026(rng: &mut Rng) -> String {
    match rng.below(12) {
        0 => String::new(),
        1 => rng.garbage(6),
        _ => format_ts(1_767_225_600_000 + rng.below(40) as i64 * 86_400_000),
    }
}

#[derive(Default, Debug, PartialEq)]
struct Row {
    launches: u64,
    routed: u64,
    plain: u64,
    entries: u64,
    before: u128,
    after: u128,
}

#[test]
fn summaries_match_an_independent_model_and_keep_providers_sorted() {
    run_cases("ledger-summarize", 1500, |_, rng| {
        let names = ["headroom", "parsec", "none", "rtk-only", ""];
        let mut launches = Vec::new();
        for _ in 0..rng.range(0, 8) {
            launches.push(LaunchRecord {
                ts: ts_in_2026(rng),
                cwd: String::new(),
                provider: rng.pick(&names).to_string(),
                preset: String::new(),
                routed: rng.chance(50),
                port: None,
                pin: None,
            });
        }
        let mut savings = Vec::new();
        for _ in 0..rng.range(0, 8) {
            savings.push(SavingsEntry {
                ts: ts_in_2026(rng),
                provider: rng.pick(&names).to_string(),
                source: String::new(),
                session: None,
                tokens_before: token_value(rng),
                tokens_after: token_value(rng),
            });
        }
        let provider = rng.chance(40).then(|| rng.pick(&names).to_string());
        let since = rng
            .chance(40)
            .then(|| 1_767_225_600_000 + rng.below(30) as i64 * 86_400_000);
        let got = summarize(&launches, &savings, provider.as_deref(), since);

        let keep = |ts: &str, name: &str| {
            provider.as_deref().is_none_or(|p| p == name)
                && since.is_none_or(|s| parse_ts(ts).is_some_and(|t| t >= s))
        };
        let mut model: BTreeMap<String, Row> = BTreeMap::new();
        for rec in launches.iter().filter(|r| keep(&r.ts, &r.provider)) {
            let row = model.entry(rec.provider.clone()).or_default();
            row.launches += 1;
            if rec.routed {
                row.routed += 1;
            } else {
                row.plain += 1;
            }
        }
        for e in savings.iter().filter(|e| keep(&e.ts, &e.provider)) {
            let row = model.entry(e.provider.clone()).or_default();
            row.entries += 1;
            row.before += e.tokens_before as u128;
            row.after += e.tokens_after as u128;
        }
        assert_eq!(got.len(), model.len());
        for (summary, (name, row)) in got.iter().zip(&model) {
            assert_eq!(&summary.provider, name);
            assert_eq!(
                (
                    summary.launches,
                    summary.routed,
                    summary.plain,
                    summary.savings_entries
                ),
                (row.launches, row.routed, row.plain, row.entries)
            );
            assert_eq!(summary.launches, summary.routed + summary.plain);
            assert_eq!(summary.tokens_before as u128, row.before);
            assert_eq!(summary.tokens_after as u128, row.after);
            assert_eq!(
                summary.saved() as i128,
                row.before as i128 - row.after as i128
            );
            match summary.saved_percent() {
                Some(p) => {
                    assert!(summary.tokens_before > 0);
                    let want = (row.before as f64 - row.after as f64) * 100.0 / row.before as f64;
                    assert!((p - want).abs() < 1e-9, "{p} {want}");
                }
                None => assert_eq!(summary.tokens_before, 0),
            }
        }
    });
}

#[test]
fn summaries_and_savings_do_not_overflow_on_huge_token_counts() {
    let huge = |before: u64, after: u64| SavingsEntry {
        ts: "2026-01-01T00:00:00.000Z".into(),
        provider: "headroom".into(),
        source: String::new(),
        session: None,
        tokens_before: before,
        tokens_after: after,
    };
    for (before, after) in [
        (u64::MAX, 0),
        (0, u64::MAX),
        (u64::MAX, u64::MAX),
        (i64::MAX as u64, u64::MAX),
        (u64::MAX, i64::MAX as u64),
        (1 << 63, 1),
    ] {
        let entry = huge(before, after);
        let want = (before as i128 - after as i128).clamp(i64::MIN as i128, i64::MAX as i128);
        assert_eq!(entry.saved() as i128, want, "{before} {after}");
    }
    let entries = [
        huge(u64::MAX, u64::MAX),
        huge(u64::MAX, 7),
        huge(5, u64::MAX),
    ];
    let rows = summarize(&[], &entries, None, None);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].savings_entries, 3);
    assert_eq!(rows[0].tokens_before, u64::MAX);
    assert_eq!(rows[0].tokens_after, u64::MAX);
    let _ = rows[0].saved();
    let _ = rows[0].saved_percent();
}

#[test]
fn summaries_of_nothing_are_empty() {
    assert!(summarize(&[], &[], None, None).is_empty());
    assert!(summarize(&[], &[], Some("headroom"), Some(0)).is_empty());
    let value: Value = serde_json::to_value(ProviderSummary::default()).unwrap();
    assert_eq!(value["savingsEntries"], 0);
}
