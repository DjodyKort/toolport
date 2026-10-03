use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::json;

use super::*;

const T0: i64 = 2_000_000;
const HOUR: i64 = 3600;
const SECRET: &str = "TOKEN-SECRET-123";

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "auth-probe-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn registry() -> ProbeRegistry {
    let mut reg = ProbeRegistry::new();
    reg.register(ProbeSpec::new("slack", ProbeKind::Http));
    reg.register(ProbeSpec::new("gdocs-a", ProbeKind::GoogleRefresh).with_profile("work"));
    reg.register(ProbeSpec::new("gdocs-b", ProbeKind::GoogleRefresh).with_profile("work"));
    reg.register(ProbeSpec::new("gdocs-c", ProbeKind::GoogleRefresh).with_profile("home"));
    reg
}

fn prober(dir: &std::path::Path, probe: Arc<MockProbe>, clock: Arc<FakeClock>) -> AuthProber {
    AuthProber::new(AuthStore::new(dir), registry(), probe, clock)
}

fn oauth(code: &str, description: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.into(),
        description: description.into(),
    }
}

#[test]
fn default_intervals_by_kind() {
    assert_eq!(
        ProbeSpec::new("g", ProbeKind::GoogleRefresh).min_interval_secs,
        6 * HOUR
    );
    assert!(ProbeSpec::new("s", ProbeKind::Http).min_interval_secs < 6 * HOUR);
    let spec = ProbeSpec::new("s", ProbeKind::Http)
        .with_param("url", "https://example.invalid")
        .with_min_interval(30);
    assert_eq!(spec.min_interval_secs, 30);
    assert_eq!(spec.params["url"], "https://example.invalid");
    assert_eq!(registry().get("slack").unwrap().kind, ProbeKind::Http);
    assert!(registry().get("nope").is_none());
}

