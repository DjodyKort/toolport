use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use toolport_plus_api::memory::{MemoryHost, MemorySecrets};
use toolport_plus_api::{convert, CatalogEntryRecord, HttpError, HttpResponse};

use super::store::{self, INDEX_FILE, LIVE_FILE, LOCK_FILE};
use super::sync::{Tuning, TUNING};
use super::*;
use crate::tasks::cron::rfc3339;

const T0: i64 = 1_800_000_000;
const NO_WAIT: Tuning = Tuning {
    retry_pause: Duration::ZERO,
    ..TUNING
};

fn host(dir: &Path) -> MemoryHost {
    let host = MemoryHost {
        secrets: MemorySecrets {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        },
        ..Default::default()
    };
    host.now.set(T0);
    host
}

fn server(id: &str, description: &str) -> Value {
    json!({
        "server": {
            "name": id, "title": id.rsplit('/').next().unwrap(), "description": description,
            "remotes": [{"url": format!("https://{}.invalid/mcp", id.replace('/', "-"))}],
        },
        "_meta": {"io.modelcontextprotocol.registry/official": {"isLatest": true, "status": "active"}},
    })
}

fn retired(id: &str) -> Value {
    let mut item = server(id, "");
    item["_meta"]["io.modelcontextprotocol.registry/official"]["status"] = json!("deleted");
    item
}

fn page(items: Vec<Value>, next: Option<&str>) -> Result<HttpResponse, HttpError> {
    let body = json!({"servers": items, "metadata": {"nextCursor": next, "count": 0}});
    Ok(HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
        truncated: false,
        url: String::new(),
    })
}

fn script(host: &MemoryHost, pages: Vec<Result<HttpResponse, HttpError>>) {
    host.http.responses.borrow_mut().extend(pages);
}

fn requested(host: &MemoryHost) -> Vec<String> {
    host.http
        .requests
        .borrow()
        .iter()
        .map(|request| request.url.clone())
        .collect()
}

fn run(host: &MemoryHost, mode: Mode) -> Result<Report, OpError> {
    sync::sync_with(host, mode, NO_WAIT, &mut |_| {})
}

fn record(name: &str, url: &str) -> CatalogEntryRecord {
    convert(&json!({
        "name": name, "description": "", "transport": "http", "command": null, "args": [],
        "url": url, "envKeys": [], "source": "registry", "homepage": null,
    }))
    .unwrap()
}

fn names(records: &[CatalogEntryRecord]) -> Vec<&str> {
    records.iter().map(|r| r.name.as_str()).collect()
}

fn synced(dir: &Path) -> MemoryHost {
    let host = host(dir);
    script(
        &host,
        vec![
            page(
                vec![
                    server("io.github.acme/widget", "A widget for sheets"),
                    server("io.github.acme/gadget", "Gadgets"),
                ],
                Some("c2"),
            ),
            page(
                vec![server("com.example/stripe-payments", "Payments")],
                None,
            ),
        ],
    );
    run(&host, Mode::Full).unwrap();
    host.http.requests.borrow_mut().clear();
    host
}

fn plus(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("plus")
}

#[test]
fn test01_first_sync_pages_by_cursor_and_writes_the_index_once() {
    let dir = TempDir::new().unwrap();
    let host = host(dir.path());
    script(
        &host,
        vec![
            page(vec![server("a/one", "1")], Some("c 2")),
            page(vec![server("a/two", "2")], None),
        ],
    );
    let report = run(&host, Mode::Incremental).unwrap();
    assert_eq!((report.kind, report.pages, report.count), ("full", 2, 2));
    assert!(report.complete);
    let urls = requested(&host);
    assert_eq!(urls[0], format!("{REGISTRY_URL}?limit=100&version=latest"));
    assert_eq!(
        urls[1],
        format!("{REGISTRY_URL}?limit=100&version=latest&cursor=c%202")
    );
    assert_eq!(store::read_main(&plus(&dir)).entries.len(), 2);
    assert_eq!(store::part_count(&plus(&dir)), 0);
}

