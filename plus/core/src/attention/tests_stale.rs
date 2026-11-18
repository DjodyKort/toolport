use toolport_plus_api::MemoryHost;

use super::store::{AccessLedger, HideKind};
use super::tests::{Dir, NOW};
use super::tests_access::{host, server, set};
use super::{observe_access, ops, snapshot, Item, Urgency};

const DAY: i64 = 24 * 3600;

fn stale_rows(host: &MemoryHost) -> Vec<Item> {
    snapshot(host)
        .items
        .into_iter()
        .filter(|item| item.id.starts_with("access:stale:"))
        .collect()
}

fn watched(dir: &Dir, servers: &[&str], members: &[&str]) -> MemoryHost {
    let host = host(dir, servers, vec![set("work", members)]);
    observe_access(&host).unwrap();
    host
}

fn later(host: &MemoryHost, days: i64) {
    host.now.set(NOW + days * DAY);
}

#[test]
fn test90_a_server_in_use_with_no_success_for_thirty_days_is_one_calm_skippable_row() {
    let dir = Dir::new("stale-row");
    let host = watched(&dir, &["a", "b"], &["a", "b"]);
    later(&host, 29);
    assert!(stale_rows(&host).is_empty());
    later(&host, 30);
    let rows = stale_rows(&host);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, "access:stale:a,b");
    assert_eq!(row.urgency, Urgency::WorthALook);
    assert_eq!(row.source, "Access");
    assert_eq!(row.title, "2 servers have not worked for 30 days");
    assert!(row
        .detail
        .starts_with("A, B are on and an access set reaches them"));
    assert_eq!(row.skip.as_deref(), Some("these servers"));
    assert_eq!(row.since, None);
}

#[test]
fn test91_a_success_in_the_audit_log_or_the_ledger_keeps_a_server_fresh() {
    let dir = Dir::new("stale-fresh");
    let host = watched(&dir, &["a", "b", "c"], &["a", "b", "c"]);
    later(&host, 40);
    host.activity
        .borrow_mut()
        .insert("a".into(), NOW + 20 * DAY);
    host.activity.borrow_mut().insert("b".into(), NOW + 5 * DAY);
    let rows = stale_rows(&host);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].id, "access:stale:b,c");
    host.activity
        .borrow_mut()
        .insert("b".into(), NOW + 39 * DAY);
    assert_eq!(stale_rows(&host)[0].id, "access:stale:c");
}

#[test]
fn test92_the_notifier_pass_remembers_a_success_after_the_audit_log_trimmed_it() {
    let dir = Dir::new("stale-ledger");
    let host = watched(&dir, &["a"], &["a"]);
    later(&host, 20);
    host.activity
        .borrow_mut()
        .insert("a".into(), NOW + 20 * DAY);
    observe_access(&host).unwrap();
    host.activity.borrow_mut().clear();
    later(&host, 45);
    assert!(stale_rows(&host).is_empty(), "last success was 25 days ago");
    later(&host, 51);
    assert_eq!(stale_rows(&host).len(), 1);
}

#[test]
fn test93_only_servers_that_are_on_and_reached_can_be_stale() {
    let dir = Dir::new("stale-scope");
    let host = watched(&dir, &["a", "b"], &["a"]);
    host.registry
        .servers
        .borrow_mut()
        .push(server("off", false));
    later(&host, 60);
    let rows = stale_rows(&host);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "access:stale:a");
    assert_eq!(rows[0].title, "A has not worked for 30 days");
    let unreached = snapshot(&host)
        .items
        .into_iter()
        .any(|item| item.id == "access:unreached:b");
    assert!(unreached, "b is the other calm row, not a stale one");
}

#[test]
fn test94_a_server_added_later_is_judged_from_when_it_appeared() {
    let dir = Dir::new("stale-added");
    let host = watched(&dir, &["a"], &["a", "n"]);
    later(&host, 50);
    host.registry.servers.borrow_mut().push(server("n", true));
    observe_access(&host).unwrap();
    later(&host, 60);
    let ids: Vec<String> = stale_rows(&host).into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["access:stale:a"]);
    later(&host, 81);
    assert_eq!(stale_rows(&host)[0].id, "access:stale:a,n");
}

#[test]
fn test95_nothing_is_stale_before_the_first_observation() {
    let dir = Dir::new("stale-unobserved");
    let host = host(&dir, &["a"], vec![set("work", &["a"])]);
    later(&host, 400);
    assert!(stale_rows(&host).is_empty());
}

#[test]
fn test96_the_number_of_days_is_a_setting_with_a_range() {
    let dir = Dir::new("stale-days");
    let host = watched(&dir, &["a"], &["a"]);
    later(&host, 8);
    assert!(stale_rows(&host).is_empty());
    let settings = ops::set_stale_days(&host, 7).unwrap();
    assert_eq!(settings["staleDays"], 7);
    let rows = stale_rows(&host);
    assert_eq!(rows[0].title, "A has not worked for 7 days");
    for days in [0, 366] {
        let error = ops::set_stale_days(&host, days).unwrap_err();
        assert!(error.message.contains("from 1 to 365"), "{}", error.message);
    }
    assert_eq!(ops::settings(&host).unwrap()["staleDays"], 7);
}

#[test]
fn test97_a_skip_holds_until_another_server_goes_stale() {
    let dir = Dir::new("stale-skip");
    let host = watched(&dir, &["a", "b"], &["a", "b"]);
    host.activity
        .borrow_mut()
        .insert("b".into(), NOW + 29 * DAY);
    later(&host, 31);
    let id = stale_rows(&host)[0].id.clone();
    assert_eq!(id, "access:stale:a");
    ops::hide(&host, &id, HideKind::Skip, None, false).unwrap();
    assert!(stale_rows(&host).is_empty());
    later(&host, 60);
    assert_eq!(stale_rows(&host)[0].id, "access:stale:a,b");
}

#[test]
fn test98_an_unreadable_audit_log_is_named_in_the_access_check_not_read_as_all_clear() {
    let dir = Dir::new("stale-unreadable");
    let host = watched(&dir, &["a"], &["a"]);
    later(&host, 60);
    *host.activity_error.borrow_mut() = Some("permission denied".into());
    let snapshot = snapshot(&host);
    let check = snapshot.checks.iter().find(|c| c.name == "access").unwrap();
    assert_eq!(
        check.error.as_deref(),
        Some("the audit log cannot be read: permission denied")
    );
    assert!(snapshot.items.iter().all(|i| !i.id.starts_with("access:")));
}

#[test]
fn test99_the_ledger_counts_idle_time_from_the_latest_known_success_or_the_first_sighting() {
    let mut ledger = AccessLedger::default();
    assert_eq!(ledger.idle_since("a", None, 500), None);
    ledger.observe(&["a".into()], Default::default(), 100);
    assert_eq!(ledger.idle_since("a", None, 500), Some(100));
    assert_eq!(ledger.idle_since("a", Some(300), 500), Some(300));
    assert_eq!(ledger.idle_since("a", Some(50), 500), Some(100));
    assert_eq!(ledger.idle_since("new", None, 500), Some(500));
    assert!(ledger.record_calls(&[("a".to_string(), 400)].into()));
    assert!(!ledger.record_calls(&[("a".to_string(), 350)].into()));
    assert_eq!(ledger.idle_since("a", None, 500), Some(400));
    ledger.observe(&[], Default::default(), 600);
    assert!(ledger.last_ok.is_empty());
}