#[test]
fn single_flight_runs_probe_once_for_concurrent_requests() {
    const N: usize = 8;
    let dir = scratch("flight");
    let clock = Arc::new(FakeClock::new(T0));
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let rx = std::sync::Mutex::new(rx);
    let probe = Arc::new(MockProbe::always(ProbeOutcome::Success).on_run(move || {
        rx.lock().unwrap().recv().unwrap();
    }));
    let prober = Arc::new(prober(&dir, probe.clone(), clock));
    let handles: Vec<_> = (0..N)
        .map(|_| {
            let p = prober.clone();
            std::thread::spawn(move || p.request("slack", Trigger::UserForce).unwrap())
        })
        .collect();
    while prober.waiters("slack") < N - 1 {
        std::thread::yield_now();
    }
    tx.send(()).unwrap();
    let reports: Vec<ProbeReport> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(probe.call_count(), 1);
    assert!(reports
        .iter()
        .all(|r| r.ran && r.tracked.state == AuthState::Ok));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn single_flight_leader_panic_lets_followers_retry() {
    let flight: Arc<SingleFlight<u32>> = Arc::new(SingleFlight::new());
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
    let leader = {
        let flight = flight.clone();
        std::thread::spawn(move || {
            flight.run("k", || {
                started_tx.send(()).unwrap();
                go_rx.recv().unwrap();
                panic!("boom")
            })
        })
    };
    started_rx.recv().unwrap();
    let follower = {
        let flight = flight.clone();
        std::thread::spawn(move || flight.run("k", || 7))
    };
    while flight.waiters("k") < 1 {
        std::thread::yield_now();
    }
    go_tx.send(()).unwrap();
    assert_eq!(follower.join().unwrap(), 7);
    assert!(leader.join().is_err());
}

#[test]
fn min_interval_is_enforced_with_fake_clock() {
    let dir = scratch("interval");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::always(ProbeOutcome::Success));
    let p = prober(&dir, probe.clone(), clock.clone());
    assert!(p.request("slack", Trigger::Scheduled).unwrap().ran);
    clock.advance(HTTP_MIN - 1);
    let skipped = p.request("slack", Trigger::Scheduled).unwrap();
    assert!(!skipped.ran);
    assert_eq!(skipped.skipped, Some("not_due"));
    assert_eq!(probe.call_count(), 1);
    clock.advance(1);
    assert!(p.request("slack", Trigger::Scheduled).unwrap().ran);
    assert_eq!(probe.call_count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

const HTTP_MIN: i64 = 10 * 60;

#[test]
fn forced_refresh_bypasses_interval_but_scheduled_cannot() {
    let dir = scratch("force");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::always(ProbeOutcome::Success));
    let p = prober(&dir, probe.clone(), clock.clone());
    assert!(p.request("gdocs-a", Trigger::Scheduled).unwrap().ran);
    for _ in 0..5 {
        clock.advance(60);
        assert!(!p.request("gdocs-a", Trigger::Scheduled).unwrap().ran);
    }
    assert_eq!(probe.call_count(), 1);
    assert!(p.request("gdocs-a", Trigger::UserForce).unwrap().ran);
    assert_eq!(probe.call_count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn google_probes_once_per_profile_per_six_hours() {
    let dir = scratch("profile");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::always(ProbeOutcome::Success));
    let p = prober(&dir, probe.clone(), clock.clone());
    assert!(p.request("gdocs-a", Trigger::Scheduled).unwrap().ran);
    let sibling = p.request("gdocs-b", Trigger::Scheduled).unwrap();
    assert_eq!(sibling.skipped, Some("profile_interval"));
    assert!(p.request("gdocs-c", Trigger::Scheduled).unwrap().ran);
    clock.advance(6 * HOUR - 1);
    assert!(!p.request("gdocs-b", Trigger::Scheduled).unwrap().ran);
    clock.advance(1);
    assert!(p.request("gdocs-b", Trigger::Scheduled).unwrap().ran);
    assert_eq!(
        probe.called_servers(),
        vec!["gdocs-a", "gdocs-c", "gdocs-b"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn probe_due_honors_intervals_profiles_and_backoff() {
    let dir = scratch("due");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::always(ProbeOutcome::TransportError));
    let p = prober(&dir, probe.clone(), clock.clone());
    assert_eq!(p.due().unwrap(), vec!["gdocs-a", "gdocs-c", "slack"]);
    p.request("slack", Trigger::Scheduled).unwrap();
    p.request("gdocs-a", Trigger::Scheduled).unwrap();
    assert_eq!(p.due().unwrap(), vec!["gdocs-c"]);
    clock.advance(HTTP_MIN);
    assert!(p.due().unwrap().contains(&"slack".to_string()));
    p.request("slack", Trigger::Scheduled).unwrap();
    clock.advance(HTTP_MIN);
    assert!(!p.due().unwrap().contains(&"slack".to_string()));
    clock.advance(HTTP_MIN);
    assert!(p.due().unwrap().contains(&"slack".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn backoff_grows_and_is_capped() {
    assert_eq!(backoff_delay(600, 1), 600);
    assert_eq!(backoff_delay(600, 2), 1200);
    assert_eq!(backoff_delay(600, 3), 2400);
    assert_eq!(backoff_delay(600, 40), 6 * HOUR);
    assert_eq!(backoff_delay(6 * HOUR, 5), 6 * HOUR);
    assert_eq!(backoff_delay(12 * HOUR, 5), 12 * HOUR);
}

#[test]
fn transient_failures_reach_unreachable_through_backoff_and_recover() {
    let dir = scratch("unreach");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::scripted(vec![
        ProbeOutcome::TransportError,
        ProbeOutcome::TransportError,
        ProbeOutcome::TransportError,
        ProbeOutcome::Success,
    ]));
    let p = prober(&dir, probe, clock.clone());
    p.request("slack", Trigger::Scheduled).unwrap();
    loop {
        clock.advance(60);
        let due = p.due().unwrap();
        if due.contains(&"slack".to_string()) {
            break;
        }
    }
    p.request("slack", Trigger::Scheduled).unwrap();
    clock.advance(2 * HTTP_MIN);
    let third = p.request("slack", Trigger::Scheduled).unwrap();
    assert_eq!(third.tracked.state, AuthState::Unreachable);
    clock.advance(10 * HTTP_MIN);
    let fourth = p.request("slack", Trigger::Scheduled).unwrap();
    assert_eq!(fourth.tracked.state, AuthState::Ok);
    let events = AuthStore::new(&dir).lock().unwrap().read_events();
    let edges: Vec<_> = events
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    assert_eq!(
        edges,
        vec![("unknown", "unreachable"), ("unreachable", "ok")]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_persists_across_reopen() {
    let dir = scratch("persist");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::always(oauth("invalid_grant", "expired")));
    {
        let p = prober(&dir, probe, clock.clone());
        p.request("slack", Trigger::Scheduled).unwrap();
    }
    let status = AuthStore::new(&dir).lock().unwrap().load_status();
    let entry = &status.servers["slack"];
    assert_eq!(entry.tracked.state, AuthState::NeedsReauth);
    assert_eq!(entry.last_probe_at, Some(T0));
    assert_eq!(entry.next_due_at, T0 + HTTP_MIN);
    let again = prober(
        &dir,
        Arc::new(MockProbe::always(ProbeOutcome::Success)),
        clock,
    );
    let report = again.request("slack", Trigger::Scheduled).unwrap();
    assert!(!report.ran);
    assert_eq!(report.tracked.state, AuthState::NeedsReauth);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expiring_state_roundtrips_through_cache() {
    let dir = scratch("expiring");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::always(ProbeOutcome::TokenTtl { secs: 600 }));
    prober(&dir, probe, clock)
        .request("slack", Trigger::Scheduled)
        .unwrap();
    let status = AuthStore::new(&dir).lock().unwrap().load_status();
    assert_eq!(
        status.servers["slack"].tracked.state,
        AuthState::Expiring { eta: T0 + 600 }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn cache_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("perm");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::always(ProbeOutcome::Success));
    prober(&dir, probe, clock)
        .request("slack", Trigger::Scheduled)
        .unwrap();
    for name in ["status.json", "events.jsonl", "auth.lock"] {
        let mode = std::fs::metadata(dir.join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_status_is_quarantined_and_events_survive() {
    let dir = scratch("corrupt");
    let clock = Arc::new(FakeClock::new(T0));
    let p = prober(
        &dir,
        Arc::new(MockProbe::always(ProbeOutcome::Success)),
        clock.clone(),
    );
    p.request("slack", Trigger::Scheduled).unwrap();
    std::fs::write(dir.join("status.json"), "{ not json").unwrap();
    let store = AuthStore::new(&dir);
    {
        let lock = store.lock().unwrap();
        assert!(lock.load_status().servers.is_empty());
        assert_eq!(lock.read_events().len(), 1);
    }
    assert!(store.corrupt_path().exists());
    assert!(!store.status_path().exists());
    clock.advance(1);
    assert!(p.request("slack", Trigger::Scheduled).unwrap().ran);
    let events = store.lock().unwrap().read_events();
    assert_eq!(events.len(), 2);
    std::fs::write(dir.join("status.json"), r#"{"version":99}"#).unwrap();
    assert!(store.lock().unwrap().load_status().servers.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn events_record_state_edges_only() {
    let dir = scratch("edges");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::scripted(vec![
        ProbeOutcome::Success,
        ProbeOutcome::Success,
        ProbeOutcome::HttpStatus { status: 200 },
        oauth("invalid_grant", "expired"),
        oauth("invalid_grant", "expired"),
        ProbeOutcome::Success,
    ]));
    let p = prober(&dir, probe, clock.clone());
    for _ in 0..6 {
        p.request("slack", Trigger::UserForce).unwrap();
        clock.advance(10);
    }
    let events = AuthStore::new(&dir).lock().unwrap().read_events();
    let edges: Vec<_> = events
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str(), e.reason.as_str()))
        .collect();
    assert_eq!(
        edges,
        vec![
            ("unknown", "ok", "ok"),
            ("ok", "needs_reauth", "invalid_grant"),
            ("needs_reauth", "ok", "ok"),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn events_log_is_capped() {
    let dir = scratch("cap");
    let store = AuthStore::new(&dir).with_event_caps(400, 5);
    let lock = store.lock().unwrap();
    for i in 0..50 {
        lock.append_events(&[EdgeEvent {
            ts: i,
            server: "slack".into(),
            from: "ok".into(),
            to: "needs_reauth".into(),
            reason: "invalid_grant".into(),
        }])
        .unwrap();
    }
    let events = lock.read_events();
    assert!(events.len() <= 5 + 3, "{}", events.len());
    assert_eq!(events.last().unwrap().ts, 49);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn secrets_in_probe_errors_never_reach_disk() {
    let dir = scratch("leak");
    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::scripted(vec![
        oauth("invalid_grant", &format!("bad refresh_token={SECRET}")),
        oauth("invalid_client", &format!("client_secret={SECRET} revoked")),
        oauth(&format!("weird_{SECRET}"), SECRET),
    ]));
    let p = prober(&dir, probe, clock);
    for _ in 0..3 {
        p.request("slack", Trigger::UserForce).unwrap();
    }
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!text.contains(SECRET), "{}", path.display());
        checked += 1;
    }
    assert!(checked >= 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_server_is_rejected() {
    let dir = scratch("unknown");
    let p = prober(
        &dir,
        Arc::new(MockProbe::always(ProbeOutcome::Success)),
        Arc::new(FakeClock::new(T0)),
    );
    assert!(p.request("ghost", Trigger::Scheduled).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn status_handler_reports_issues_without_probing() {
    let _guard = crate::registry::data_dir_test_lock();
    let base = scratch("handler");
    let _dir = crate::registry::DataDirOverride::set(base.join("data"));
    let empty = crate::plus::dispatch("plus.auth.status", json!({})).unwrap();
    assert_eq!(empty["issues"], json!([]));
    assert_eq!(empty["servers"], json!({}));

    let clock = Arc::new(FakeClock::new(T0));
    let probe = Arc::new(MockProbe::always(oauth("invalid_grant", "expired")));
    let p = prober(&base.join("data/auth"), probe.clone(), clock);
    p.request("slack", Trigger::Scheduled).unwrap();
    assert_eq!(probe.call_count(), 1);

    let out = crate::plus::dispatch("plus.auth.status", json!({})).unwrap();
    assert_eq!(probe.call_count(), 1);
    assert_eq!(out["issues"][0]["id"], "slack:needs_reauth:invalid_grant");
    assert_eq!(out["issues"][0]["server"], "slack");
    assert_eq!(out["servers"]["slack"]["tracked"]["state"], "needs_reauth");
    assert_eq!(out["servers"]["slack"]["lastProbeAt"], T0);
    assert_eq!(out["servers"]["slack"]["nextDueAt"], T0 + HTTP_MIN);
    let _ = std::fs::remove_dir_all(&base);
}
