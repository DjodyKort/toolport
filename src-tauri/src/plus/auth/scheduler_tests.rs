use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use super::scheduler::{
    disabled, tick, Scheduler, Tick, DISABLE_ENV, STARTUP_DELAY_SECS, TICK_SECS,
};
use super::*;

const T0: i64 = 4_000_000;
const EVERY: i64 = 600;

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "auth-sched-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn registry() -> ProbeRegistry {
    let mut reg = ProbeRegistry::new();
    reg.register(ProbeSpec::new("slack", ProbeKind::Http).with_min_interval(EVERY));
    reg.register(ProbeSpec::new("figma", ProbeKind::Http).with_min_interval(EVERY));
    reg
}

struct Rig {
    dir: PathBuf,
    clock: Arc<FakeClock>,
    probe: Arc<MockProbe>,
    builds: Arc<AtomicUsize>,
    scheduler: Arc<Scheduler>,
}

fn rig(tag: &str, probe: MockProbe) -> Rig {
    let dir = scratch(tag);
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(probe);
    let builds = Arc::new(AtomicUsize::new(0));
    let factory = {
        let (dir, clock, probe, builds) =
            (dir.clone(), clock.clone(), probe.clone(), builds.clone());
        Box::new(move || {
            builds.fetch_add(1, Ordering::SeqCst);
            Ok(AuthProber::new(
                AuthStore::new(&dir),
                registry(),
                probe.clone(),
                clock.clone(),
            ))
        })
    };
    let scheduler = Scheduler::new(clock.clone(), factory);
    Rig {
        dir,
        clock,
        probe,
        builds,
        scheduler,
    }
}

fn settle(scheduler: &Scheduler) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while scheduler.running() {
        assert!(Instant::now() < deadline, "the scan never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_scan_waits_out_the_startup_delay_then_paces_itself() {
    let rig = rig("pace", MockProbe::always(ProbeOutcome::Success));
    let s = &rig.scheduler;
    assert_eq!(s.begin(), Tick::Waiting);
    rig.clock.advance(STARTUP_DELAY_SECS - 1);
    assert_eq!(s.begin(), Tick::Waiting);
    rig.clock.advance(1);
    assert_eq!(s.begin(), Tick::Started);
    assert!(s.running());
    rig.clock.advance(TICK_SECS * 10);
    assert_eq!(s.begin(), Tick::Busy, "no second scan while one is running");
    assert_eq!(rig.builds.load(Ordering::SeqCst), 0);
}

#[test]
fn a_tick_runs_the_due_probes_on_its_own_thread_and_respects_the_cache() {
    let rig = rig("run", MockProbe::always(ProbeOutcome::Success));
    let s = &rig.scheduler;
    assert_eq!(s.tick(), Tick::Waiting);
    assert_eq!(rig.probe.call_count(), 0);

    rig.clock.advance(STARTUP_DELAY_SECS);
    assert_eq!(s.tick(), Tick::Started);
    settle(s);
    let mut called = rig.probe.called_servers();
    called.sort();
    assert_eq!(called, ["figma", "slack"]);

    rig.clock.advance(TICK_SECS);
    assert_eq!(s.tick(), Tick::Started);
    settle(s);
    assert_eq!(rig.probe.call_count(), 2, "nothing is due a minute later");
    assert_eq!(rig.builds.load(Ordering::SeqCst), 2);

    rig.clock.advance(EVERY);
    assert_eq!(s.tick(), Tick::Started);
    settle(s);
    assert_eq!(
        rig.probe.call_count(),
        4,
        "both are due again after the cadence"
    );
    let _ = std::fs::remove_dir_all(&rig.dir);
}

#[test]
fn a_slow_probe_never_blocks_the_caller() {
    let (release, gate) = mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let probe = MockProbe::always(ProbeOutcome::Success).on_run(move || {
        let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(10));
    });
    let rig = rig("slow", probe);
    let s = &rig.scheduler;
    rig.clock.advance(STARTUP_DELAY_SECS);
    assert_eq!(s.tick(), Tick::Started);
    assert!(s.running(), "tick returned only after the probe finished");
    rig.clock.advance(TICK_SECS * 5);
    assert_eq!(s.tick(), Tick::Busy);
    release.send(()).unwrap();
    release.send(()).unwrap();
    settle(s);
    let _ = std::fs::remove_dir_all(&rig.dir);
}

#[test]
fn a_failing_or_panicking_scan_is_survived_and_retried_later() {
    let clock = Arc::new(FakeClock::new(T0));
    let attempts = Arc::new(AtomicUsize::new(0));
    let factory = {
        let attempts = attempts.clone();
        Box::new(move || match attempts.fetch_add(1, Ordering::SeqCst) {
            0 => Err("data directory unavailable".to_string()),
            1 => panic!("registry exploded"),
            _ => Ok(AuthProber::new(
                AuthStore::new(&scratch("survive")),
                ProbeRegistry::new(),
                Arc::new(MockProbe::always(ProbeOutcome::Success)),
                Arc::new(FakeClock::new(T0)),
            )),
        })
    };
    let s = Scheduler::new(clock.clone(), factory);
    for expected in 1..=3 {
        clock.advance(STARTUP_DELAY_SECS.max(TICK_SECS));
        assert_eq!(s.tick(), Tick::Started);
        settle(&s);
        assert_eq!(attempts.load(Ordering::SeqCst), expected);
    }
}

#[test]
fn scan_once_reports_the_run_without_pacing() {
    let rig = rig("once", MockProbe::always(ProbeOutcome::Success));
    let first = rig.scheduler.scan_once().unwrap();
    assert_eq!(first.probed(), 2);
    let second = rig.scheduler.scan_once().unwrap();
    assert!(second.reports.is_empty());
    let _ = std::fs::remove_dir_all(&rig.dir);
}

#[test]
fn the_env_opt_out_disables_the_global_scan() {
    let _env = crate::clients::env_test_lock();
    let before = std::env::var_os(DISABLE_ENV);
    let cases = [
        ("0", true),
        ("off", true),
        ("false", true),
        (" off ", true),
        ("1", false),
        ("", false),
    ];
    for (value, expected) in cases {
        std::env::set_var(DISABLE_ENV, value);
        assert_eq!(disabled(), expected, "{value:?}");
    }
    std::env::set_var(DISABLE_ENV, "off");
    assert_eq!(tick(), Tick::Waiting);
    match before {
        Some(value) => std::env::set_var(DISABLE_ENV, value),
        None => std::env::remove_var(DISABLE_ENV),
    }
    assert!(!disabled());
}

#[test]
fn the_gateway_watch_loop_hands_the_scan_over() {
    let source = include_str!("../../bin/toolport-gateway.rs");
    let start = source.find("\nfn watch_registry(").expect("watch_registry");
    let rest = &source[start + 1..];
    let end = rest.find("\nfn ").map_or(rest.len(), |at| at + 1);
    assert!(
        rest[..end].contains("conduit_lib::plus::auth::scheduler::tick();"),
        "watch_registry no longer drives the auth scan"
    );
}
