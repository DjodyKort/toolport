//! `compression verify`: provider-agnostic health checks (engine, pin, shims, fingerprint)
//! plus the prompt-cache measurement over Claude Code's own transcripts. The billed usage in
//! a transcript is the only evidence trusted; a provider's self-reported savings never is.

use super::capability::{split_unsealed, unsealed_for};
use super::ledger::{attribute, parse_ts, LaunchRecord};
use super::model::{version_tuple, CompressionConfig, ProviderName};
use super::shims::{defined_functions, SHIM_FUNCTIONS};
use super::store::Paths;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MIN_TURNS: usize = 5;
pub const FAIL_READ_RATIO: f64 = 0.90;
pub const FAIL_READ_WRITE: f64 = 15.0;

/// Top-level `/health` keys the launcher depends on; anything else is optional.
pub const HEALTH_REQUIRED: [&str; 4] = ["service", "status", "ready", "version"];

/// `/health.config` keys recorded from a verified build; a pinned build must still expose them.
pub const HEALTH_CONFIG_KEYS: [&str; 21] = [
    "accuracy_guard",
    "anthropic_api_url",
    "backend",
    "cache",
    "compress_system_messages",
    "compress_user_messages",
    "disable_kompress",
    "force_kompress",
    "learn",
    "max_items_after_crush",
    "memory",
    "min_tokens_to_crush",
    "optimize",
    "pid",
    "protect_analysis_context",
    "protect_recent",
    "rate_limit",
    "savings_profile",
    "smart_crusher_with_compaction",
    "target_ratio",
    "target_savings_percent",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub sessions: u64,
    pub turns: u64,
    pub cache_read: u64,
    pub cache_create: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Metrics {
    pub fn read_ratio(&self) -> Option<f64> {
        let total = u128::from(self.cache_read) + u128::from(self.cache_create);
        (total > 0).then(|| self.cache_read as f64 / total as f64)
    }

    pub fn read_write(&self) -> Option<f64> {
        (self.cache_create > 0).then(|| self.cache_read as f64 / self.cache_create as f64)
    }

    /// Unknown data never passes silently.
    pub fn verdict(&self) -> (bool, String) {
        let Some(ratio) = self.read_ratio() else {
            return (false, "no cache data in the sampled sessions".into());
        };
        if ratio < FAIL_READ_RATIO {
            return (
                false,
                format!(
                    "read ratio {:.1}% < {:.0}% - prefix is being busted",
                    ratio * 100.0,
                    FAIL_READ_RATIO * 100.0
                ),
            );
        }
        match self.read_write() {
            Some(rw) if rw < FAIL_READ_WRITE => (
                false,
                format!("read:write {rw:.1}x < {FAIL_READ_WRITE:.0}x - too much cache re-creation"),
            ),
            Some(rw) => (
                true,
                format!("read ratio {:.1}%, read:write {rw:.1}x", ratio * 100.0),
            ),
            None => (true, "ok".into()),
        }
    }
}

pub fn transcript_root() -> PathBuf {
    let base = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"));
    base.join("projects")
}

/// `<root>/*/*.jsonl`, newest first.
pub fn iter_transcripts(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let Ok(projects) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    for project in projects.flatten() {
        let Ok(files) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "jsonl") && path.is_file() {
                let mtime = file
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                found.push((mtime, path));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, p)| p).collect()
}

/// Transcripts are appended to live, so the last record may be half written.
fn records(path: &Path) -> Vec<Value> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(Value::is_object)
        .collect()
}

fn usage_blocks(path: &Path) -> Vec<Value> {
    records(path)
        .into_iter()
        .filter_map(|r| r.get("message")?.get("usage").cloned())
        .filter(Value::is_object)
        .collect()
}

fn tokens(usage: &Value, key: &str) -> u64 {
    usage.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// (start ms, cwd) from the first record carrying both: the launch identity matched
/// against the ledger.
pub fn session_origin(path: &Path) -> (Option<i64>, Option<String>) {
    for rec in records(path) {
        let ts = rec
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        let cwd = rec
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty());
        if let (Some(ts), Some(cwd)) = (ts, cwd) {
            return (Some(ts), Some(cwd.to_string()));
        }
    }
    (None, None)
}

pub fn measure(paths: &[PathBuf], min_turns: usize) -> Metrics {
    let mut m = Metrics::default();
    for path in paths {
        let (mut rd, mut cw, mut inp, mut out, mut turns) = (0u64, 0u64, 0u64, 0u64, 0usize);
        for u in usage_blocks(path) {
            rd = rd.saturating_add(tokens(&u, "cache_read_input_tokens"));
            cw = cw.saturating_add(tokens(&u, "cache_creation_input_tokens"));
            inp = inp.saturating_add(tokens(&u, "input_tokens"));
            out = out.saturating_add(tokens(&u, "output_tokens"));
            turns += 1;
        }
        if turns < min_turns || (rd == 0 && cw == 0) {
            continue;
        }
        m.sessions += 1;
        m.turns += turns as u64;
        m.cache_read = m.cache_read.saturating_add(rd);
        m.cache_create = m.cache_create.saturating_add(cw);
        m.input_tokens = m.input_tokens.saturating_add(inp);
        m.output_tokens = m.output_tokens.saturating_add(out);
    }
    m
}

