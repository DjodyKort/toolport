//! Ledger, verify and engine-lifecycle tests on synthetic transcripts, launches and fakes.
//! Mirrors mcpm-compression's test_ledger and test_verify, plus the provider-agnostic checks.

use super::engine::*;
use super::launch::{LedgerEntry, Probe, SystemOps};
use super::ledger::*;
use super::model::*;
use super::store::Paths;
use super::verify::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "tp-cmp-verify-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }

    fn paths(&self) -> Paths {
        Paths::new(&self.0)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn entry(cwd: &str, provider: ProviderName, routed: bool, pin: &str) -> LedgerEntry {
    LedgerEntry {
        cwd: cwd.into(),
        provider,
        preset: "interactive".into(),
        routed,
        port: routed.then_some(8787),
        pin: pin.into(),
    }
}

const T0: &str = "2026-10-03T12:00:00.000Z";

fn t0() -> i64 {
    parse_ts(T0).unwrap()
}

fn write_transcript(
    root: &Path,
    name: &str,
    start_ms: i64,
    cwd: &str,
    turns: usize,
    read: u64,
    create: u64,
) -> PathBuf {
    let dir = root.join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    let mut text = format!(
        "{}\n",
        json!({"timestamp": format_ts(start_ms), "cwd": cwd, "type": "user"})
    );
    for _ in 0..turns {
        text.push_str(&format!(
            "{}\n",
            json!({"message": {"usage": {
                "cache_read_input_tokens": read,
                "cache_creation_input_tokens": create,
                "input_tokens": 10,
                "output_tokens": 20,
            }}})
        ));
    }
    text.push_str("{\"message\": {\"usage\": {\"cache_read");
    let path = dir.join(format!("{name}.jsonl"));
    std::fs::write(&path, text).unwrap();
    path
}

// ---- timestamps ----

#[test]
fn timestamps_round_trip_and_honour_offsets() {
    assert_eq!(format_ts(t0()), T0);
    assert_eq!(parse_ts("2026-10-03T14:00:00+02:00"), Some(t0()));
    assert_eq!(parse_ts("2026-10-03T12:00:00Z"), Some(t0()));
    assert_eq!(parse_ts("2026-10-03T12:00:00.5Z"), Some(t0() + 500));
    assert_eq!(parse_ts("2026-10-03T12:00:00"), Some(t0()));
    assert_eq!(parse_ts("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_ts("not a time"), None);
    assert_eq!(parse_ts(""), None);
    assert_eq!(format_ts(0), "1970-01-01T00:00:00.000Z");
}

// ---- ledger ----

#[test]
fn append_launch_is_append_only_private_and_tolerates_a_torn_line() {
    let tmp = Tmp::new("append");
    let paths = tmp.paths();
    append_launch(
        &paths,
        &entry("/work/a/", ProviderName::Headroom, true, "0.29.0"),
        t0(),
    )
    .unwrap();
    append_launch(
        &paths,
        &entry("/work/b", ProviderName::None, false, "0.29.0"),
        t0() + 1000,
    )
    .unwrap();
    {
        use std::io::Write;
        let mut f = crate::registry::open_append_private(&paths.launches()).unwrap();
        f.write_all(b"{\"ts\": \"2026-").unwrap();
    }
    let rows = read_launches(&paths);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].cwd, "/work/a");
    assert_eq!(rows[0].provider, "headroom");
    assert!(rows[0].routed);
    assert_eq!(rows[0].port, Some(8787));
    assert_eq!(rows[1].port, None);
    assert_eq!(rows[1].pin.as_deref(), Some("0.29.0"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(paths.launches())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn missing_ledger_reads_empty() {
    let tmp = Tmp::new("empty");
    assert!(read_launches(&tmp.paths()).is_empty());
    assert!(read_savings(&tmp.paths()).is_empty());
}

fn rec(ts: i64, cwd: &str, routed: bool) -> LaunchRecord {
    LaunchRecord {
        ts: format_ts(ts),
        cwd: cwd.into(),
        provider: "headroom".into(),
        preset: "interactive".into(),
        routed,
        port: None,
        pin: None,
    }
}

#[test]
fn attribution_needs_same_cwd_and_a_start_inside_the_window() {
    let launches = vec![rec(t0(), "/work/a", true)];
    assert!(attribute(Some(t0() + 2_000), Some("/work/a"), &launches).is_some());
    assert!(attribute(Some(t0() + MATCH_WINDOW_MS), Some("/work/a"), &launches).is_some());
    assert!(attribute(Some(t0() + MATCH_WINDOW_MS + 1), Some("/work/a"), &launches).is_none());
    assert!(attribute(Some(t0() - 1), Some("/work/a"), &launches).is_none());
    assert!(attribute(Some(t0() + 2_000), Some("/work/other"), &launches).is_none());
    assert!(attribute(None, Some("/work/a"), &launches).is_none());
    assert!(attribute(Some(t0()), None, &launches).is_none());
}

#[test]
fn attribution_takes_the_latest_qualifying_launch() {
    let launches = vec![
        rec(t0(), "/work/a", true),
        rec(t0() + 30_000, "/work/a", false),
    ];
    let hit = attribute(Some(t0() + 40_000), Some("/work/a"), &launches).unwrap();
    assert!(!hit.routed);
}

#[cfg(unix)]
#[test]
fn attribution_resolves_symlinked_directories() {
    let tmp = Tmp::new("symlink");
    let real = tmp.0.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = tmp.0.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let launches = vec![rec(t0(), link.to_str().unwrap(), true)];
    assert!(attribute(Some(t0() + 1000), Some(real.to_str().unwrap()), &launches).is_some());
}

#[test]
fn summary_groups_by_provider_and_filters() {
    let tmp = Tmp::new("summary");
    let paths = tmp.paths();
    append_launch(
        &paths,
        &entry("/a", ProviderName::Headroom, true, "0.29.0"),
        t0(),
    )
    .unwrap();
    append_launch(
        &paths,
        &entry("/a", ProviderName::Headroom, false, "0.29.0"),
        t0() + 1,
    )
    .unwrap();
    append_launch(
        &paths,
        &entry("/a", ProviderName::RtkOnly, false, "0.29.0"),
        t0() + 2,
    )
    .unwrap();
    let sav = |provider: &str, ts: i64, before, after| SavingsEntry {
        ts: format_ts(ts),
        provider: provider.into(),
        source: "test".into(),
        session: None,
        tokens_before: before,
        tokens_after: after,
    };
    append_savings(&paths, &sav("rtk-only", t0(), 1000, 400)).unwrap();
    append_savings(&paths, &sav("rtk-only", t0() + 10, 500, 500)).unwrap();
    append_savings(&paths, &sav("headroom", t0() - 86_400_000, 2000, 1000)).unwrap();

    let all = summarize(&read_launches(&paths), &read_savings(&paths), None, None);
    let names: Vec<&str> = all.iter().map(|r| r.provider.as_str()).collect();
    assert_eq!(names, ["headroom", "rtk-only"]);
    let hr = &all[0];
    assert_eq!((hr.launches, hr.routed, hr.plain), (2, 1, 1));
    assert_eq!(hr.saved(), 1000);
    assert_eq!(hr.saved_percent(), Some(50.0));
    let rtk = &all[1];
    assert_eq!(
        (rtk.savings_entries, rtk.tokens_before, rtk.tokens_after),
        (2, 1500, 900)
    );
    assert_eq!(rtk.saved(), 600);

    let only = summarize(
        &read_launches(&paths),
        &read_savings(&paths),
        Some("rtk-only"),
        None,
    );
    assert_eq!(only.len(), 1);
    let since = summarize(
        &read_launches(&paths),
        &read_savings(&paths),
        None,
        Some(t0()),
    );
    let hr_since = since.iter().find(|r| r.provider == "headroom").unwrap();
    assert_eq!(hr_since.savings_entries, 0);
    assert_eq!(hr_since.launches, 2);
    assert_eq!(ProviderSummary::default().saved_percent(), None);
}

// ---- transcripts ----

#[test]
fn measure_sums_billed_usage_and_skips_short_sessions_and_torn_tails() {
    let tmp = Tmp::new("measure");
    let long = write_transcript(&tmp.0, "long", t0(), "/work/a", 6, 9_000, 1_000);
    let short = write_transcript(&tmp.0, "short", t0(), "/work/a", 2, 9_000, 1_000);
    let m = measure(&[long, short], MIN_TURNS);
    assert_eq!((m.sessions, m.turns), (1, 6));
    assert_eq!((m.cache_read, m.cache_create), (54_000, 6_000));
    assert_eq!((m.input_tokens, m.output_tokens), (60, 120));
    assert_eq!(m.read_ratio(), Some(0.9));
    assert_eq!(m.read_write(), Some(9.0));
}

#[test]
fn verdict_fails_unknown_low_ratio_and_low_read_write() {
    assert!(!Metrics::default().verdict().0);
    let busted = Metrics {
        cache_read: 80,
        cache_create: 20,
        ..Metrics::default()
    };
    let (ok, why) = busted.verdict();
    assert!(!ok && why.contains("prefix is being busted"), "{why}");
    let churn = Metrics {
        cache_read: 920,
        cache_create: 80,
        ..Metrics::default()
    };
    let (ok, why) = churn.verdict();
    assert!(!ok && why.contains("re-creation"), "{why}");
    let good = Metrics {
        cache_read: 965,
        cache_create: 35,
        ..Metrics::default()
    };
    assert!(good.verdict().0);
    let no_writes = Metrics {
        cache_read: 100,
        ..Metrics::default()
    };
    assert_eq!(no_writes.verdict(), (true, "ok".into()));
}

#[test]
fn partition_splits_proxied_plain_and_unattributed() {
    let tmp = Tmp::new("partition");
    let routed = write_transcript(&tmp.0, "routed", t0() + 2_000, "/work/a", 6, 900, 100);
    let plain = write_transcript(&tmp.0, "plain", t0() + 2_000, "/work/b", 6, 900, 100);
    let stray = write_transcript(&tmp.0, "stray", t0() + 500_000, "/work/a", 6, 900, 100);
    let launches = vec![rec(t0(), "/work/a", true), rec(t0(), "/work/b", false)];
    let b = partition(&[routed.clone(), plain.clone(), stray.clone()], &launches);
    assert_eq!(b.proxied, [routed]);
    assert_eq!(b.plain, [plain]);
    assert_eq!(b.unattributed, [stray]);
}

#[test]
fn partition_by_pin_keys_routed_sessions_and_defaults_to_unknown() {
    let tmp = Tmp::new("bypin");
    let a = write_transcript(&tmp.0, "a", t0() + 1_000, "/work/a", 6, 900, 100);
    let b = write_transcript(&tmp.0, "b", t0() + 1_000, "/work/b", 6, 900, 100);
    let c = write_transcript(&tmp.0, "c", t0() + 1_000, "/work/c", 6, 900, 100);
    let mut with_pin = rec(t0(), "/work/a", true);
    with_pin.pin = Some("0.29.0".into());
    let launches = vec![
        with_pin,
        rec(t0(), "/work/b", true),
        rec(t0(), "/work/c", false),
    ];
    let groups = partition_by_pin(&[a.clone(), b.clone(), c], &launches);
    assert_eq!(groups.keys().collect::<Vec<_>>(), ["0.29.0", "unknown"]);
    assert_eq!(groups["0.29.0"], [a]);
    assert_eq!(groups["unknown"], [b]);
}

#[test]
fn compare_is_a_percentage_point_delta_and_none_when_empty() {
    let a = Metrics {
        cache_read: 90,
        cache_create: 10,
        ..Metrics::default()
    };
    let b = Metrics {
        cache_read: 95,
        cache_create: 5,
        ..Metrics::default()
    };
    assert!((compare(&a, &b).unwrap() - 5.0).abs() < 1e-9);
    assert_eq!(compare(&Metrics::default(), &b), None);
}

#[test]
fn schema_ok_requires_the_cache_fields() {
    let tmp = Tmp::new("schema");
    let good = write_transcript(&tmp.0, "good", t0(), "/w", 3, 1, 1);
    assert!(schema_ok(&[good]));
    let dir = tmp.0.join("other");
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("bad.jsonl");
    std::fs::write(&bad, "{\"message\":{\"usage\":{\"input_tokens\":5}}}\n").unwrap();
    assert!(!schema_ok(&[bad]));
    assert!(!schema_ok(&[]));
}

#[test]
fn iter_transcripts_lists_project_jsonl_files_only() {
    let tmp = Tmp::new("iter");
    let one = write_transcript(&tmp.0, "one", t0(), "/w", 1, 1, 1);
    std::fs::write(tmp.0.join("proj").join("notes.txt"), "x").unwrap();
    std::fs::write(tmp.0.join("top.jsonl"), "{}").unwrap();
    assert_eq!(iter_transcripts(&tmp.0), [one]);
    assert!(iter_transcripts(&tmp.0.join("absent")).is_empty());
}

// ---- health checks ----

struct FakeHealth {
    binaries: Vec<&'static str>,
    version: Option<&'static str>,
    health: Option<Value>,
}

impl HealthProbe for FakeHealth {
    fn binary(&self, name: &str) -> Option<String> {
        self.binaries
            .contains(&name)
            .then(|| format!("/usr/bin/{name}"))
    }

    fn installed_version(&self) -> Option<String> {
        self.version.map(str::to_string)
    }

    fn proxy_health(&self, _port: u16) -> Option<Value> {
        self.health.clone()
    }
}

fn healthy(version: &str) -> Value {
    let config: serde_json::Map<String, Value> = HEALTH_CONFIG_KEYS
        .iter()
        .map(|k| (k.to_string(), Value::Null))
        .collect();
    json!({
        "service": "headroom-proxy",
        "status": "healthy",
        "ready": true,
        "version": version,
        "config": config,
    })
}

fn names_failing(checks: &[Check]) -> Vec<&str> {
    checks
        .iter()
        .filter(|c| !c.ok)
        .map(|c| c.name.as_str())
        .collect()
}

fn headroom_cfg() -> CompressionConfig {
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    for (_, preset) in cfg.presets.iter_mut() {
        preset.snapshot_version = Some(cfg.provider_version.pin.clone());
    }
    cfg
}

#[test]
fn disabled_provider_passes_without_shims() {
    let tmp = Tmp::new("none");
    let cfg = CompressionConfig::with_provider(ProviderName::None);
    let probe = FakeHealth {
        binaries: vec![],
        version: None,
        health: None,
    };
    let checks = health_checks(&cfg, &tmp.paths(), &probe);
    assert!(names_failing(&checks).is_empty(), "{checks:?}");
}

#[test]
fn rtk_only_needs_the_binary_but_not_pin_or_shims() {
    let tmp = Tmp::new("rtk");
    let cfg = CompressionConfig::with_provider(ProviderName::RtkOnly);
    let up = FakeHealth {
        binaries: vec!["rtk"],
        version: None,
        health: None,
    };
    assert!(names_failing(&health_checks(&cfg, &tmp.paths(), &up)).is_empty());
    let down = FakeHealth {
        binaries: vec![],
        version: None,
        health: None,
    };
    assert_eq!(
        names_failing(&health_checks(&cfg, &tmp.paths(), &down)),
        ["engine"]
    );
}

#[test]
fn headroom_passes_when_pinned_ready_fingerprinted_and_shimmed() {
    let tmp = Tmp::new("hr-ok");
    let paths = tmp.paths();
    let cfg = headroom_cfg();
    std::fs::write(
        paths.shims(),
        super::shims::shim_snippet(super::shims::ShimOptions::default()),
    )
    .unwrap();
    let probe = FakeHealth {
        binaries: vec!["headroom"],
        version: Some("0.29.0"),
        health: Some(healthy("0.29.0")),
    };
    let checks = health_checks(&cfg, &paths, &probe);
    assert!(names_failing(&checks).is_empty(), "{checks:?}");
    let fp = checks
        .iter()
        .find(|c| c.name == "engine fingerprint")
        .unwrap();
    assert!(fp.detail.contains("contract keys present"), "{}", fp.detail);
}

#[test]
fn headroom_reports_drift_missing_shims_and_an_unreachable_proxy() {
    let tmp = Tmp::new("hr-bad");
    let cfg = headroom_cfg();
    let probe = FakeHealth {
        binaries: vec!["headroom"],
        version: Some("0.31.0"),
        health: None,
    };
    let failing: Vec<String> = health_checks(&cfg, &tmp.paths(), &probe)
        .into_iter()
        .filter(|c| !c.ok)
        .map(|c| c.name)
        .collect();
    assert_eq!(failing, ["engine version", "engine reachable", "shims"]);
}

#[test]
fn partial_shims_name_the_missing_functions() {
    let tmp = Tmp::new("shims");
    let paths = tmp.paths();
    std::fs::write(paths.shims(), "hrclaude() { :; }\nhrup() { :; }\n").unwrap();
    let cfg = headroom_cfg();
    let probe = FakeHealth {
        binaries: vec!["headroom"],
        version: Some("0.29.0"),
        health: None,
    };
    let checks = health_checks(&cfg, &paths, &probe);
    let shims = checks.iter().find(|c| c.name == "shims").unwrap();
    assert!(!shims.ok && shims.detail.contains("hrdown") && !shims.detail.contains("hrclaude,"));
}

#[test]
fn fingerprint_flags_a_changed_version_and_lost_contract_keys() {
    let tmp = Tmp::new("fp");
    let cfg = headroom_cfg();
    let mut moved = healthy("0.31.0");
    moved["config"]
        .as_object_mut()
        .unwrap()
        .remove("target_ratio");
    let probe = FakeHealth {
        binaries: vec!["headroom"],
        version: Some("0.29.0"),
        health: Some(moved.clone()),
    };
    let checks = health_checks(&cfg, &tmp.paths(), &probe);
    let fp = checks
        .iter()
        .find(|c| c.name == "engine fingerprint")
        .unwrap();
    assert!(
        !fp.ok && fp.detail.contains("target_ratio"),
        "{}",
        fp.detail
    );

    let fp = fingerprint(&moved);
    assert_eq!(fp.missing_config, ["target_ratio"]);
    assert_eq!(fp.version.as_deref(), Some("0.31.0"));
    assert_eq!(fp.digest.len(), 16);
    assert_ne!(fp.digest, fingerprint(&healthy("0.31.0")).digest);
    assert_eq!(
        fingerprint(&healthy("1.0.0")).digest,
        fingerprint(&healthy("2.0.0")).digest
    );
    assert_eq!(fingerprint(&json!({})).missing_required.len(), 4);
}

#[test]
fn sealed_posture_flags_vendor_decided_knobs_a_live_proxy_holds() {
    let tmp = Tmp::new("seal");
    let cfg = headroom_cfg();
    let mut live = healthy("0.29.0");
    live["config"]["protect_recent"] = json!(4);
    let probe = FakeHealth {
        binaries: vec!["headroom"],
        version: Some("0.29.0"),
        health: Some(live),
    };
    let checks = health_checks(&cfg, &tmp.paths(), &probe);
    let sealed = checks.iter().find(|c| c.name == "sealed posture").unwrap();
    assert!(
        !sealed.ok && sealed.detail.contains("HEADROOM_PROTECT_RECENT=4"),
        "{}",
        sealed.detail
    );
}

#[test]
fn stale_snapshots_and_a_bad_pin_fail_the_pin_checks() {
    let tmp = Tmp::new("pins");
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    let probe = FakeHealth {
        binaries: vec!["headroom"],
        version: None,
        health: None,
    };
    let checks = health_checks(&cfg, &tmp.paths(), &probe);
    assert!(names_failing(&checks).contains(&"preset provenance"));
    cfg.provider_version.pin = "latest".into();
    assert!(names_failing(&health_checks(&cfg, &tmp.paths(), &probe)).contains(&"pin"));
}

// ---- engine lifecycle ----

#[derive(Default)]
struct FakeEngine {
    health_after_spawns: Option<usize>,
    health: Option<Value>,
    spawned: Vec<(u16, String, BTreeMap<String, String>)>,
    pids: Vec<u32>,
    killed: Vec<u32>,
    slept: u64,
    installed: Option<String>,
    install_to: Option<String>,
    installs: Vec<String>,
    latest: Option<String>,
    profiles: BTreeMap<String, Vec<(String, String)>>,
    spawn_error: Option<String>,
    version_probes: std::cell::Cell<usize>,
}

impl EngineOps for FakeEngine {
    fn proxy_health(&self, _port: u16) -> Option<Value> {
        match self.health_after_spawns {
            Some(0) => self.health.clone(),
            Some(n) if self.slept >= n as u64 && !self.spawned.is_empty() => self.health.clone(),
            Some(_) => None,
            None => self.health.clone(),
        }
    }

    fn spawn_proxy(
        &mut self,
        port: u16,
        mode: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if let Some(why) = &self.spawn_error {
            return Err(why.clone());
        }
        self.spawned.push((port, mode.into(), env.clone()));
        Ok(())
    }

    fn listening_pids(&self, _port: u16) -> Vec<u32> {
        self.pids.clone()
    }

    fn terminate(&mut self, pid: u32) -> Result<(), String> {
        self.killed.push(pid);
        Ok(())
    }

    fn sleep(&mut self, seconds: u64) {
        self.slept += seconds;
    }

    fn installed_version(&self) -> Option<String> {
        self.version_probes.set(self.version_probes.get() + 1);
        self.install_to.clone().or_else(|| self.installed.clone())
    }

    fn latest_version(&self, _package: &str) -> Option<String> {
        self.latest.clone()
    }

    fn install(&mut self, requirement: &str) -> Result<(Option<String>, Option<String>), String> {
        self.installs.push(requirement.into());
        Ok((
            self.installed.clone(),
            self.install_to.clone().or_else(|| self.installed.clone()),
        ))
    }

    fn agent_savings(&self, profile: &str) -> Result<Vec<(String, String)>, String> {
        self.profiles
            .get(profile)
            .cloned()
            .ok_or_else(|| format!("no profile {profile}"))
    }
}

fn env_for(cfg: &CompressionConfig) -> OrderedMap<String> {
    env_for_preset(cfg, &cfg.preset_for(None))
}

#[test]
fn proxy_up_reuses_a_ready_proxy() {
    let cfg = headroom_cfg();
    let mut ops = FakeEngine {
        health: Some(healthy("0.29.0")),
        ..FakeEngine::default()
    };
    let msg = proxy_up(&mut ops, 8787, &env_for(&cfg), 5).unwrap();
    assert_eq!(msg, "reusing proxy on :8787");
    assert!(ops.spawned.is_empty());
}

#[test]
fn proxy_up_spawns_with_the_preset_env_and_waits_for_health() {
    let cfg = headroom_cfg();
    let mut ops = FakeEngine {
        health: Some(healthy("0.29.0")),
        health_after_spawns: Some(2),
        ..FakeEngine::default()
    };
    let msg = proxy_up(&mut ops, 8787, &env_for(&cfg), 5).unwrap();
    assert!(msg.starts_with("started proxy on :8787 (mode="), "{msg}");
    assert_eq!(ops.slept, 2);
    let (port, mode, env) = &ops.spawned[0];
    assert_eq!(*port, 8787);
    assert_eq!(mode, &env["HEADROOM_MODE"]);
    assert_eq!(env["ANTHROPIC_BASE_URL"], "http://127.0.0.1:8787");
}

#[test]
fn proxy_up_times_out_and_surfaces_spawn_errors() {
    let cfg = headroom_cfg();
    let mut never = FakeEngine {
        health_after_spawns: Some(1000),
        ..FakeEngine::default()
    };
    let err = proxy_up(&mut never, 8787, &env_for(&cfg), 3).unwrap_err();
    assert!(err.contains("did not become ready in 3s"), "{err}");
    let mut broken = FakeEngine {
        spawn_error: Some("headroom not on PATH".into()),
        ..FakeEngine::default()
    };
    assert_eq!(
        proxy_up(&mut broken, 8787, &env_for(&cfg), 3).unwrap_err(),
        "headroom not on PATH"
    );
}

#[test]
fn proxy_down_prefers_the_reported_pid_then_the_listener() {
    let mut reported = FakeEngine {
        health: Some(json!({"ready": true, "config": {"pid": 4242}})),
        pids: vec![1],
        ..FakeEngine::default()
    };
    assert_eq!(
        proxy_down(&mut reported, 8787).unwrap(),
        "stopped proxy on :8787 (4242)"
    );
    assert_eq!(reported.killed, [4242]);

    let mut listener = FakeEngine {
        pids: vec![7, 8],
        ..FakeEngine::default()
    };
    assert_eq!(
        proxy_down(&mut listener, 8787).unwrap(),
        "stopped proxy on :8787 (7, 8)"
    );

    let mut none = FakeEngine::default();
    assert!(proxy_down(&mut none, 8787)
        .unwrap_err()
        .contains("no proxy listening"));
}

#[test]
fn resolve_target_uses_the_lookup_only_without_an_explicit_version() {
    let cfg = headroom_cfg();
    let ops = FakeEngine {
        latest: Some("0.32.1".into()),
        ..FakeEngine::default()
    };
    let t = resolve_target(&cfg, &ops, None, true).unwrap();
    assert_eq!(
        (t.current.as_str(), t.target.as_str(), t.same),
        ("0.29.0", "0.32.1", false)
    );
    assert!(
        resolve_target(&cfg, &ops, Some("0.29.0"), false)
            .unwrap()
            .same
    );
    let offline = FakeEngine::default();
    assert_eq!(
        resolve_target(&cfg, &offline, None, true).unwrap_err(),
        super::ops::UpdateError::Unresolvable
    );
    assert_eq!(
        resolve_target(&cfg, &ops, Some("0.30.0"), true).unwrap_err(),
        super::ops::UpdateError::Conflict
    );
    assert!(matches!(
        resolve_target(&cfg, &ops, Some("soon"), false).unwrap_err(),
        super::ops::UpdateError::Unparseable(_)
    ));
}

#[test]
fn apply_update_pins_installs_exactly_and_resnapshots_profile_presets() {
    let mut cfg = headroom_cfg();
    let mut ops = FakeEngine {
        installed: Some("0.29.0".into()),
        install_to: Some("0.30.0".into()),
        ..FakeEngine::default()
    };
    ops.profiles.insert(
        "agent-90".into(),
        vec![("HEADROOM_TARGET_RATIO".into(), "0.5".into())],
    );
    let report = apply_update(&mut cfg, "0.30.0", &mut ops).unwrap();
    assert_eq!(cfg.provider_version.pin, "0.30.0");
    assert_eq!(ops.installs, ["headroom-ai[proxy,code,ml]==0.30.0"]);
    assert_eq!(report.install_detail, "0.29.0 -> 0.30.0");
    assert!(report.version_changed);
    assert_eq!(ops.version_probes.get(), 0, "the install result already carries the new version");
    let agent = cfg.presets.get("agent").unwrap();
    assert_eq!(agent.snapshot_version.as_deref(), Some("0.30.0"));
    assert_eq!(
        agent.knobs.get("HEADROOM_TARGET_RATIO").unwrap().value,
        "0.5"
    );
    assert!(report
        .snapshot_notes
        .iter()
        .any(|n| n.starts_with("preset 'agent': +1")));
    assert!(
        report
            .snapshot_notes
            .iter()
            .any(|n| n.contains("snapshot not refreshed")),
        "{:?}",
        report.snapshot_notes
    );
}

// ---- system seams ----

#[test]
fn system_probe_finds_binaries_on_the_overridden_path_and_reads_health() {
    let tmp = Tmp::new("system");
    let bin = tmp.0.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tool = bin.join("rtk");
    #[cfg(unix)]
    crate::plus::testutil::exec::write_executable(&tool, "#!/bin/sh\necho rtk\n");
    #[cfg(not(unix))]
    std::fs::write(&tool, "#!/bin/sh\necho rtk\n").unwrap();
    let ops = SystemOps {
        path: Some(bin.clone().into_os_string()),
    };
    assert_eq!(
        HealthProbe::binary(&ops, "rtk"),
        Some(tool.to_string_lossy().into_owned())
    );
    assert_eq!(HealthProbe::binary(&ops, "absent"), None);
    assert_eq!(Probe::headroom_version(&ops), None);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let body = "{\"ready\":true,\"version\":\"0.29.0\"}";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    let health = HealthProbe::proxy_health(&ops, port).unwrap();
    assert_eq!(health["version"], "0.29.0");
    server.join().unwrap();
    assert!(HealthProbe::proxy_health(&ops, port).is_none());
}

#[test]
fn store_paths_name_the_two_ledger_files() {
    let paths = Paths::new("/data");
    assert_eq!(
        paths.launches(),
        PathBuf::from("/data/compression-launches.jsonl")
    );
    assert_eq!(
        paths.savings(),
        PathBuf::from("/data/compression-savings.jsonl")
    );
}
