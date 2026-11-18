use serde_json::json;
use toolport_plus_api::{
    convert, AccessSetInfo, MemoryClients, MemoryHost, MemoryRegistry, ScopeKind, ServerRecord,
};

use super::*;

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

struct World {
    host: MemoryHost,
}

fn world(default: Option<&str>, sets: Vec<AccessSetInfo>) -> World {
    let host = MemoryHost {
        registry: MemoryRegistry::with_servers(vec![
            server("a", true),
            server("b", true),
            server("c", true),
            server("d", false),
        ]),
        clients: MemoryClients::with_client("claude-code", "Claude Code"),
        ..Default::default()
    };
    *host.registry.access_sets.borrow_mut() = sets;
    *host.registry.default_access.borrow_mut() = default.map(String::from);
    World { host }
}

impl World {
    fn scope(&self, client: &str, reference: &str) {
        self.host
            .registry
            .client_scopes
            .borrow_mut()
            .insert(client.into(), reference.into());
    }

    fn folder(&self, path: &str, reference: &str) {
        self.host
            .registry
            .folder_routes
            .borrow_mut()
            .push((path.into(), reference.into()));
    }

    fn report(&self) -> Report {
        assess(&self.host, Filter::default()).unwrap()
    }
}

fn server_row<'a>(report: &'a Report, id: &str) -> &'a ServerReach {
    report.servers.iter().find(|s| s.id == id).unwrap()
}

#[test]
fn test01_an_explicit_default_access_reaches_only_its_enabled_members() {
    let w = world(Some("work"), vec![set("work", &["a", "d"])]);
    let report = w.report();
    assert!(server_row(&report, "a").default);
    assert!(!server_row(&report, "b").default);
    assert!(
        !server_row(&report, "d").default,
        "a set keeps a server that is off, but nothing reaches it"
    );
    assert_eq!(report.default_access.unwrap().server_ids, vec!["a"]);
}

#[test]
fn test02_a_server_no_set_reaches_is_unreached_unless_it_is_off() {
    let w = world(Some("work"), vec![set("work", &["a"])]);
    let report = w.report();
    let unreached: Vec<&str> = report
        .servers
        .iter()
        .filter(|s| s.unreached)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(unreached, vec!["b", "c"]);
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|f| f.kind == FindingKind::Unreached)
            .count(),
        2
    );
}

#[test]
fn test03_without_an_explicit_default_every_enabled_server_is_reached() {
    let w = world(None, vec![set("work", &["a"])]);
    let report = w.report();
    assert!(report.servers.iter().all(|s| !s.unreached));
    assert!(server_row(&report, "c").default);
    assert!(!server_row(&report, "d").default);
}

#[test]
fn test04_a_clients_own_set_or_a_folder_set_reaches_what_the_default_leaves_out() {
    let w = world(
        Some("work"),
        vec![
            set("work", &["a"]),
            set("bare", &["b"]),
            set("proj", &["c"]),
        ],
    );
    w.scope("claude-code", "bare");
    w.folder("/home/test/proj", "proj");
    let report = w.report();
    assert_eq!(server_row(&report, "b").clients, vec!["claude-code"]);
    assert_eq!(server_row(&report, "c").folders, vec!["/home/test/proj"]);
    assert!(report.servers.iter().all(|s| !s.unreached));
}

#[test]
fn test05_a_folder_mapping_that_outranks_a_clients_set_is_named_with_the_rule() {
    let w = world(
        Some("work"),
        vec![
            set("work", &["a"]),
            set("bare", &["b", "c"]),
            set("proj", &["b", "a"]),
        ],
    );
    w.scope("claude-code", "bare");
    w.folder("/home/test/proj", "proj");
    let report = w.report();
    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == FindingKind::FolderOutranks)
        .unwrap();
    assert_eq!(finding.client.as_deref(), Some("claude-code"));
    assert_eq!(finding.folder.as_deref(), Some("/home/test/proj"));
    assert!(finding
        .text
        .contains("hides 1 server its own set lists (C)"));
    assert!(finding
        .text
        .contains("adds 1 server its own set leaves out (A)"));
    assert!(finding.text.contains(FOLDER_RULE));
}

#[test]
fn test06_a_folder_set_equal_to_the_clients_set_raises_nothing() {
    let w = world(Some("work"), vec![set("work", &["a"]), set("bare", &["b"])]);
    w.scope("claude-code", "bare");
    w.folder("/home/test/proj", "bare");
    assert!(w
        .report()
        .findings
        .iter()
        .all(|f| f.kind != FindingKind::FolderOutranks));
}

#[test]
fn test07_a_client_that_follows_the_default_gets_no_folder_finding() {
    let w = world(
        Some("work"),
        vec![set("work", &["a", "b"]), set("proj", &["a"])],
    );
    w.scope("claude-code", "");
    w.folder("/home/test/proj", "proj");
    let report = w.report();
    assert!(!report.clients[0].own_set);
    assert!(report
        .findings
        .iter()
        .all(|f| f.kind != FindingKind::FolderOutranks));
}

