//! Seeded randomized and edge-case tests for `compression verify`: transcript parsing, the cache
//! metrics and verdict, the engine fingerprint, version specs, context globs and shim scanning.

use super::ledger::{format_ts, LaunchRecord};
use super::model::{fnmatch, spec_matches, version_tuple, CompressionConfig, ProviderName};
use super::shims::{defined_functions, shim_snippet, ShimOptions, SHIM_FUNCTIONS};
use super::store::Paths;
use super::verify::*;
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

fn token_field(rng: &mut Rng) -> (Option<Value>, u64) {
    match rng.below(9) {
        0 => (None, 0),
        1 => (Some(json!(-5)), 0),
        2 => (Some(json!(1.5)), 0),
        3 => (Some(json!("7")), 0),
        4 => (Some(Value::Null), 0),
        5 => (Some(json!(5.0)), 0),
        6 => (Some(json!(0)), 0),
        _ => {
            let n = rng.next_u64() % (1 << 36);
            (Some(json!(n)), n)
        }
    }
}

#[derive(Default, Debug, PartialEq)]
struct Sums {
    turns: usize,
    read: u64,
    create: u64,
    input: u64,
    output: u64,
}

const KEYS: [&str; 4] = [
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
    "input_tokens",
    "output_tokens",
];

fn transcript(rng: &mut Rng, with_origin: bool) -> (Vec<u8>, Sums) {
    let mut bytes = Vec::new();
    let mut sums = Sums::default();
    for _ in 0..rng.range(0, 12) {
        match rng.below(10) {
            0 => bytes.extend_from_slice(rng.garbage(30).replace('\n', " ").as_bytes()),
            1 => bytes.extend_from_slice(b"[1, 2, 3]"),
            2 => bytes.extend_from_slice(b"\"just a string\""),
            3 => bytes.extend_from_slice(br#"{"message": "text", "type": "user"}"#),
            4 => bytes.extend_from_slice(br#"{"message": {"usage": "none"}}"#),
            5 => bytes.extend_from_slice(&[0xff, 0xfe, b'{']),
            6 => {}
            _ => {
                let mut usage = Map::new();
                let mut counted = [0u64; 4];
                for (i, key) in KEYS.iter().enumerate() {
                    let (value, n) = token_field(rng);
                    if let Some(v) = value {
                        usage.insert(key.to_string(), v);
                    }
                    counted[i] = n;
                }
                let mut record = json!({"message": {"usage": usage, "role": "assistant"}});
                if with_origin && rng.chance(50) {
                    record["cwd"] = json!("/work/a");
                    record["timestamp"] = json!("2026-01-01T00:00:00.000Z");
                }
                bytes.extend_from_slice(record.to_string().as_bytes());
                sums.turns += 1;
                sums.read += counted[0];
                sums.create += counted[1];
                sums.input += counted[2];
                sums.output += counted[3];
            }
        }
        bytes.push(b'\n');
    }
    if rng.chance(40) {
        let line = r#"{"message": {"usage": {"cache_read_input_tokens": 5}}}"#;
        bytes.extend_from_slice(&line.as_bytes()[..rng.below(line.len())]);
    }
    (bytes, sums)
}

#[test]
fn measure_matches_an_independent_sum_over_noisy_transcripts() {
    let dir = ScratchDir::new("verify-measure");
    let (mut counted, mut skipped) = (0, 0);
    run_cases("verify-measure", 400, |_, rng| {
        dir.reset();
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut all: Vec<Sums> = Vec::new();
        for i in 0..rng.range(0, 5) {
            let (bytes, sums) = transcript(rng, false);
            let path = dir.path().join(format!("s{i}.jsonl"));
            fs::write(&path, bytes).unwrap();
            paths.push(path);
            all.push(sums);
        }
        paths.push(dir.path().join("missing.jsonl"));
        let min_turns = rng.range(0, 6);
        let got = measure(&paths, min_turns);
        let mut want = Metrics::default();
        for s in &all {
            if s.turns < min_turns || s.read + s.create == 0 {
                skipped += 1;
                continue;
            }
            counted += 1;
            want.sessions += 1;
            want.turns += s.turns as u64;
            want.cache_read += s.read;
            want.cache_create += s.create;
            want.input_tokens += s.input;
            want.output_tokens += s.output;
        }
        assert_eq!(got, want, "min_turns {min_turns}");
    });
    assert!(counted > 200 && skipped > 100, "{counted} {skipped}");
}

#[test]
fn measure_and_metrics_survive_counts_beyond_u64() {
    let dir = ScratchDir::new("verify-measure-huge");
    let line = |read: u64, create: u64| {
        json!({"message": {"usage": {
            "cache_read_input_tokens": read,
            "cache_creation_input_tokens": create,
            "input_tokens": u64::MAX,
            "output_tokens": u64::MAX,
        }}})
        .to_string()
    };
    let path = dir.path().join("big.jsonl");
    let text = [
        line(u64::MAX, u64::MAX),
        line(u64::MAX, 1),
        line(3, u64::MAX),
    ]
    .join("\n");
    fs::write(&path, text).unwrap();
    let other = dir.path().join("big2.jsonl");
    fs::write(&other, [line(u64::MAX, 0), line(u64::MAX, 0)].join("\n")).unwrap();
    let m = measure(&[path, other], 1);
    assert_eq!(m.sessions, 2);
    assert_eq!(m.cache_read, u64::MAX);
    assert_eq!(m.cache_create, u64::MAX);
    assert_eq!(m.input_tokens, u64::MAX);
    let _ = (m.read_ratio(), m.read_write(), m.verdict());

    let extreme = Metrics {
        sessions: u64::MAX,
        turns: u64::MAX,
        cache_read: u64::MAX,
        cache_create: u64::MAX,
        input_tokens: u64::MAX,
        output_tokens: u64::MAX,
    };
    let ratio = extreme.read_ratio().unwrap();
    assert!((ratio - 0.5).abs() < 1e-6, "{ratio}");
    assert!(extreme.read_write().is_some());
    assert!(!extreme.verdict().0);
    let only_reads = Metrics {
        cache_read: u64::MAX,
        cache_create: 1,
        ..Metrics::default()
    };
    assert!(only_reads.verdict().0);
}

fn random_metrics(rng: &mut Rng) -> Metrics {
    let pick = |rng: &mut Rng| match rng.below(5) {
        0 => 0,
        1 => rng.below(20) as u64,
        _ => rng.next_u64() % 100_000,
    };
    Metrics {
        sessions: pick(rng),
        turns: pick(rng),
        cache_read: pick(rng) * 50,
        cache_create: pick(rng),
        input_tokens: pick(rng),
        output_tokens: pick(rng),
    }
}

#[test]
fn verdict_follows_the_thresholds() {
    let (mut pass, mut fail) = (0, 0);
    run_cases("verify-verdict", 6000, |_, rng| {
        let m = random_metrics(rng);
        let total = m.cache_read + m.cache_create;
        let ratio = (total > 0).then(|| m.cache_read as f64 / total as f64);
        assert_eq!(m.read_ratio(), ratio);
        let rw = (m.cache_create > 0).then(|| m.cache_read as f64 / m.cache_create as f64);
        assert_eq!(m.read_write(), rw);
        let (ok, text) = m.verdict();
        let want = match ratio {
            None => false,
            Some(r) if r < 0.90 => false,
            Some(_) => rw.is_none_or(|x| x >= 15.0),
        };
        assert_eq!(ok, want, "{m:?}: {text}");
        assert!(!text.is_empty());
        if ok {
            pass += 1;
        } else {
            fail += 1;
        }
        let other = random_metrics(rng);
        let delta = compare(&m, &other);
        match (m.read_ratio(), other.read_ratio()) {
            (Some(a), Some(b)) => assert_eq!(delta, Some((b - a) * 100.0)),
            _ => assert_eq!(delta, None),
        }
    });
    assert!(pass > 300 && fail > 300, "{pass} {fail}");
    assert_eq!(
        Metrics::default().verdict().1,
        "no cache data in the sampled sessions"
    );
}

fn origin_record(rng: &mut Rng) -> (String, Option<(i64, String)>) {
    let ts_ms = 1_800_000_000_000i64 + rng.below(1_000_000) as i64;
    let cwd = rng.pick(&["/work/a", "/work/b", ""]).to_string();
    let mut record = Map::new();
    let mut ts_ok = false;
    match rng.below(5) {
        0 => {}
        1 => {
            record.insert("timestamp".into(), json!("not a time"));
        }
        2 => {
            record.insert("timestamp".into(), json!(12345));
        }
        _ => {
            record.insert("timestamp".into(), json!(format_ts(ts_ms)));
            ts_ok = true;
        }
    }
    let mut cwd_ok = false;
    match rng.below(4) {
        0 => {}
        1 => {
            record.insert("cwd".into(), json!(7));
        }
        _ => {
            record.insert("cwd".into(), json!(cwd));
            cwd_ok = !cwd.is_empty();
        }
    }
    let origin = (ts_ok && cwd_ok).then_some((ts_ms, cwd));
    (Value::Object(record).to_string(), origin)
}

#[test]
fn session_origin_is_the_first_record_with_a_time_and_a_directory() {
    let dir = ScratchDir::new("verify-origin");
    run_cases("verify-origin", 800, |_, rng| {
        dir.reset();
        let path = dir.path().join("s.jsonl");
        let mut text = String::new();
        let mut want = None;
        for _ in 0..rng.range(0, 8) {
            if rng.chance(15) {
                text.push_str(&rng.garbage(20).replace('\n', " "));
            } else {
                let (line, origin) = origin_record(rng);
                text.push_str(&line);
                if want.is_none() {
                    want = origin;
                }
            }
            text.push('\n');
        }
        fs::write(&path, &text).unwrap();
        let (start, cwd) = session_origin(&path);
        assert_eq!(start, want.as_ref().map(|(t, _)| *t));
        assert_eq!(cwd, want.map(|(_, c)| c));
    });
    assert_eq!(session_origin(&dir.path().join("missing")), (None, None));
}

fn launch(ts: i64, cwd: &str, routed: bool, pin: Option<&str>) -> LaunchRecord {
    LaunchRecord {
        ts: format_ts(ts),
        cwd: cwd.into(),
        provider: "headroom".into(),
        preset: "interactive".into(),
        routed,
        port: None,
        pin: pin.map(String::from),
    }
}

#[test]
fn partitions_cover_every_transcript_exactly_once() {
    let dir = ScratchDir::new("verify-partition");
    run_cases("verify-partition", 400, |_, rng| {
        dir.reset();
        let base = 1_800_000_000_000i64;
        let mut launches = Vec::new();
        for _ in 0..rng.range(0, 5) {
            let pin = rng.pick(&[None, Some(""), Some("1.2.3"), Some("1.3.0")]);
            launches.push(launch(
                base + rng.below(100_000) as i64,
                rng.pick(&["/work/a", "/work/b"]),
                rng.chance(50),
                *pin,
            ));
        }
        let mut paths = Vec::new();
        for i in 0..rng.range(0, 7) {
            let path = dir.path().join(format!("s{i:02}.jsonl"));
            let record = json!({
                "timestamp": format_ts(base + rng.below(200_000) as i64),
                "cwd": rng.pick(&["/work/a", "/work/b", "/elsewhere"]),
            });
            fs::write(&path, record.to_string()).unwrap();
            paths.push(path);
        }
        let buckets = partition(&paths, &launches);
        let mut seen: Vec<&PathBuf> = buckets
            .proxied
            .iter()
            .chain(&buckets.plain)
            .chain(&buckets.unattributed)
            .collect();
        seen.sort();
        let mut all: Vec<&PathBuf> = paths.iter().collect();
        all.sort();
        assert_eq!(seen, all);
        for bucket in [&buckets.proxied, &buckets.plain, &buckets.unattributed] {
            let order: Vec<usize> = bucket
                .iter()
                .map(|p| paths.iter().position(|q| q == p).unwrap())
                .collect();
            assert!(order.windows(2).all(|w| w[0] < w[1]), "input order kept");
        }
        let by_pin = partition_by_pin(&paths, &launches);
        let mut pinned: Vec<&PathBuf> = by_pin.values().flatten().collect();
        pinned.sort();
        let mut proxied: Vec<&PathBuf> = buckets.proxied.iter().collect();
        proxied.sort();
        assert_eq!(pinned, proxied);
        for pin in by_pin.keys() {
            assert!(
                ["unknown", "1.2.3", "1.3.0"].contains(&pin.as_str()),
                "{pin}"
            );
        }
    });
}

#[test]
fn schema_check_looks_only_at_the_first_transcript_with_usage() {
    let dir = ScratchDir::new("verify-schema");
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        fs::write(&path, text).unwrap();
        path
    };
    let with = write(
        "a.jsonl",
        r#"{"message": {"usage": {"cache_read_input_tokens": null}}}"#,
    );
    let without = write("b.jsonl", r#"{"message": {"usage": {"input_tokens": 5}}}"#);
    let empty = write("c.jsonl", "");
    let junk = write("d.jsonl", "not json");
    assert!(
        schema_ok(&[with.clone()]),
        "a present key counts, whatever its value"
    );
    assert!(!schema_ok(&[without.clone()]));
    assert!(!schema_ok(&[]));
    assert!(!schema_ok(&[empty.clone(), junk.clone()]));
    assert!(schema_ok(&[empty.clone(), junk.clone(), with.clone()]));
    assert!(
        !schema_ok(&[without, with]),
        "later transcripts are not consulted"
    );
    assert!(!schema_ok(&[dir.path().join("missing.jsonl")]));
}

#[test]
fn transcript_discovery_lists_project_files_newest_first() {
    let dir = ScratchDir::new("verify-discovery");
    run_cases("verify-discovery", 150, |_, rng| {
        dir.reset();
        let root = dir.path().join("projects");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("top-level.jsonl"), "{}").unwrap();
        let mut want: Vec<(u64, PathBuf)> = Vec::new();
        for p in 0..rng.range(0, 4) {
            let project = root.join(format!("proj{p}"));
            fs::create_dir_all(&project).unwrap();
            fs::create_dir_all(project.join("nested")).unwrap();
            fs::write(project.join("nested/deep.jsonl"), "{}").unwrap();
            fs::create_dir_all(project.join("dir.jsonl")).unwrap();
            fs::write(project.join("notes.txt"), "x").unwrap();
            fs::write(project.join("noext"), "x").unwrap();
            for f in 0..rng.range(0, 4) {
                let path = project.join(format!("f{f}.jsonl"));
                fs::write(&path, "{}").unwrap();
                let secs = rng.below(6) as u64 * 100;
                let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
                file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
                    .unwrap();
                want.push((secs, path));
            }
        }
        want.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let got = iter_transcripts(&root);
        assert_eq!(got, want.into_iter().map(|(_, p)| p).collect::<Vec<_>>());
    });
    assert!(iter_transcripts(&dir.path().join("missing")).is_empty());
}

fn health_value(rng: &mut Rng) -> Value {
    let mut health = Map::new();
    for key in HEALTH_REQUIRED {
        if rng.chance(75) {
            let value = match key {
                "ready" => json!(rng.chance(80)),
                "version" => json!(*rng.pick(&["1.2.3", "0.9.0", "", "x"])),
                _ => json!("ok"),
            };
            health.insert(key.to_string(), value);
        }
    }
    match rng.below(6) {
        0 => {}
        1 => {
            health.insert("config".into(), json!([1, 2]));
        }
        2 => {
            health.insert("config".into(), json!("text"));
        }
        _ => {
            let mut config = Map::new();
            for key in HEALTH_CONFIG_KEYS.iter().chain(&["extra_a", "extra_b"]) {
                if rng.chance(70) {
                    let value = match rng.below(7) {
                        0 => Value::Null,
                        1 => json!(true),
                        2 => json!(rng.below(100)),
                        3 => json!(rng.garbage(6)),
                        4 => json!([1, "a"]),
                        5 => json!({"x": null}),
                        _ => json!(1.5),
                    };
                    config.insert(key.to_string(), value);
                }
            }
            health.insert("config".into(), Value::Object(config));
        }
    }
    Value::Object(health)
}

#[test]
fn fingerprints_follow_the_documented_digest_and_missing_lists() {
    run_cases("verify-fingerprint", 1500, |_, rng| {
        let health = match rng.below(8) {
            0 => json!([1]),
            1 => json!("text"),
            2 => Value::Null,
            _ => health_value(rng),
        };
        let fp = fingerprint(&health);
        let mut keys: Vec<String> = health
            .get("config")
            .and_then(Value::as_object)
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default();
        keys.sort();
        let digest = Sha256::digest(keys.join("\n").as_bytes());
        let want: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
        assert_eq!(fp.digest, want);
        assert_eq!(fp.digest.len(), 16);
        assert_eq!(
            fp.version.as_deref(),
            health.get("version").and_then(Value::as_str)
        );
        let missing: Vec<&str> = HEALTH_REQUIRED
            .iter()
            .copied()
            .filter(|k| health.get(*k).is_none())
            .collect();
        assert_eq!(fp.missing_required, missing);
        let gone: Vec<&str> = HEALTH_CONFIG_KEYS
            .iter()
            .copied()
            .filter(|k| !keys.iter().any(|have| have == k))
            .collect();
        assert_eq!(fp.missing_config, gone);
        assert_eq!(fingerprint(&health), fp, "deterministic");
    });
}

struct Fake {
    installed: Option<String>,
    health: Option<Value>,
    binary: bool,
}

impl HealthProbe for Fake {
    fn binary(&self, name: &str) -> Option<String> {
        self.binary.then(|| format!("/usr/bin/{name}"))
    }

    fn installed_version(&self) -> Option<String> {
        self.installed.clone()
    }

    fn proxy_health(&self, _port: u16) -> Option<Value> {
        self.health.clone()
    }
}

#[test]
fn health_checks_are_total_over_arbitrary_engine_answers() {
    let dir = ScratchDir::new("verify-health");
    run_cases("verify-health", 800, |_, rng| {
        let mut health = health_value(rng);
        if rng.chance(70) {
            health["ready"] = json!(true);
        }
        let probe = Fake {
            installed: rng
                .chance(70)
                .then(|| rng.pick(&["1.2.3", "0.9.0", ""]).to_string()),
            health: rng.chance(85).then_some(health),
            binary: rng.chance(50),
        };
        let provider = *rng.pick(&ProviderName::ALL);
        let config = CompressionConfig::with_provider(provider);
        let paths = Paths::new(dir.path());
        let checks = health_checks(&config, &paths, &probe);
        assert!(!checks.is_empty());
        assert!(checks
            .iter()
            .all(|c| !c.name.is_empty() && !c.detail.is_empty()));
        assert_eq!(
            checks,
            health_checks(&config, &paths, &probe),
            "deterministic"
        );
    });
}

#[test]
fn version_tuples_follow_a_regex_model() {
    let digits = Regex::new(r"^\+?[0-9]+$").unwrap();
    run_cases("verify-version-tuple", 5000, |_, rng| {
        let text = match rng.below(4) {
            0 => rng.garbage(12),
            1 => format!("{}.{}.{}", rng.below(50), rng.below(50), rng.below(50)),
            2 => rng.tokens(
                &[
                    "1",
                    ".",
                    "2",
                    " ",
                    "x",
                    "-",
                    "+",
                    "99999999999999999999",
                    "0",
                ],
                8,
            ),
            _ => format!(
                "{}{}.{}.{}{}",
                rng.string(" ", 2),
                rng.below(9),
                rng.below(9),
                rng.below(9),
                rng.pick(&["", ".4", "-rc1", "+b", " "])
            ),
        };
        let parts: Vec<&str> = text.trim().split('.').take(3).collect();
        let parsed: Option<Vec<u64>> = parts
            .iter()
            .map(|p| {
                let p = p.trim();
                digits.is_match(p).then(|| p.parse::<u64>().ok()).flatten()
            })
            .collect();
        assert_eq!(
            version_tuple(Some(&text)),
            parsed.unwrap_or_default(),
            "{text:?}"
        );
    });
    assert!(version_tuple(None).is_empty());
    assert_eq!(version_tuple(Some("1.2")), vec![1, 2]);
    assert_eq!(version_tuple(Some(" 1 . 2 . 3 ")), vec![1, 2, 3]);
    assert_eq!(version_tuple(Some("1.2.3.4")), vec![1, 2, 3]);
    assert!(version_tuple(Some("1.2.3-rc1")).is_empty());
    assert!(version_tuple(Some("")).is_empty());
}

fn version_text(rng: &mut Rng) -> String {
    format!("{}.{}.{}", rng.below(4), rng.below(4), rng.below(4))
}

#[test]
fn version_specs_obey_complement_and_conjunction_laws() {
    run_cases("verify-spec", 6000, |_, rng| {
        let v = version_text(rng);
        let x = version_text(rng);
        let y = version_text(rng);
        let m = |spec: &str| spec_matches(Some(&v), spec);
        assert_eq!(m(&format!(">={x}")), !m(&format!("<{x}")), "{v} {x}");
        assert_eq!(m(&format!(">{x}")), !m(&format!("<={x}")), "{v} {x}");
        assert_eq!(m(&format!("=={x}")), !m(&format!("!={x}")), "{v} {x}");
        assert_eq!(m(&x), m(&format!("=={x}")));
        assert_eq!(m(&format!("=={x}")), v == x);
        let (a, b) = (format!(">={x}"), format!("<{y}"));
        assert_eq!(m(&format!("{a},{b}")), m(&a) && m(&b));
        assert_eq!(m(&format!(" {a} , {b} ,")), m(&a) && m(&b));
        assert!(m("*") && m("") && m("  "));
        let tuple = |s: &str| version_tuple(Some(s));
        assert_eq!(m(&format!(">={x}")), tuple(&v) >= tuple(&x));
        assert_eq!(m(&format!("<={x}")), tuple(&v) <= tuple(&x));

        let unknown = rng.pick(&["", "x", "1.2.3-rc", "..", "1.x.3"]);
        let spec = rng.tokens(&["*", ">=1.0.0", "==1.2.3", ",", " ", "<2.0.0"], 4);
        let want = matches!(spec.trim(), "" | "*");
        assert_eq!(
            spec_matches(Some(unknown), &spec),
            want,
            "{unknown:?} {spec:?}"
        );
        assert_eq!(spec_matches(None, &spec), want);
    });
}

#[test]
fn malformed_clauses_never_match() {
    for spec in [
        ">=", ">=x", "==", "!=1.x", "<", "~1.2.3", "=>1.0.0", "1.x.3", ">= ",
    ] {
        assert!(!spec_matches(Some("1.2.3"), spec), "{spec:?}");
    }
    assert!(spec_matches(Some("1.2.3"), ">= 1.0.0"));
    assert!(spec_matches(Some("1.2.3"), "1.2.3"));
    assert!(spec_matches(Some("1.2"), ">=1.2"));
    assert!(spec_matches(Some("1.2"), "<1.2.0"));
}

fn esc(c: char) -> String {
    format!("\\x{{{:x}}}", c as u32)
}

fn translate(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::from("(?s)^");
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => {
                let mut j = i;
                if j < chars.len() && chars[j] == '!' {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ']' {
                    j += 1;
                }
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j >= chars.len() {
                    out.push_str("\\[");
                    continue;
                }
                let mut stuff = &chars[i..j];
                let negate = stuff.first() == Some(&'!');
                if negate {
                    stuff = &stuff[1..];
                }
                let mut items = String::new();
                let mut k = 0;
                while k < stuff.len() {
                    if k + 2 < stuff.len() && stuff[k + 1] == '-' {
                        if stuff[k] <= stuff[k + 2] {
                            items.push_str(&format!("{}-{}", esc(stuff[k]), esc(stuff[k + 2])));
                        }
                        k += 3;
                    } else {
                        items.push_str(&esc(stuff[k]));
                        k += 1;
                    }
                }
                match (items.is_empty(), negate) {
                    (true, true) => out.push('.'),
                    (true, false) => out.push_str("[^\\x{0}-\\x{10FFFF}]"),
                    (false, true) => out.push_str(&format!("[^{items}]")),
                    (false, false) => out.push_str(&format!("[{items}]")),
                }
                i = j + 1;
            }
            c => out.push_str(&esc(c)),
        }
    }
    out.push('$');
    out
}

