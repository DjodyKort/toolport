use serde_json::{json, Map};
use toolport_plus_api::{HttpError, HttpResponse, MemoryHost, ScriptedHttp, ServerRecord};

use super::store::{HideKind, Store};
use super::tests::{host, Dir};
use super::{ops, snapshot, Item, Urgency};

fn server(id: &str, name: &str, command: &str, args: &[&str]) -> ServerRecord {
    let mut extra = Map::new();
    extra.insert("transport".into(), json!("stdio"));
    extra.insert("command".into(), json!(command));
    extra.insert("args".into(), json!(args));
    ServerRecord {
        id: id.into(),
        name: name.into(),
        enabled: true,
        extra,
        ..Default::default()
    }
}

fn npm(version: &str) -> Result<HttpResponse, HttpError> {
    answer(200, &format!(r#"{{"version":"{version}"}}"#))
}

fn answer(status: u16, body: &str) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
        truncated: false,
        url: "https://registry.test/".into(),
    })
}

fn with_replies(host: MemoryHost, replies: Vec<Result<HttpResponse, HttpError>>) -> MemoryHost {
    MemoryHost {
        http: ScriptedHttp::answering(replies),
        ..host
    }
}

fn rows(host: &MemoryHost) -> Vec<Item> {
    snapshot(host)
        .items
        .into_iter()
        .filter(|item| item.id.starts_with("updates:package:"))
        .collect()
}

fn asked(host: &MemoryHost) -> Vec<String> {
    host.http
        .requests
        .borrow()
        .iter()
        .map(|r| r.url.clone())
        .collect()
}

fn files_server() -> ServerRecord {
    server("srv-files", "files", "npx", &["-y", "@scope/files@1.4.2"])
}

#[test]
fn test80_a_pinned_package_with_a_newer_release_is_one_calm_skippable_row() {
    let dir = Dir::new("pkg-row");
    let host = with_replies(host(&dir, vec![files_server()]), vec![npm("1.5.0")]);
    assert!(rows(&host).is_empty());
    ops::check(&host).unwrap();
    let found = rows(&host);
    assert_eq!(found.len(), 1);
    let row = &found[0];
    assert_eq!(row.id, "updates:package:files:1.4.2>1.5.0");
    assert_eq!(row.urgency, Urgency::WorthALook);
    assert_eq!(row.source, "Updates");
    assert_eq!(
        row.title,
        "files: @scope/files 1.5.0 is out, pinned to 1.4.2"
    );
    assert_eq!(row.skip.as_deref(), Some("Skip 1.5.0"));
    assert_eq!(
        asked(&host),
        ["https://registry.npmjs.org/@scope%2Ffiles/latest"]
    );
}

#[test]
fn test81_the_list_never_asks_the_registry_only_the_explicit_check_does() {
    let dir = Dir::new("pkg-quiet");
    let host = with_replies(host(&dir, vec![files_server()]), vec![npm("1.5.0")]);
    for _ in 0..3 {
        snapshot(&host);
    }
    assert!(asked(&host).is_empty());
    ops::check(&host).unwrap();
    assert_eq!(asked(&host).len(), 1);
    for _ in 0..3 {
        snapshot(&host);
    }
    assert_eq!(asked(&host).len(), 1);
}

#[test]
fn test82_a_release_that_is_not_newer_or_a_package_nobody_knows_gives_no_row() {
    let dir = Dir::new("pkg-none");
    let servers = vec![
        files_server(),
        server("srv-same", "same", "npx", &["same@2.0.0"]),
        server("srv-private", "private", "npx", &["private-files@1.0.0"]),
        server("srv-unpinned", "unpinned", "npx", &["-y", "free-files"]),
    ];
    let host = with_replies(
        host(&dir, servers),
        vec![
            npm("1.4.2"),
            answer(404, r#"{"error":"not found"}"#),
            npm("2.0.0"),
        ],
    );
    ops::check(&host).unwrap();
    assert!(rows(&host).is_empty());
    assert_eq!(
        asked(&host).len(),
        3,
        "the unpinned server is not asked about"
    );
    let items = snapshot(&host).items;
    assert!(
        items.iter().all(|item| item.id != "updates:unreachable"),
        "a package the registry does not know is not an unreachable source: {items:?}"
    );
}

#[test]
fn test83_servers_sharing_a_package_cost_one_request_and_each_get_their_row() {
    let dir = Dir::new("pkg-shared");
    let mut second = files_server();
    second.id = "srv-files-2".into();
    second.name = "files two".into();
    let host = with_replies(host(&dir, vec![files_server(), second]), vec![npm("1.5.0")]);
    ops::check(&host).unwrap();
    assert_eq!(asked(&host).len(), 1);
    let ids: Vec<String> = rows(&host).into_iter().map(|item| item.id).collect();
    assert_eq!(
        ids,
        [
            "updates:package:files two:1.4.2>1.5.0",
            "updates:package:files:1.4.2>1.5.0"
        ]
    );
}

#[test]
fn test84_an_unreachable_registry_shows_the_offline_row_and_no_package_row() {
    let dir = Dir::new("pkg-offline");
    let host = with_replies(
        host(&dir, vec![files_server()]),
        vec![npm("1.5.0"), Err(HttpError::Connect("no route".into()))],
    );
    ops::check(&host).unwrap();
    let report = ops::check(&host).unwrap();
    assert_eq!(report.sources.len(), 1);
    assert!(report.sources[0]
        .error
        .as_ref()
        .unwrap()
        .contains("no route"));
    let ids: Vec<String> = snapshot(&host).items.into_iter().map(|i| i.id).collect();
    assert_eq!(ids, ["updates:unreachable"]);
    assert!(rows(&host).is_empty());
}

#[test]
fn test85_a_skipped_release_stays_skipped_and_a_newer_one_brings_the_row_back() {
    let dir = Dir::new("pkg-skip");
    let host = with_replies(
        host(&dir, vec![files_server()]),
        vec![npm("1.5.0"), npm("1.5.0"), npm("1.6.0")],
    );
    ops::check(&host).unwrap();
    let id = rows(&host)[0].id.clone();
    ops::hide(&host, &id, HideKind::Skip, None, false).unwrap();
    assert!(rows(&host).is_empty());
    ops::check(&host).unwrap();
    assert!(rows(&host).is_empty());
    ops::check(&host).unwrap();
    assert_eq!(rows(&host)[0].id, "updates:package:files:1.4.2>1.6.0");
    ops::undo(&host).unwrap();
}

#[test]
fn test86_a_switched_off_updates_check_and_disabled_servers_ask_nothing() {
    let dir = Dir::new("pkg-off");
    let mut off = files_server();
    off.enabled = false;
    let host = with_replies(host(&dir, vec![off]), vec![npm("9.9.9")]);
    ops::check(&host).unwrap();
    assert!(asked(&host).is_empty());
    host.registry.servers.borrow_mut()[0].enabled = true;
    let mut store = Store::read(&dir.0).unwrap();
    store.settings.checks.insert("updates".into(), false);
    store.write(&dir.0).unwrap();
    let report = ops::check(&host).unwrap();
    assert!(report.off && asked(&host).is_empty());
}