#[test]
fn test02_a_fresh_index_answers_a_search_without_the_live_call() {
    let dir = TempDir::new().unwrap();
    let host = synced(dir.path());
    let hits: Result<_, ()> = answer(&host, "sheets", |_| panic!("the live search ran"));
    assert_eq!(names(&hits.unwrap()), ["widget"]);
    let hits: Result<_, ()> = answer(&host, "io.github.acme", |_| panic!("the live search ran"));
    assert_eq!(names(&hits.unwrap()), ["gadget", "widget"]);
}

#[test]
fn test03_a_stale_index_asks_the_live_search_and_keeps_its_hits() {
    let dir = TempDir::new().unwrap();
    let host = synced(dir.path());
    host.now.set(T0 + 25 * 3600);
    let live: Result<_, ()> = answer(&host, "zzz", |_| {
        Ok(vec![record("Zzz Tool", "https://zzz.invalid")])
    });
    assert_eq!(names(&live.unwrap()), ["Zzz Tool"]);
    let down: Result<_, &str> = answer(&host, "zzz", |_| Err("timed out"));
    assert_eq!(names(&down.unwrap()), ["Zzz Tool"]);
    assert!(plus(&dir).join(LIVE_FILE).exists());
    assert_eq!(
        store::read_main(&plus(&dir)).entries.len(),
        3,
        "a live hit never rewrites the synced index"
    );
}

#[test]
fn test04_a_failing_live_search_serves_stale_hits_and_errors_without_any() {
    let dir = TempDir::new().unwrap();
    let host = synced(dir.path());
    host.now.set(T0 + 30 * 3600);
    let stale: Result<_, &str> = answer(&host, "widget", |_| Err("timed out"));
    assert_eq!(names(&stale.unwrap()), ["widget"]);
    let none: Result<_, &str> = answer(&host, "nothing-like-it", |_| Err("timed out"));
    assert_eq!(none.unwrap_err(), "timed out");

    let empty = TempDir::new().unwrap();
    let fresh_host = self::host(empty.path());
    let error: Result<Vec<CatalogEntryRecord>, &str> =
        answer(&fresh_host, "widget", |_| Err("timed out"));
    assert_eq!(error.unwrap_err(), "timed out");
}

#[test]
fn test05_an_incremental_sync_asks_updated_since_and_applies_changes() {
    let dir = TempDir::new().unwrap();
    let host = synced(dir.path());
    host.now.set(T0 + 13 * 3600);
    script(
        &host,
        vec![page(
            vec![
                server("io.github.acme/widget", "A widget, now with charts"),
                retired("io.github.acme/gadget"),
                server("org.new/fresh", "Brand new"),
            ],
            None,
        )],
    );
    let report = run(&host, Mode::Incremental).unwrap();
    assert_eq!((report.kind, report.count), ("incremental", 3));
    let since = rfc3339(T0);
    assert_eq!(
        requested(&host),
        [format!(
            "{REGISTRY_URL}?limit=100&version=latest&updated_since={}",
            since.replace(':', "%3A")
        )]
    );
    let hits: Result<_, ()> = answer(&host, "charts", |_| panic!("the live search ran"));
    assert_eq!(names(&hits.unwrap()), ["widget"]);
    let gone: Result<_, ()> = answer(&host, "gadget", |_| panic!("the live search ran"));
    assert!(gone.unwrap().is_empty());
}

