use serde_json::json;
use toolport_plus_api::{
    convert, AccessSetInfo, MemoryClients, MemoryHost, MemoryRegistry, ServerRecord,
};

use super::feed_access::{self, AccessInputs, UnreachedServer};
use super::store::{AccessLedger, Store};
use super::tests::{Dir, NOW};
use super::{compute, observe_access, read, snapshot, Item, Screen, Urgency};

const LATER: i64 = NOW + 3600;

fn server(id: &str, enabled: bool) -> ServerRecord {
    convert(
        &json!({"id": id, "name": id.to_uppercase(), "enabled": enabled,
        "transport": "stdio", "command": "run-it"}),
    )
    .unwrap()
}

fn set(id: &str, members: &[&str]) -> AccessSetInfo {
    AccessSetInfo {
        id: id.into(),
        name: id.into(),
        server_ids: members.iter().map(|m| m.to_string()).collect(),
    }
}

fn host(dir: &Dir, servers: &[&str], sets: Vec<AccessSetInfo>) -> MemoryHost {
    let host = MemoryHost {
        registry: MemoryRegistry::with_servers(servers.iter().map(|id| server(id, true)).collect()),
        clients: MemoryClients::with_client("claude-code", "Claude Code"),
        secrets: toolport_plus_api::MemorySecrets {
            data_dir: Some(dir.0.clone()),
            ..Default::default()
        },
        ..Default::default()
    };
    *host.registry.access_sets.borrow_mut() = sets;
    *host.registry.default_access.borrow_mut() = Some("work".into());
    host.now.set(NOW);
    host
}

fn rows(host: &MemoryHost) -> Vec<Item> {
    snapshot(host)
        .items
        .into_iter()
        .filter(|item| item.source == "Access")
        .collect()
}

fn add_server(host: &MemoryHost, id: &str) {
    host.registry.servers.borrow_mut().push(server(id, true));
}

#[test]
fn test70_servers_no_set_reaches_are_one_calm_row_that_names_them() {
    let dir = Dir::new("access-calm");
    let host = host(&dir, &["a", "b", "c"], vec![set("work", &["a"])]);
    let rows = rows(&host);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, "access:unreached:b,c");
    assert_eq!(row.urgency, Urgency::WorthALook);
    assert_eq!(row.title, "2 servers no access set reaches");
    assert!(row.detail.starts_with("B, C are on"));
    assert!(row.skippable());
    let action = row.action.as_ref().unwrap();
    assert_eq!(
        (action.label.as_str(), action.screen),
        ("Review", Screen::Servers)
    );
}

#[test]
fn test71_reached_servers_and_servers_that_are_off_raise_nothing() {
    let dir = Dir::new("access-quiet");
    let host = host(&dir, &["a", "b"], vec![set("work", &["a", "b"])]);
    host.registry
        .servers
        .borrow_mut()
        .push(server("off", false));
    assert!(rows(&host).is_empty());
}

#[test]
fn test72_a_server_added_after_the_sets_last_changed_needs_you_until_a_set_changes() {
    let dir = Dir::new("access-added");
    let host = host(&dir, &["a", "b"], vec![set("work", &["a"])]);
    observe_access(&host).unwrap();
    host.now.set(LATER);
    add_server(&host, "c");
    observe_access(&host).unwrap();
    let rows = rows(&host);
    let new = rows.iter().find(|row| row.id == "access:new:c").unwrap();
    assert_eq!(new.urgency, Urgency::NeedsYou);
    assert_eq!(new.title, "C was added, and no access set reaches it");
    assert_eq!(new.since, Some(LATER));
    assert!(!new.skippable());
    assert!(rows.iter().any(|row| row.id == "access:unreached:b"));

    host.now.set(LATER + 60);
    *host.registry.access_sets.borrow_mut() = vec![set("work", &["a", "b"])];
    let rows_now = rows_after_edit(&host);
    assert!(
        rows_now
            .iter()
            .all(|row| row.urgency == Urgency::WorthALook),
        "an edited set is a decision, even before the next observation: {rows_now:?}"
    );
    assert!(rows_now.iter().any(|row| row.id == "access:unreached:c"));
}

fn rows_after_edit(host: &MemoryHost) -> Vec<Item> {
    let before = rows(host);
    observe_access(host).unwrap();
    let after = rows(host);
    assert_eq!(before, after, "observing changes nothing the list showed");
    after
}

#[test]
fn test73_nothing_counts_as_added_before_the_registry_was_first_observed() {
    let dir = Dir::new("access-first");
    let host = host(&dir, &["a", "b"], vec![set("work", &["a"])]);
    assert_eq!(rows(&host)[0].urgency, Urgency::WorthALook);
    observe_access(&host).unwrap();
    let store = Store::read(&dir.0).unwrap();
    assert_eq!(store.access.observed_at, Some(NOW));
    assert_eq!(store.access.servers["b"], 0);
    assert_eq!(rows(&host)[0].urgency, Urgency::WorthALook);
}

