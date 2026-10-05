use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::scan::{report_value, run, ProbeRun, Selector, MAX_PARALLEL};
use super::surfaces;
use super::*;

const T0: i64 = 3_000_000;
const HTTP_EVERY: i64 = 600;
const STDIO_EVERY: i64 = 30 * 60;
const EVERY: &[&str] = &["files", "gdocs-a", "gdocs-c", "slack"];

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "auth-scan-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn registry() -> ProbeRegistry {
    let mut reg = ProbeRegistry::new();
    reg.register(ProbeSpec::new("slack", ProbeKind::Http).with_min_interval(HTTP_EVERY));
    reg.register(ProbeSpec::new("files", ProbeKind::Stdio));
    reg.register(ProbeSpec::new("gdocs-a", ProbeKind::GoogleRefresh).with_profile("work"));
    reg.register(ProbeSpec::new("gdocs-b", ProbeKind::GoogleRefresh).with_profile("work"));
    reg.register(ProbeSpec::new("gdocs-c", ProbeKind::GoogleRefresh).with_profile("home"));
    reg
}

struct Rig {
    dir: PathBuf,
    clock: Arc<FakeClock>,
    probe: Arc<MockProbe>,
    prober: AuthProber,
}

fn rig(tag: &str, registry: ProbeRegistry, probe: MockProbe) -> Rig {
    let dir = scratch(tag);
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(probe);
    let prober = AuthProber::new(AuthStore::new(&dir), registry, probe.clone(), clock.clone());
    Rig {
        dir,
        clock,
        probe,
        prober,
    }
}

fn ran(run: &ProbeRun) -> Vec<&str> {
    run.reports
        .iter()
        .filter(|report| report.ran)
        .map(|report| report.server.as_str())
        .collect()
}

#[test]
fn the_due_scan_follows_the_cache_with_a_fake_clock() {
    let rig = rig(
        "table",
        registry(),
        MockProbe::always(ProbeOutcome::Success),
    );
    struct Step {
        name: &'static str,
        advance: i64,
        selector: Selector,
        force: bool,
        ran: &'static [&'static str],
        skipped: Option<&'static str>,
    }
    let steps = [
        Step {
            name: "first scan runs everything once",
            advance: 0,
            selector: Selector::Due,
            force: false,
            ran: EVERY,
            skipped: None,
        },
        Step {
            name: "nothing is due right after",
            advance: 0,
            selector: Selector::Due,
            force: false,
            ran: &[],
            skipped: None,
        },
        Step {
            name: "one second short of the http cadence",
            advance: HTTP_EVERY - 1,
            selector: Selector::Due,
            force: false,
            ran: &[],
            skipped: None,
        },
        Step {
            name: "http is due on its cadence",
            advance: 1,
            selector: Selector::Due,
            force: false,
            ran: &["slack"],
            skipped: None,
        },
        Step {
            name: "stdio joins at its slower cadence",
            advance: STDIO_EVERY - HTTP_EVERY,
            selector: Selector::Due,
            force: false,
            ran: &["files", "slack"],
            skipped: None,
        },
        Step {
            name: "a scheduled request honours the gate",
            advance: 0,
            selector: Selector::One("slack".into()),
            force: false,
            ran: &[],
            skipped: Some("not_due"),
        },
        Step {
            name: "force ignores the cache for one server",
            advance: 0,
            selector: Selector::One("slack".into()),
            force: true,
            ran: &["slack"],
            skipped: None,
        },
        Step {
            name: "force on the whole set probes each gate once",
            advance: 0,
            selector: Selector::All,
            force: true,
            ran: EVERY,
            skipped: None,
        },
        Step {
            name: "google comes due after its 6 h gate",
            advance: 6 * 3600,
            selector: Selector::Due,
            force: false,
            ran: EVERY,
            skipped: None,
        },
    ];
    for step in steps {
        rig.clock.advance(step.advance);
        let result = run(&rig.prober, &step.selector, step.force, MAX_PARALLEL).unwrap();
        assert_eq!(ran(&result), step.ran, "{}", step.name);
        if let Some(reason) = step.skipped {
            assert_eq!(result.reports[0].skipped, Some(reason), "{}", step.name);
        }
        assert!(result.failures.is_empty(), "{}", step.name);
    }
    let _ = std::fs::remove_dir_all(&rig.dir);
}