#[test]
fn context_globs_follow_pythons_fnmatch_translation() {
    let (mut yes, mut no) = (0, 0);
    run_cases("verify-fnmatch", 2500, |_, rng| {
        let pattern = rng.tokens(
            &[
                "*", "?", "a", "b", "c", "/", "[ab]", "[!a]", "[a-c]", "[]a]", "[a-]", "[-a]",
                "[!]a]", "[", "]", "x", "é", "!", "*a*", "a*b", "-",
            ],
            6,
        );
        let model = Regex::new(&translate(&pattern)).unwrap();
        for _ in 0..12 {
            let text = rng.tokens(&["a", "b", "c", "/", "x", "[", "]", "-", "é", "!", "\n"], 8);
            let want = model.is_match(&text);
            assert_eq!(fnmatch(&pattern, &text), want, "{pattern:?} vs {text:?}");
            if want {
                yes += 1;
            } else {
                no += 1;
            }
        }
    });
    assert!(yes > 500 && no > 2000, "{yes} {no}");
}

#[test]
fn glob_matching_stays_fast_on_pathological_patterns() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let pattern = format!("{}b", "*a".repeat(12));
        let text = "a".repeat(80);
        let started = std::time::Instant::now();
        let first = fnmatch(&pattern, &text);
        let second = fnmatch(&format!("{}a", "*a".repeat(12)), &text);
        let _ = tx.send((first, second, started.elapsed()));
    });
    let (first, second, took) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("glob matching blew up on repeated stars");
    assert!(!first);
    assert!(second);
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn shim_scanning_follows_a_line_model_and_sees_every_shim() {
    let model = Regex::new(r"^([A-Za-z0-9_-]+)\s*\(\)").unwrap();
    run_cases("verify-shims", 3000, |_, rng| {
        let mut lines = Vec::new();
        for _ in 0..rng.range(0, 8) {
            lines.push(match rng.below(8) {
                0 => rng.garbage(20).replace('\n', " "),
                1 => format!("{}() {{ true; }}", rng.slug(8)),
                2 => format!("{} () {{ true; }}", rng.slug(8)),
                3 => format!("  {}() {{ true; }}", rng.slug(8)),
                4 => format!("{}(x)", rng.slug(8)),
                5 => format!("function {}() {{ true; }}", rng.slug(8)),
                6 => format!("{}\t()", rng.slug(8)),
                _ => format!("é{}()", rng.slug(4)),
            });
        }
        let text = lines.join("\n");
        let want: Vec<String> = text
            .lines()
            .filter_map(|l| model.captures(l).map(|c| c[1].to_string()))
            .collect();
        assert_eq!(defined_functions(&text), want, "{text:?}");
    });
    for route_claude in [false, true] {
        let defined = defined_functions(&shim_snippet(ShimOptions { route_claude }));
        for name in SHIM_FUNCTIONS {
            assert!(defined.iter().any(|d| d == name), "{name}");
        }
        assert_eq!(defined.iter().any(|d| d == "claude"), route_claude);
    }
}
