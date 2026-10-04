use serde_json::json;

use super::cache::{EdgeEvent, ServerEntry};
use super::notify::{due, notifications_at, Notified};
use super::surfaces::NOTIFY_DEDUPE_SECS;
use super::*;

const NOW: i64 = 2_000_000;

fn status_of(states: &[(&str, AuthState)]) -> StatusFile {
    let mut status = StatusFile::default();
    for (server, state) in states {
        status.servers.insert(
            server.to_string(),
            ServerEntry {
                tracked: Tracked {
                    state: *state,
                    reason: "invalid_grant".into(),
                    since: NOW - 100,
                    transient: None,
                },
                last_probe_at: Some(NOW - 10),
                next_due_at: NOW + 60,
            },
        );
    }
    status
}

fn edge(ts: i64, server: &str, to: &str) -> EdgeEvent {
    EdgeEvent {
        ts,
        server: server.into(),
        from: "ok".into(),
        to: to.into(),
        reason: "invalid_grant".into(),
    }
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("auth-notify-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn an_edge_is_announced_once_and_then_consumed() {
    let status = status_of(&[("beta", AuthState::NeedsReauth)]);
    let events = [edge(NOW - 50, "beta", "needs_reauth")];
    let mut notified = Notified::default();
    let first = due(&events, &status, &mut notified, NOW);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].dedupe_key, "beta:needs_reauth");
    assert_eq!(notified.cursor, NOW - 50);
    assert_eq!(notified.sent["beta:needs_reauth"], NOW);
    assert!(due(&events, &status, &mut notified, NOW + 60).is_empty());
}

#[test]
fn a_flapping_login_is_held_back_inside_the_dedupe_window() {
    let status = status_of(&[("beta", AuthState::NeedsReauth)]);
    let mut notified = Notified::default();
    let first = [edge(NOW - 50, "beta", "needs_reauth")];
    assert_eq!(due(&first, &status, &mut notified, NOW).len(), 1);

    let again = [first[0].clone(), edge(NOW + 600, "beta", "needs_reauth")];
    let soon = NOW + 700;
    assert!(due(&again, &status, &mut notified, soon).is_empty());
    assert_eq!(notified.cursor, NOW + 600);

    let later = [edge(NOW + NOTIFY_DEDUPE_SECS + 10, "beta", "needs_reauth")];
    let after_window = NOW + NOTIFY_DEDUPE_SECS + 20;
    assert_eq!(due(&later, &status, &mut notified, after_window).len(), 1);
}

#[test]
fn a_login_fixed_since_the_edge_is_not_announced() {
    let status = status_of(&[
        ("beta", AuthState::Ok),
        ("gamma", AuthState::Expiring { eta: NOW + 600 }),
        ("delta", AuthState::Revoked),
    ]);
    let events = [
        edge(NOW - 300, "beta", "needs_reauth"),
        edge(NOW - 200, "gamma", "expiring"),
        edge(NOW - 100, "delta", "revoked"),
        edge(NOW - 50, "gone", "needs_reauth"),
    ];
    let mut notified = Notified::default();
    let planned = due(&events, &status, &mut notified, NOW);
    let keys: Vec<&str> = planned.iter().map(|n| n.dedupe_key.as_str()).collect();
    assert_eq!(keys, ["gamma:expiring"]);
    assert_eq!(notified.cursor, NOW - 50);
}

#[test]
fn announced_keys_older_than_the_window_are_forgotten() {
    let status = status_of(&[("beta", AuthState::NeedsReauth)]);
    let mut notified = Notified::default();
    notified
        .sent
        .insert("old:needs_reauth".into(), NOW - NOTIFY_DEDUPE_SECS - 1);
    notified.sent.insert("recent:expiring".into(), NOW - 10);
    due(&[], &status, &mut notified, NOW);
    assert!(!notified.sent.contains_key("old:needs_reauth"));
    assert!(notified.sent.contains_key("recent:expiring"));
}

#[test]
fn the_store_round_trip_persists_what_was_announced() {
    let dir = scratch("store");
    let store = AuthStore::new(&dir);
    {
        let guard = store.lock().unwrap();
        guard
            .save_status(&status_of(&[("beta", AuthState::NeedsReauth)]))
            .unwrap();
        guard
            .append_events(&[edge(NOW - 50, "beta", "needs_reauth")])
            .unwrap();
    }
    let first = notifications_at(&store, NOW).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].title, "beta needs a new login");
    let saved = std::fs::read_to_string(store.notified_path()).unwrap();
    assert!(saved.contains("beta:needs_reauth"), "{saved}");
    assert!(notifications_at(&store, NOW + 60).unwrap().is_empty());

    let reopened = AuthStore::new(&dir);
    assert!(notifications_at(&reopened, NOW + 120).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unchanged_state_is_not_rewritten() {
    let dir = scratch("quiet");
    let store = AuthStore::new(&dir);
    store
        .lock()
        .unwrap()
        .save_status(&status_of(&[("alpha", AuthState::Ok)]))
        .unwrap();
    assert!(notifications_at(&store, NOW).unwrap().is_empty());
    assert!(!store.notified_path().exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_handler_reads_the_data_dir_and_stays_write_free_without_history() {
    let _lock = crate::registry::data_dir_test_lock();
    let dir = scratch("handler");
    std::fs::create_dir_all(&dir).unwrap();
    let _guard = crate::registry::DataDirOverride::set(&dir);

    let empty = crate::plus::dispatch("plus.auth.notifications", json!({})).unwrap();
    assert_eq!(empty, json!({"notifications": []}));
    assert!(!dir.join("auth").exists());

    let store = AuthStore::new(&dir.join("auth"));
    let now = SystemClock.now();
    {
        let guard = store.lock().unwrap();
        guard
            .save_status(&status_of(&[("beta", AuthState::NeedsReauth)]))
            .unwrap();
        guard
            .append_events(&[edge(now - 5, "beta", "needs_reauth")])
            .unwrap();
    }
    let value = crate::plus::dispatch("plus.auth.notifications", json!({})).unwrap();
    let list = value["notifications"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["dedupeKey"], "beta:needs_reauth");
    assert!(!value.to_string().contains("FAKE-"));
    let again = crate::plus::dispatch("plus.auth.notifications", json!({})).unwrap();
    assert_eq!(again, json!({"notifications": []}));
    drop(_guard);
    let _ = std::fs::remove_dir_all(&dir);
}