#[test]
fn test74_listing_never_writes_and_observing_writes_only_when_something_changed() {
    let dir = Dir::new("access-writes");
    let host = host(&dir, &["a", "b"], vec![set("work", &["a"])]);
    let _ = snapshot(&host);
    assert!(!super::store::path(&dir.0).exists());
    observe_access(&host).unwrap();
    let file = super::store::path(&dir.0);
    let first = std::fs::read_to_string(&file).unwrap();
    host.now.set(LATER);
    observe_access(&host).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), first);
}

#[test]
fn test75_a_mapped_folder_that_outranks_a_clients_own_set_is_a_calm_skippable_row() {
    let dir = Dir::new("access-folder");
    let host = host(
        &dir,
        &["a", "b", "c"],
        vec![
            set("work", &["a", "b", "c"]),
            set("bare", &["b", "c"]),
            set("proj", &["b", "a"]),
        ],
    );
    host.registry
        .client_scopes
        .borrow_mut()
        .insert("claude-code".into(), "bare".into());
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/home/test/proj".into(), "proj".into()));
    let rows = rows(&host);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "access:folder:/home/test/proj:claude-code");
    assert_eq!(rows[0].urgency, Urgency::WorthALook);
    assert!(rows[0].skippable());
    assert!(rows[0].detail.contains("hides 1 server its own set lists"));
}

#[test]
fn test76_added_servers_and_old_ones_are_separate_rows_with_their_own_ids() {
    let inputs = AccessInputs {
        unreached: vec![
            UnreachedServer {
                id: "x".into(),
                name: "X".into(),
                added: None,
            },
            UnreachedServer {
                id: "y".into(),
                name: "Y".into(),
                added: Some(NOW),
            },
            UnreachedServer {
                id: "z".into(),
                name: "Z".into(),
                added: Some(NOW - 60),
            },
        ],
        folders: Vec::new(),
    };
    let items = feed_access::items(&inputs);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "access:new:y,z");
    assert_eq!(
        items[0].title,
        "2 servers were added, and no access set reaches them"
    );
    assert_eq!(items[0].since, Some(NOW - 60));
    assert_eq!(items[1].id, "access:unreached:x");
}

#[test]
fn test77_the_access_check_is_listed_and_can_be_switched_off() {
    let dir = Dir::new("access-off");
    let host = host(&dir, &["a", "b"], vec![set("work", &["a"])]);
    let json = snapshot(&host).to_json();
    assert_eq!(json["checks"][2], json!({"name": "access", "ok": true}));
    assert_eq!(json["counts"]["worthALook"], 1);

    let mut store = Store::default();
    store.settings.checks.insert("access".into(), false);
    store.write(&dir.0).unwrap();
    let off = snapshot(&host);
    assert!(off.items.iter().all(|item| item.source != "Access"));
    assert_eq!(
        off.to_json()["checks"][2],
        json!({"name": "access", "ok": true, "off": true})
    );
}

#[test]
fn test78_a_registry_that_cannot_be_read_is_named_not_shown_as_all_clear() {
    let dir = Dir::new("access-error");
    let host = host(&dir, &["a"], vec![set("work", &["a"])]);
    let mut inputs = read(&host);
    inputs.access = Err("the registry cannot be read".into());
    let snapshot = compute(&inputs);
    let check = snapshot.checks.iter().find(|c| c.name == "access").unwrap();
    assert_eq!(check.error.as_deref(), Some("the registry cannot be read"));
}

#[test]
fn test79_the_ledger_tells_added_from_old_by_the_last_change_to_any_set() {
    let sets = |members: &str| {
        [(
            "work".to_string(),
            AccessLedger::fingerprint("work", &[members.to_string()]),
        )]
        .into_iter()
        .collect()
    };
    let mut ledger = AccessLedger::default();
    assert_eq!(ledger.added_after_sets("a", &sets("a"), NOW), None);
    assert!(ledger.observe(&["a".into()], sets("a"), NOW));
    assert!(!ledger.observe(&["a".into()], sets("a"), NOW + 5));
    assert_eq!(ledger.added_after_sets("a", &sets("a"), NOW + 9), None);
    assert_eq!(
        ledger.added_after_sets("b", &sets("a"), NOW + 9),
        Some(NOW + 9)
    );
    assert!(ledger.observe(&["a".into(), "b".into()], sets("a"), NOW + 10));
    assert_eq!(
        ledger.added_after_sets("b", &sets("a"), NOW + 20),
        Some(NOW + 10)
    );
    assert_eq!(ledger.added_after_sets("b", &sets("a,b"), NOW + 20), None);
    assert_eq!(
        AccessLedger::fingerprint("w", &["b".into(), "a".into()]),
        AccessLedger::fingerprint("w", &["a".into(), "b".into()])
    );
}