#[test]
fn test08_a_scope_that_names_a_deleted_set_reaches_nothing_and_is_reported() {
    let w = world(Some("work"), vec![set("work", &["a", "b", "c"])]);
    w.scope("claude-code", "gone");
    w.folder("/home/test/proj", "also-gone");
    let report = w.report();
    assert!(report.clients[0].scope.dangling);
    assert!(report.clients[0].scope.server_ids.is_empty());
    let missing: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.kind == FindingKind::MissingSet)
        .collect();
    assert_eq!(missing.len(), 2);
}

#[test]
fn test09_a_client_with_the_gateway_and_no_scope_follows_the_default_access() {
    let w = world(Some("work"), vec![set("work", &["a"])]);
    w.host.clients.clients.borrow_mut()[0].gateway_installed = true;
    let report = w.report();
    assert_eq!(report.clients.len(), 1);
    assert!(!report.clients[0].own_set);
    assert_eq!(report.clients[0].scope.server_ids, vec!["a"]);
    assert_eq!(server_row(&report, "a").clients, vec!["claude-code"]);
}

#[test]
fn test10_a_server_with_a_working_direct_entry_is_not_a_finding() {
    let servers = vec![server("a", true), server("b", true)];
    let sets = vec![set("work", &["a"])];
    let scopes = vec![ScopeReach {
        kind: ScopeKind::Default,
        key: String::new(),
        reference: String::new(),
        set_id: Some("work".into()),
        dangling: false,
        server_ids: vec!["a".into()],
    }];
    let rows = vec![direct::Row {
        client: "claude-code".into(),
        client_name: "Claude Code".into(),
        entry: "b".into(),
        server: "b".into(),
        server_name: Some("B".into()),
        state: direct::State::Ok,
        path: "/x".into(),
        url: None,
    }];
    let report = build(&servers, &sets, &scopes, &[], &rows);
    assert!(server_row(&report, "b").unreached);
    assert!(report.findings.is_empty());
    assert_eq!(server_row(&report, "b").direct[0].state, "ok");
}

#[test]
fn test11_assess_narrows_to_one_server_or_client_and_refuses_unknown_keys() {
    let w = world(Some("work"), vec![set("work", &["a"]), set("bare", &["b"])]);
    w.scope("claude-code", "bare");
    let one = assess(
        &w.host,
        Filter {
            server: Some("b"),
            client: None,
        },
    )
    .unwrap();
    assert_eq!(one.servers.len(), 1);
    assert_eq!(one.findings.len(), 0);
    let by_name = assess(
        &w.host,
        Filter {
            server: Some("c"),
            client: Some("Claude Code"),
        },
    )
    .unwrap();
    assert_eq!(by_name.servers[0].id, "c");
    assert_eq!(by_name.findings.len(), 1);
    let error = assess(
        &w.host,
        Filter {
            server: Some("zzz"),
            client: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, crate::op::ErrorKind::NotFound);
    let error = assess(
        &w.host,
        Filter {
            server: None,
            client: Some("zzz"),
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, crate::op::ErrorKind::NotFound);
}

#[test]
fn test12_the_access_rows_name_the_unreached_servers_and_each_outranking_folder() {
    let w = world(
        Some("work"),
        vec![
            set("work", &["a"]),
            set("bare", &["b"]),
            set("proj", &["a"]),
        ],
    );
    w.scope("claude-code", "bare");
    w.folder("/home/test/proj", "proj");
    let rows = access_rows(&w.report());
    assert_eq!(rows, access_rows_of(&w.host).unwrap());
    assert_eq!(rows, w.report().access_rows);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "access:unreached:c");
    assert_eq!(rows[0].title, "1 server no access set reaches");
    assert!(rows[0].detail.contains('C'));
    assert_eq!(rows[1].id, "access:folder:/home/test/proj:claude-code");
    assert!(rows[1].detail.contains(FOLDER_RULE));
}

#[test]
fn test13_no_registry_file_reports_an_empty_picture() {
    let host = MemoryHost::default();
    let report = assess(&host, Filter::default()).unwrap();
    assert!(report.servers.is_empty() && report.findings.is_empty());
    assert!(access_rows(&report).is_empty());
}

#[test]
fn test14_a_direct_entry_kept_after_its_server_was_removed_is_reported_with_the_fix() {
    let rows = vec![direct::Row {
        client: "claude-code".into(),
        client_name: "Claude Code".into(),
        entry: "gone".into(),
        server: "srv-gone".into(),
        server_name: None,
        state: direct::State::Orphan,
        path: "/x".into(),
        url: None,
    }];
    let report = build(&[server("a", true)], &[], &[], &[], &rows);
    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == FindingKind::DirectOrphan)
        .unwrap();
    assert_eq!(finding.client.as_deref(), Some("claude-code"));
    assert!(finding
        .text
        .contains("`client direct rm srv-gone --client claude-code`"));
    assert!(report
        .access_rows
        .iter()
        .all(|row| !row.detail.contains("direct entry")));
}