#[test]
fn test06_a_stopped_pass_resumes_from_the_last_flushed_cursor() {
    let dir = TempDir::new().unwrap();
    let host = host(dir.path());
    let mut pages: Vec<_> = (0..12)
        .map(|n| {
            page(
                vec![server(&format!("a/s{n}"), "x")],
                Some(&format!("c{}", n + 1)),
            )
        })
        .collect();
    pages[11] = page(vec![server("a/s11", "x")], None);
    script(&host, pages[..10].to_vec());
    script(
        &host,
        vec![
            Err(HttpError::Connect("down".into())),
            Err(HttpError::Connect("down".into())),
        ],
    );
    let error = run(&host, Mode::Incremental).unwrap_err();
    assert_eq!(error.code(), "registry");
    assert_eq!(store::part_count(&plus(&dir)), 1);
    assert!(!plus(&dir).join(INDEX_FILE).exists());
    let state = status(&host);
    assert_eq!((state.state, state.resumable), (STATE_ERROR, true));

    host.http.requests.borrow_mut().clear();
    script(&host, pages[10..].to_vec());
    let report = run(&host, Mode::Incremental).unwrap();
    assert_eq!((report.kind, report.count), ("resume", 12));
    assert_eq!(
        requested(&host)[0],
        format!("{REGISTRY_URL}?limit=100&version=latest&cursor=c10")
    );
    assert_eq!(store::part_count(&plus(&dir)), 0);
    assert_eq!(status(&host).state, STATE_FRESH);
}

#[test]
fn test07_a_full_sync_drops_what_the_registry_no_longer_lists() {
    let dir = TempDir::new().unwrap();
    let host = synced(dir.path());
    let hit: Result<_, ()> = answer(&host, "gadget", |_| panic!("the live search ran"));
    assert_eq!(hit.unwrap().len(), 1);
    store::remember(&plus(&dir), &[record("Live", "https://live.invalid")]).unwrap();
    script(
        &host,
        vec![page(vec![server("io.github.acme/widget", "w")], None)],
    );
    host.now.set(T0 + 3600);
    let report = run(&host, Mode::Full).unwrap();
    assert_eq!((report.kind, report.count), ("full", 1));
    assert!(!plus(&dir).join(LIVE_FILE).exists());
    let gone: Result<_, ()> = answer(&host, "gadget", |_| Ok(Vec::new()));
    assert!(gone.unwrap().is_empty());
}

#[test]
fn test08_hits_are_ranked_by_name_and_capped() {
    let dir = TempDir::new().unwrap();
    let host = host(dir.path());
    let mut items: Vec<Value> = (0..150)
        .map(|n| server(&format!("a/tool-{n:03}"), "mentions search"))
        .collect();
    items.push(server("a/search", "exact"));
    items.push(server("a/search-engine", "prefix"));
    script(&host, vec![page(items, None)]);
    run(&host, Mode::Full).unwrap();
    let hits: Result<_, ()> = answer(&host, "search", |_| panic!("the live search ran"));
    let hits = hits.unwrap();
    assert_eq!(hits.len(), 100);
    assert_eq!(names(&hits)[..2], ["search", "search-engine"]);
}

#[test]
fn test09_status_names_never_synced_fresh_stale_and_syncing() {
    let dir = TempDir::new().unwrap();
    let host = host(dir.path());
    let never = status(&host);
    assert_eq!(
        (never.state, never.count, never.last_synced_at),
        (STATE_STALE, 0, None)
    );
    let host = synced(dir.path());
    let fresh = status(&host);
    assert_eq!((fresh.state, fresh.count), (STATE_FRESH, 3));
    assert_eq!(fresh.last_synced_at, Some(rfc3339(T0)));
    host.now.set(T0 + 25 * 3600);
    assert_eq!(status(&host).state, STATE_STALE);
    let _held = crate::tasks::store::lock(&plus(&dir).join(LOCK_FILE), Duration::ZERO).unwrap();
    assert_eq!(status(&host).state, STATE_SYNCING);
    let refused = run(&host, Mode::Incremental).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Conflict);
}