#[test]
fn one_google_profile_is_probed_once_even_with_two_servers() {
    let rig = rig(
        "profile",
        registry(),
        MockProbe::always(ProbeOutcome::Success),
    );
    run(&rig.prober, &Selector::Due, false, MAX_PARALLEL).unwrap();
    let mut called = rig.probe.called_servers();
    called.sort();
    assert_eq!(called, ["files", "gdocs-a", "gdocs-c", "slack"]);
    assert_eq!(
        rig.prober.registered(),
        ["files", "gdocs-a", "gdocs-c", "slack"]
    );
    let _ = std::fs::remove_dir_all(&rig.dir);
}

#[test]
fn failures_back_off_and_a_forced_probe_overrides_the_wait() {
    let mut reg = ProbeRegistry::new();
    reg.register(ProbeSpec::new("slack", ProbeKind::Http).with_min_interval(HTTP_EVERY));
    let rig = rig(
        "backoff",
        reg,
        MockProbe::always(ProbeOutcome::TransportError),
    );
    let next_due = |result: &ProbeRun| result.reports[0].next_due_at;

    let first = run(&rig.prober, &Selector::Due, false, 1).unwrap();
    assert_eq!(ran(&first), ["slack"]);
    assert_eq!(next_due(&first), T0 + HTTP_EVERY);

    rig.clock.advance(HTTP_EVERY);
    let second = run(&rig.prober, &Selector::Due, false, 1).unwrap();
    assert_eq!(ran(&second), ["slack"]);
    assert_eq!(next_due(&second), T0 + HTTP_EVERY + 2 * HTTP_EVERY);

    rig.clock.advance(HTTP_EVERY);
    let waiting = run(&rig.prober, &Selector::Due, false, 1).unwrap();
    assert!(waiting.reports.is_empty(), "{waiting:?}");
    assert_eq!(rig.probe.call_count(), 2);

    let forced = run(&rig.prober, &Selector::One("slack".into()), true, 1).unwrap();
    assert_eq!(ran(&forced), ["slack"]);
    assert_eq!(rig.probe.call_count(), 3);
    assert_eq!(forced.reports[0].tracked.state, AuthState::Unknown);
    assert_eq!(next_due(&forced), T0 + 2 * HTTP_EVERY + 4 * HTTP_EVERY);

    rig.clock.advance(HTTP_EVERY);
    let settled = run(&rig.prober, &Selector::One("slack".into()), true, 1).unwrap();
    assert_eq!(
        settled.reports[0].tracked.state,
        AuthState::Unreachable,
        "repeated transient failures over 30 minutes"
    );
    let _ = std::fs::remove_dir_all(&rig.dir);
}

#[test]
fn an_empty_registry_scans_and_writes_nothing() {
    let rig = rig(
        "empty",
        ProbeRegistry::new(),
        MockProbe::always(ProbeOutcome::Success),
    );
    for selector in [Selector::Due, Selector::All] {
        let result = run(&rig.prober, &selector, false, MAX_PARALLEL).unwrap();
        assert!(result.reports.is_empty() && result.failures.is_empty());
    }
    assert_eq!(rig.probe.call_count(), 0);
    assert!(!rig.dir.exists(), "the auth directory was created");
}

#[test]
fn an_unknown_server_is_an_error_and_writes_nothing() {
    let rig = rig(
        "unknown",
        registry(),
        MockProbe::always(ProbeOutcome::Success),
    );
    let error = run(&rig.prober, &Selector::One("nope".into()), true, 1).unwrap_err();
    assert!(error.contains("no probe registered for nope"), "{error}");
    assert!(!rig.dir.exists());
}