#[derive(Clone, Debug, Default)]
pub struct Buckets {
    pub proxied: Vec<PathBuf>,
    pub plain: Vec<PathBuf>,
    pub unattributed: Vec<PathBuf>,
}

/// Proxied launches routed through a provider; plain ones deliberately did not (a true
/// control); no matching launch is its own bucket, never folded into plain.
pub fn partition(paths: &[PathBuf], launches: &[LaunchRecord]) -> Buckets {
    let mut out = Buckets::default();
    for path in paths {
        let (start, cwd) = session_origin(path);
        match attribute(start, cwd.as_deref(), launches) {
            None => out.unattributed.push(path.clone()),
            Some(rec) if rec.routed => out.proxied.push(path.clone()),
            Some(_) => out.plain.push(path.clone()),
        }
    }
    out
}

/// Routed sessions grouped by the pin their launch ran against (`unknown` before pins were
/// recorded), so an A/B across builds does not blend into one number.
pub fn partition_by_pin(
    paths: &[PathBuf],
    launches: &[LaunchRecord],
) -> BTreeMap<String, Vec<PathBuf>> {
    let mut out: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for path in paths {
        let (start, cwd) = session_origin(path);
        if let Some(rec) = attribute(start, cwd.as_deref(), launches).filter(|r| r.routed) {
            let pin = rec.pin.clone().filter(|p| !p.is_empty());
            out.entry(pin.unwrap_or_else(|| "unknown".into()))
                .or_default()
                .push(path.clone());
        }
    }
    out
}

/// Percentage-point change in read ratio, `b` relative to `a`.
pub fn compare(a: &Metrics, b: &Metrics) -> Option<f64> {
    Some((b.read_ratio()? - a.read_ratio()?) * 100.0)
}