#[test]
fn test10_a_failed_sync_is_recorded_with_its_reason() {
    let dir = TempDir::new().unwrap();
    let host = host(dir.path());
    script(
        &host,
        vec![
            Ok(HttpResponse {
                status: 503,
                headers: Vec::new(),
                body: Vec::new(),
                truncated: false,
                url: String::new(),
            });
            2
        ],
    );
    run(&host, Mode::Full).unwrap_err();
    let state = status(&host);
    assert_eq!(state.state, STATE_ERROR);
    let error = state.error.unwrap();
    assert_eq!(error.kind, "registry");
    assert!(error.message.contains("503"), "{}", error.message);
}

#[test]
fn test11_a_repeated_page_cursor_stops_the_sync() {
    let dir = TempDir::new().unwrap();
    let host = host(dir.path());
    script(
        &host,
        vec![
            page(vec![server("a/one", "1")], Some("same")),
            page(vec![server("a/two", "2")], Some("same")),
        ],
    );
    let error = run(&host, Mode::Full).unwrap_err();
    assert!(error.message.contains("repeated a page cursor"));
}

#[test]
fn test12_tick_syncs_when_due_and_backs_off_after_a_failure() {
    let dir = TempDir::new().unwrap();
    let host = host(dir.path());
    script(&host, vec![Err(HttpError::Connect("down".into())); 2]);
    assert!(matches!(tick_with(&host, NO_WAIT), Tick::Failed(_)));
    assert_eq!(tick_with(&host, NO_WAIT), Tick::NotDue);
    host.now.set(T0 + 11 * 60);
    script(&host, vec![page(vec![server("a/one", "1")], None)]);
    assert!(matches!(tick_with(&host, NO_WAIT), Tick::Synced(_)));
    host.now.set(T0 + 3 * 3600);
    assert_eq!(tick_with(&host, NO_WAIT), Tick::NotDue);
    host.now.set(T0 + 13 * 3600);
    script(&host, vec![page(vec![], None)]);
    assert!(matches!(tick_with(&host, NO_WAIT), Tick::Synced(_)));
}

#[test]
fn test13_a_corrupt_or_foreign_index_file_reads_as_empty() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(plus(&dir)).unwrap();
    std::fs::write(plus(&dir).join(INDEX_FILE), "{ not json").unwrap();
    let host = host(dir.path());
    assert_eq!(status(&host).count, 0);
    std::fs::write(
        plus(&dir).join(INDEX_FILE),
        r#"{"version":99,"lastSyncedAt":"2027-01-01T00:00:00Z","entries":[]}"#,
    )
    .unwrap();
    assert_eq!(status(&host).last_synced_at, None);
}

#[test]
fn test14_a_bounded_sync_saves_its_pass_and_the_next_call_continues_it() {
    let dir = TempDir::new().unwrap();
    let host = host(dir.path());
    script(
        &host,
        vec![
            page(vec![server("a/one", "1")], Some("c2")),
            page(vec![server("a/two", "2")], Some("c3")),
            page(vec![server("a/three", "3")], None),
        ],
    );
    let first = sync_bounded(&host, Mode::Incremental, 2, &mut |_| {}).unwrap();
    assert_eq!((first.pages, first.count, first.complete), (2, 2, false));
    assert_eq!(store::part_count(&plus(&dir)), 1);
    assert!(status(&host).resumable);

    let second = sync_bounded(&host, Mode::Incremental, 2, &mut |_| {}).unwrap();
    assert_eq!(
        (second.kind, second.count, second.complete),
        ("resume", 3, true)
    );
    assert_eq!(status(&host).state, STATE_FRESH);
}

#[test]
fn test15_search_lists_curated_entries_before_the_index_without_asking_the_network() {
    let dir = TempDir::new().unwrap();
    let mut host = synced(dir.path());
    host.catalog = vec![record("Widget Curated", "https://curated.invalid/mcp")];
    let hits = search(&host, "widget");
    assert_eq!(
        hits.first().map(|hit| hit.name.as_str()),
        Some("Widget Curated")
    );
    assert!(hits.len() >= 2);
    assert!(host.http.requests.borrow().is_empty());
}