#[test]
fn concurrency_stays_within_the_bound() {
    let mut reg = ProbeRegistry::new();
    for index in 0..8 {
        reg.register(ProbeSpec::new(&format!("srv-{index}"), ProbeKind::Http));
    }
    let live = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let gate = crate::plus::testutil::Gate::new(2);
    let (live_in, peak_in, gate_in) = (live.clone(), peak.clone(), gate.clone());
    let probe = MockProbe::always(ProbeOutcome::Success).on_run(move || {
        let now = live_in.fetch_add(1, Ordering::SeqCst) + 1;
        peak_in.fetch_max(now, Ordering::SeqCst);
        gate_in.pass();
        std::thread::sleep(Duration::from_millis(40));
        live_in.fetch_sub(1, Ordering::SeqCst);
    });
    let rig = rig("bound", reg, probe);
    let result = run(&rig.prober, &Selector::Due, false, 2).unwrap();
    assert_eq!(result.probed(), 8);
    assert!(!gate.timed_out(), "two probes never ran at the same time");
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    let order: Vec<&str> = result.reports.iter().map(|r| r.server.as_str()).collect();
    let expected: Vec<String> = (0..8).map(|i| format!("srv-{i}")).collect();
    assert_eq!(order, expected);
    let _ = std::fs::remove_dir_all(&rig.dir);
}

struct Boom;

impl Probe for Boom {
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome {
        if spec.server == "boom" {
            panic!("probe exploded");
        }
        ProbeOutcome::Success
    }
}

#[test]
fn a_panicking_probe_is_isolated_and_reported() {
    let mut reg = ProbeRegistry::new();
    for server in ["alpha", "boom", "omega"] {
        reg.register(ProbeSpec::new(server, ProbeKind::Http));
    }
    let dir = scratch("panic");
    let clock = Arc::new(FakeClock::new(T0));
    let prober = AuthProber::new(AuthStore::new(&dir), reg, Arc::new(Boom), clock);
    let result = run(&prober, &Selector::Due, false, 2).unwrap();
    assert_eq!(ran(&result), ["alpha", "omega"]);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].server, "boom");
    assert_eq!(result.failures[0].error, "probe panicked");
    assert_eq!(result.servers(), ["alpha", "omega", "boom"]);
    let again = run(&prober, &Selector::Due, false, 2).unwrap();
    assert!(
        again.reports.is_empty() && again.failures.is_empty(),
        "no hot retry: {again:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_report_carries_counts_and_rows_for_the_probed_servers() {
    let mut reg = ProbeRegistry::new();
    reg.register(ProbeSpec::new("alpha", ProbeKind::Http));
    reg.register(ProbeSpec::new("beta", ProbeKind::Http));
    let probe = MockProbe::scripted(vec![
        ProbeOutcome::Success,
        ProbeOutcome::OauthError {
            code: "invalid_grant".into(),
            description: String::new(),
        },
    ]);
    let rig = rig("report", reg, probe);
    let result = run(&rig.prober, &Selector::Due, false, 1).unwrap();
    let status = rig.prober.store().lock().unwrap().load_status();
    let value = report_value(&result, Selector::Due.name(), &status, T0);
    assert_eq!(value["mode"], "due");
    assert_eq!(value["counts"]["ok"], 1);
    assert_eq!(value["counts"]["needs_reauth"], 1);
    assert_eq!(value["servers"][0]["server"], "beta");
    // `beta` is an API-token (`ProbeKind::Http`) probe: `auth login` would refuse
    // it (see `login::plan`), so the hint must point at `secret set` instead.
    assert_eq!(
        value["servers"][0]["fix"]["command"],
        "toolportctl secret set beta <KEY> (value on stdin or --value-env <VAR>), then toolportctl auth probe --server beta --force"
    );
    assert_eq!(value["probes"].as_array().unwrap().len(), 2);
    assert_eq!(value["failures"], json!([]));

    let only = report_value(
        &ProbeRun {
            reports: vec![result.reports[0].clone()],
            failures: Vec::new(),
        },
        "server",
        &status,
        T0,
    );
    assert_eq!(only["servers"].as_array().unwrap().len(), 1);
    assert_eq!(only["servers"][0]["server"], "alpha");
    let _ = std::fs::remove_dir_all(&rig.dir);
}

fn servers_of(registry: &ProbeRegistry) -> Vec<String> {
    registry.iter().map(|spec| spec.server.clone()).collect()
}

fn trimmed_registry() -> ProbeRegistry {
    let mut reg = ProbeRegistry::new();
    reg.register(ProbeSpec::new("files", ProbeKind::Stdio));
    reg.register(ProbeSpec::new("gdocs-a", ProbeKind::GoogleRefresh).with_profile("work"));
    reg.register(ProbeSpec::new("gdocs-b", ProbeKind::GoogleRefresh).with_profile("work"));
    reg
}