/// If Claude Code stops writing the cache fields, zeros would read as a total collapse.
pub fn schema_ok(paths: &[PathBuf]) -> bool {
    paths
        .iter()
        .find_map(|p| usage_blocks(p).into_iter().next())
        .is_some_and(|u| {
            u.get("cache_read_input_tokens").is_some()
                || u.get("cache_creation_input_tokens").is_some()
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

fn check(name: &str, ok: bool, detail: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        ok,
        detail: detail.into(),
    }
}

/// What the checks need from the machine; a fake in tests, `SystemOps` in the binary.
pub trait HealthProbe {
    fn binary(&self, name: &str) -> Option<String>;
    fn installed_version(&self) -> Option<String>;
    fn proxy_health(&self, port: u16) -> Option<Value>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fingerprint {
    pub version: Option<String>,
    pub digest: String,
    pub missing_required: Vec<String>,
    pub missing_config: Vec<String>,
}

/// The engine's shape from `/health`: its version and a digest of the config keys it exposes.
pub fn fingerprint(health: &Value) -> Fingerprint {
    let mut keys: Vec<String> = health
        .get("config")
        .and_then(Value::as_object)
        .map(|c| c.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    let mut hasher = Sha256::new();
    hasher.update(keys.join("\n").as_bytes());
    let digest: String = hasher
        .finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    Fingerprint {
        version: health
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string),
        digest,
        missing_required: HEALTH_REQUIRED
            .iter()
            .filter(|k| health.get(**k).is_none())
            .map(|k| k.to_string())
            .collect(),
        missing_config: HEALTH_CONFIG_KEYS
            .iter()
            .filter(|k| !keys.iter().any(|have| have == **k))
            .map(|k| k.to_string())
            .collect(),
    }
}

fn provider_checks(
    config: &CompressionConfig,
    probe: &dyn HealthProbe,
    out: &mut Vec<Check>,
) -> Option<Value> {
    match config.provider {
        ProviderName::None => {
            out.push(check(
                "engine",
                true,
                "compression disabled (provider none)",
            ));
            None
        }
        ProviderName::RtkOnly => {
            let found = probe.binary("rtk");
            out.push(check(
                "engine",
                found.is_some(),
                match &found {
                    Some(p) => format!("rtk binary found at {p}"),
                    None => "rtk binary not on PATH".into(),
                },
            ));
            None
        }
        ProviderName::Parsec => {
            let found = probe.binary("parsec");
            out.push(check(
                "engine",
                found.is_some(),
                match &found {
                    Some(p) => format!("parsec binary found at {p}"),
                    None => "parsec binary not on PATH".into(),
                },
            ));
            None
        }
        ProviderName::Headroom => {
            let pin = &config.provider_version.pin;
            let installed = probe.installed_version();
            out.push(check(
                "engine binary",
                installed.is_some(),
                match &installed {
                    Some(v) if v == pin => format!("{v} == pin {pin}"),
                    Some(v) => format!("{v} != pin {pin}"),
                    None => format!("headroom not on PATH (pin {pin})"),
                },
            ));
            if let Some(v) = &installed {
                out.push(check(
                    "engine version",
                    v == pin,
                    if v == pin {
                        format!("{v} == pin")
                    } else {
                        format!("{v} != pin {pin}: unverified build, run `compression update`")
                    },
                ));
            }
            let port = config.preset_for(None).port;
            let health = probe.proxy_health(port);
            let ready = health
                .as_ref()
                .and_then(|h| h.get("ready"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            out.push(check(
                "engine reachable",
                ready,
                if ready {
                    format!("proxy ready on :{port}")
                } else {
                    format!("no ready proxy on :{port}")
                },
            ));
            health.filter(|_| ready)
        }
    }
}

fn fingerprint_checks(config: &CompressionConfig, health: &Value, out: &mut Vec<Check>) {
    let fp = fingerprint(health);
    let pin = &config.provider_version.pin;
    let version_ok = fp.version.as_deref() == Some(pin.as_str());
    out.push(check(
        "engine fingerprint",
        version_ok && fp.missing_required.is_empty() && fp.missing_config.is_empty(),
        match (
            &fp.version,
            fp.missing_required.is_empty(),
            fp.missing_config.is_empty(),
        ) {
            (_, false, _) => format!("health lacks: {}", fp.missing_required.join(", ")),
            (_, _, false) => format!(
                "version {} digest {}; config keys gone: {}",
                fp.version.as_deref().unwrap_or("?"),
                fp.digest,
                fp.missing_config.join(", ")
            ),
            (Some(v), _, _) if v != pin => {
                format!("running {v} != pin {pin} (digest {})", fp.digest)
            }
            (v, _, _) => format!(
                "running {} == pin, digest {}, contract keys present",
                v.as_deref().unwrap_or("?"),
                fp.digest
            ),
        },
    ));
    if let Some(effective) = health.get("config").and_then(Value::as_object) {
        let preset = config.preset_for(None);
        let (declarable, unset) = split_unsealed(&unsealed_for(&preset.knobs, effective));
        let mut note = if declarable.is_empty() {
            "every declarable knob is policy".to_string()
        } else {
            format!(
                "vendor-decided, declarable: {}",
                declarable
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        if !unset.is_empty() {
            note.push_str(&format!(
                "; {} at the engine's internal default (pinned)",
                unset.len()
            ));
        }
        out.push(check("sealed posture", declarable.is_empty(), note));
    }
}

fn pin_checks(config: &CompressionConfig, out: &mut Vec<Check>) {
    let pv = &config.provider_version;
    if config.provider != ProviderName::Headroom {
        out.push(check(
            "pin",
            true,
            format!("not used by provider {}", config.provider.as_str()),
        ));
        return;
    }
    let parseable = !version_tuple(Some(&pv.pin)).is_empty();
    out.push(check(
        "pin",
        parseable,
        if parseable {
            format!("exact pin {}", pv.requirement())
        } else {
            format!("'{}' is not a parseable X.Y.Z version", pv.pin)
        },
    ));
    let stale: Vec<&str> = config
        .presets
        .iter()
        .filter(|(_, p)| {
            p.savings_profile.is_some() && p.snapshot_version.as_deref() != Some(&pv.pin)
        })
        .map(|(n, _)| n.as_str())
        .collect();
    out.push(check(
        "preset provenance",
        stale.is_empty(),
        if stale.is_empty() {
            "all snapshots match the pin".to_string()
        } else {
            format!("snapshotted from another build: {}", stale.join(", "))
        },
    ));
}

fn shim_checks(config: &CompressionConfig, paths: &Paths, out: &mut Vec<Check>) {
    let required = config.provider == ProviderName::Headroom;
    let path = paths.shims();
    let Ok(text) = std::fs::read_to_string(&path) else {
        out.push(check(
            "shims",
            !required,
            if required {
                format!("{} missing", path.display())
            } else {
                "not required for this provider".into()
            },
        ));
        return;
    };
    let defined = defined_functions(&text);
    let missing: Vec<&str> = SHIM_FUNCTIONS
        .iter()
        .copied()
        .filter(|f| !defined.iter().any(|d| d == f))
        .collect();
    out.push(check(
        "shims",
        missing.is_empty(),
        if missing.is_empty() {
            format!(
                "{} defines all {} functions",
                path.display(),
                SHIM_FUNCTIONS.len()
            )
        } else {
            format!("{} lacks: {}", path.display(), missing.join(", "))
        },
    ));
}

pub fn health_checks(
    config: &CompressionConfig,
    paths: &Paths,
    probe: &dyn HealthProbe,
) -> Vec<Check> {
    let mut out = Vec::new();
    let health = provider_checks(config, probe, &mut out);
    pin_checks(config, &mut out);
    shim_checks(config, paths, &mut out);
    if let Some(health) = health {
        fingerprint_checks(config, &health, &mut out);
    }
    out
}