#[test]
fn removing_a_server_drops_it_from_the_counts_and_from_the_cache() {
    let invalid_grant = ProbeOutcome::OauthError {
        code: "invalid_grant".into(),
        description: String::new(),
    };
    let rig = rig(
        "prune",
        registry(),
        MockProbe::scripted(vec![
            ProbeOutcome::Success,
            ProbeOutcome::Success,
            ProbeOutcome::Success,
            invalid_grant,
        ]),
    );
    run(&rig.prober, &Selector::Due, false, 1).unwrap();
    let load = || rig.prober.store().lock().unwrap().load_status();
    let before = load();
    assert_eq!(
        before.servers.keys().collect::<Vec<_>>(),
        ["files", "gdocs-a", "gdocs-c", "slack"]
    );
    assert_eq!(
        before.profiles.keys().collect::<Vec<_>>(),
        ["google:home", "google:work"]
    );
    let line = surfaces::statusline(&before, T0, &servers_of(&registry()));
    assert_eq!(line["auth"]["needs_reauth"], 1);
    assert_eq!(line["auth"]["worst"], json!(["slack"]));

    let kept = trimmed_registry();
    let after_removal = AuthProber::new(
        AuthStore::new(&rig.dir),
        kept.clone(),
        rig.probe.clone(),
        rig.clock.clone(),
    );
    let stale = load();
    assert!(stale.servers.contains_key("slack"), "nothing pruned it yet");
    for line in [
        surfaces::statusline(&stale, T0, &servers_of(&kept)),
        surfaces::hook(&stale, T0, &servers_of(&kept)),
    ] {
        assert_eq!(line["auth"]["needs_reauth"], 0);
        assert_eq!(line["auth"]["text"], "auth ok (2)");
    }
    assert!(surfaces::hook(&stale, T0, &servers_of(&kept))
        .get("hookSpecificOutput")
        .is_none());

    let calls = rig.probe.call_count();
    let scan = run(&after_removal, &Selector::Due, false, 1).unwrap();
    assert!(scan.reports.is_empty() && scan.failures.is_empty(), "{scan:?}");
    assert_eq!(rig.probe.call_count(), calls, "pruning probes nothing");

    let after = load();
    assert!(after.servers.get("slack").is_none());
    assert!(after.servers.get("gdocs-c").is_none());
    assert_eq!(after.servers.keys().collect::<Vec<_>>(), ["files", "gdocs-a"]);
    assert_eq!(after.profiles.keys().collect::<Vec<_>>(), ["google:work"]);
    assert_eq!(after.servers["files"], before.servers["files"]);
    assert_eq!(after.servers["gdocs-a"], before.servers["gdocs-a"]);
    assert_eq!(after.profiles["google:work"], before.profiles["google:work"]);

    let on_disk = surfaces::read_status(&rig.dir);
    assert_eq!(on_disk, after);
    let unfiltered = surfaces::rows(&on_disk, T0);
    assert!(unfiltered.iter().all(|row| row.server != "slack"));
    assert_eq!(
        surfaces::statusline(&on_disk, T0, &servers_of(&kept)),
        surfaces::statusline(&on_disk, T0, &on_disk.servers.keys().cloned().collect::<Vec<_>>())
    );
    let _ = std::fs::remove_dir_all(&rig.dir);
}

#[test]
fn pruning_touches_the_cache_only_when_something_is_stale() {
    let rig = rig(
        "prune-quiet",
        registry(),
        MockProbe::always(ProbeOutcome::Success),
    );
    assert!(!rig.prober.prune_unregistered().unwrap());
    assert!(!rig.dir.exists(), "no cache, nothing created");

    run(&rig.prober, &Selector::Due, false, 1).unwrap();
    let path = rig.prober.store().status_path();
    let bytes = std::fs::read(&path).unwrap();
    assert!(!rig.prober.prune_unregistered().unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    let nothing = AuthProber::new(
        AuthStore::new(&rig.dir),
        ProbeRegistry::new(),
        rig.probe.clone(),
        rig.clock.clone(),
    );
    assert!(nothing.prune_unregistered().unwrap());
    let emptied = surfaces::read_status(&rig.dir);
    assert!(emptied.servers.is_empty() && emptied.profiles.is_empty());
    assert!(!nothing.prune_unregistered().unwrap());
    let _ = std::fs::remove_dir_all(&rig.dir);
}
