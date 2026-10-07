use super::testkit::{
    entry, index_of, page_body, query_param, server_item, write_index, MockRegistry, Scratch,
    TEST_TUNING,
};
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const BASE: &str = "/v0.1/servers?limit=100&version=latest";

fn sync_incremental(scratch: &Scratch, base_url: &str) -> Result<SyncReport, SyncError> {
    sync_at(
        &scratch.index,
        base_url,
        SyncMode::Incremental,
        TEST_TUNING,
        &mut |_| {},
    )
}

fn hours_ago(hours: i64) -> String {
    cron::rfc3339(cron::now() - hours * 3600)
}

fn names(index: &RegistryIndex) -> Vec<&str> {
    index.entries.values().map(|e| e.name.as_str()).collect()
}

fn three_pages(url: &str) -> (u16, String) {
    let body = match query_param(url, "cursor").as_deref() {
        None => page_body(
            vec![
                server_item("Alpha", "First server", "active"),
                server_item("Beta", "Second server", "active"),
            ],
            Some("c 2/+"),
        ),
        Some("c 2/+") => page_body(
            vec![server_item("Gamma", "Third server", "active")],
            Some("c3"),
        ),
        Some(_) => page_body(vec![server_item("Delta", "Fourth server", "active")], None),
    };
    (200, body)
}

#[test]
fn a_full_sync_pages_the_registry_and_persists_a_complete_index() {
    let registry = MockRegistry::start(three_pages);
    let scratch = Scratch::new("full");
    let mut pages_seen = Vec::new();

    let report = sync_at(
        &scratch.index,
        &registry.base_url,
        SyncMode::Incremental,
        TEST_TUNING,
        &mut |progress| pages_seen.push((progress.pages, progress.count)),
    )
    .unwrap();

    assert_eq!((report.kind, report.pages, report.fetched), ("full", 3, 4));
    assert!(report.complete);
    assert_eq!(report.count, 4);
    assert_eq!(pages_seen, [(1, 2), (2, 3), (3, 4)]);
    assert_eq!(
        registry.requests(),
        [
            BASE.to_string(),
            format!("{BASE}&cursor=c%202%2F%2B"),
            format!("{BASE}&cursor=c3"),
        ]
    );
    let on_disk = read_index(&scratch.index);
    assert_eq!(names(&on_disk), ["Alpha", "Beta", "Delta", "Gamma"]);
    assert!(on_disk.complete);
    assert!(on_disk.resume_cursor.is_none() && on_disk.pass_started_at.is_none());
    assert_eq!(on_disk.last_synced_at, report.last_synced_at);
    assert!(cron::parse_rfc3339(on_disk.last_synced_at.as_deref().unwrap()).is_some());
    assert_eq!(*load(&scratch.index), on_disk);
}

#[test]
fn an_interrupted_sync_keeps_its_pages_and_resumes_from_the_cursor() {
    let broken = Arc::new(AtomicBool::new(true));
    let registry = {
        let broken = Arc::clone(&broken);
        MockRegistry::start(move |url| match query_param(url, "cursor").as_deref() {
            None => (
                200,
                page_body(
                    vec![server_item("Alpha", "First server", "active")],
                    Some("c2"),
                ),
            ),
            Some(_) if broken.load(Ordering::SeqCst) => (500, "oops".into()),
            Some(_) => (
                200,
                page_body(vec![server_item("Beta", "Second server", "active")], None),
            ),
        })
    };
    let scratch = Scratch::new("resume");

    let error = sync_incremental(&scratch, &registry.base_url).unwrap_err();

    assert!(matches!(error, SyncError::Registry(..)), "{error:?}");
    assert_eq!(error.report().unwrap().pages, 1);
    assert!(error.message().contains("Run it again to continue"));
    assert_eq!(registry.requests().len(), 3, "page one, then page two and its retry");
    let stopped = read_index(&scratch.index);
    assert_eq!(names(&stopped), ["Alpha"]);
    assert_eq!(stopped.resume_cursor.as_deref(), Some("c2"));
    assert!(!stopped.complete && stopped.last_synced_at.is_none());
    let pass_started_at = stopped.pass_started_at.clone().unwrap();
    assert_eq!(load(&scratch.index).search("first").len(), 1, "a partial index is searchable");
    assert!(load(&scratch.index).needs_live());

    broken.store(false, Ordering::SeqCst);
    let report = sync_incremental(&scratch, &registry.base_url).unwrap();

    assert_eq!((report.kind, report.pages), ("resume", 1));
    assert_eq!(registry.requests()[3], format!("{BASE}&cursor=c2"));
    assert_eq!(registry.requests().len(), 4, "page one is not fetched again");
    let done = read_index(&scratch.index);
    assert_eq!(names(&done), ["Alpha", "Beta"]);
    assert!(done.complete && done.resume_cursor.is_none());
    assert_eq!(done.last_synced_at, Some(pass_started_at));
}

#[test]
fn one_retry_hides_a_transient_page_failure() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let registry = {
        let attempts = Arc::clone(&attempts);
        MockRegistry::start(move |url| match query_param(url, "cursor").as_deref() {
            None => (
                200,
                page_body(vec![server_item("Alpha", "x", "active")], Some("c2")),
            ),
            Some(_) if attempts.fetch_add(1, Ordering::SeqCst) == 0 => (503, "busy".into()),
            Some(_) => (
                200,
                page_body(vec![server_item("Beta", "y", "active")], None),
            ),
        })
    };
    let scratch = Scratch::new("retry");

    let report = sync_incremental(&scratch, &registry.base_url).unwrap();

    assert!(report.complete);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(registry.requests().len(), 3);
}

#[test]
fn an_incremental_sync_asks_for_updates_since_the_last_sync_and_merges_them() {
    let scratch = Scratch::new("incremental");
    let since = "2026-10-06T00:00:00Z";
    write_index(
        &scratch.index,
        &index_of(
            &[
                entry("Kept", "Untouched"),
                entry("Changed", "Old description"),
                entry("Gone", "About to be deprecated"),
            ],
            Some(since),
            true,
        ),
    );
    let registry = MockRegistry::start(|_| {
        (
            200,
            page_body(
                vec![
                    server_item("Changed", "New description", "active"),
                    server_item("Fresh", "Published since", "active"),
                    server_item("Gone", "About to be deprecated", "deprecated"),
                ],
                None,
            ),
        )
    });

    let report = sync_incremental(&scratch, &registry.base_url).unwrap();

    assert_eq!((report.kind, report.pages), ("incremental", 1));
    assert_eq!(
        registry.requests(),
        [format!("{BASE}&updated_since=2026-10-06T00%3A00%3A00Z")]
    );
    let merged = read_index(&scratch.index);
    assert_eq!(names(&merged), ["Changed", "Fresh", "Kept"]);
    assert_eq!(merged.entries["io.example/changed"].description, "New description");
    assert!(merged.complete);
    let advanced = cron::parse_rfc3339(merged.last_synced_at.as_deref().unwrap()).unwrap();
    assert!(advanced > cron::parse_rfc3339(since).unwrap());
}

#[test]
fn an_interrupted_incremental_sync_starts_over_from_the_same_point() {
    let scratch = Scratch::new("incremental-stop");
    let since = "2026-10-06T00:00:00Z";
    write_index(
        &scratch.index,
        &index_of(&[entry("Kept", "x")], Some(since), true),
    );
    let registry = MockRegistry::start(|url| match query_param(url, "cursor") {
        None => (
            200,
            page_body(vec![server_item("Fresh", "x", "active")], Some("c2")),
        ),
        Some(_) => (500, "oops".into()),
    });

    assert!(sync_incremental(&scratch, &registry.base_url).is_err());

    let stopped = read_index(&scratch.index);
    assert_eq!(names(&stopped), ["Fresh", "Kept"]);
    assert_eq!(stopped.last_synced_at.as_deref(), Some(since));
    assert!(stopped.resume_cursor.is_none());
}

#[test]
fn a_full_sync_drops_what_the_registry_no_longer_lists() {
    let scratch = Scratch::new("prune");
    write_index(
        &scratch.index,
        &index_of(
            &[entry("Alpha", "x"), entry("Stale", "removed upstream")],
            Some(&hours_ago(1)),
            true,
        ),
    );
    let registry = MockRegistry::start(|_| {
        (200, page_body(vec![server_item("Alpha", "x", "active")], None))
    });

    let report = sync_at(
        &scratch.index,
        &registry.base_url,
        SyncMode::Full,
        TEST_TUNING,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(report.kind, "full");
    assert_eq!(registry.requests(), [BASE.to_string()]);
    assert_eq!(names(&read_index(&scratch.index)), ["Alpha"]);
}

#[test]
fn servers_sharing_a_title_are_kept_apart_by_registry_id() {
    let scratch = Scratch::new("same-title");
    let titled = |id: &str, status: &str| {
        let mut item = server_item("GitHub", id, status);
        item["server"]["name"] = json!(id);
        item
    };
    let registry = MockRegistry::start(move |_| {
        (
            200,
            page_body(
                vec![
                    titled("io.github.acme/github", "active"),
                    titled("io.github.other/github", "active"),
                    titled("io.github.old/github", "deleted"),
                ],
                None,
            ),
        )
    });

    sync_incremental(&scratch, &registry.base_url).unwrap();

    let index = read_index(&scratch.index);
    assert_eq!(
        index.entries.keys().collect::<Vec<_>>(),
        ["io.github.acme/github", "io.github.other/github"]
    );
}

#[test]
fn a_resumed_pass_keeps_entries_it_cannot_prove_are_gone() {
    let scratch = Scratch::new("no-prune");
    let mut unfinished = index_of(&[entry("Early", "from the first process"), entry("Old", "x")], None, false);
    unfinished.resume_cursor = Some("c2".into());
    unfinished.pass_started_at = Some(hours_ago(2));
    write_index(&scratch.index, &unfinished);
    let registry = MockRegistry::start(|_| {
        (200, page_body(vec![server_item("Late", "x", "active")], None))
    });

    let report = sync_incremental(&scratch, &registry.base_url).unwrap();

    assert_eq!(report.kind, "resume");
    assert_eq!(names(&read_index(&scratch.index)), ["Early", "Late", "Old"]);
}

#[test]
fn a_full_sync_restarts_an_unfinished_pass_from_its_first_page() {
    let scratch = Scratch::new("full-restart");
    let mut unfinished = index_of(&[entry("Early", "x")], None, false);
    unfinished.resume_cursor = Some("c2".into());
    write_index(&scratch.index, &unfinished);
    let registry = MockRegistry::start(three_pages);

    let report = sync_at(
        &scratch.index,
        &registry.base_url,
        SyncMode::Full,
        TEST_TUNING,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!((report.kind, report.pages), ("full", 3));
    assert_eq!(registry.requests()[0], BASE);
    assert!(!names(&read_index(&scratch.index)).contains(&"Early"));
}

#[test]
fn a_second_sync_of_the_same_index_reports_that_one_is_running() {
    let scratch = Scratch::new("single-flight");
    let registry = MockRegistry::start(|_| {
        std::thread::sleep(Duration::from_millis(300));
        (200, page_body(vec![server_item("Alpha", "x", "active")], None))
    });
    let first = {
        let (path, base_url) = (scratch.index.clone(), registry.base_url.clone());
        std::thread::spawn(move || {
            sync_at(&path, &base_url, SyncMode::Incremental, TEST_TUNING, &mut |_| {})
        })
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !is_running(&scratch.index) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(status_at(&scratch.index)["running"], true);

    let second = sync_incremental(&scratch, &registry.base_url).unwrap_err();

    assert!(matches!(second, SyncError::Running), "{second:?}");
    assert!(second.message().contains("already running"));
    assert!(first.join().unwrap().is_ok());
    assert_eq!(status_at(&scratch.index)["running"], false);
    assert!(sync_incremental(&scratch, &registry.base_url).is_ok());
}

#[test]
fn a_registry_that_repeats_a_cursor_stops_the_sync() {
    let scratch = Scratch::new("cursor-loop");
    let registry = MockRegistry::start(|_| {
        (
            200,
            page_body(vec![server_item("Alpha", "x", "active")], Some("same")),
        )
    });

    let error = sync_incremental(&scratch, &registry.base_url).unwrap_err();

    assert!(error.message().contains("repeated a page cursor"), "{}", error.message());
    assert!(registry.requests().len() <= 3);
}

#[test]
fn a_sync_stops_at_the_page_guard_and_the_next_one_carries_on() {
    let registry = MockRegistry::start(three_pages);
    let scratch = Scratch::new("page-guard");
    let guarded = Tuning {
        max_pages: 2,
        ..TEST_TUNING
    };

    let error = sync_at(
        &scratch.index,
        &registry.base_url,
        SyncMode::Incremental,
        guarded,
        &mut |_| {},
    )
    .unwrap_err();

    assert!(error.message().contains("more than 2 pages"), "{}", error.message());
    assert_eq!(read_index(&scratch.index).resume_cursor.as_deref(), Some("c3"));

    let report = sync_at(
        &scratch.index,
        &registry.base_url,
        SyncMode::Incremental,
        guarded,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!((report.kind, report.pages, report.complete), ("resume", 1, true));
    assert_eq!(read_index(&scratch.index).len(), 4);
}

#[test]
fn a_missing_or_corrupt_index_is_an_empty_one() {
    let scratch = Scratch::new("corrupt");
    assert!(load(&scratch.index).is_empty());
    assert!(load(&scratch.index).needs_live());
    assert_eq!(status_at(&scratch.index)["count"], 0);

    let foreign_version = json!({"version": INDEX_FILE_VERSION + 1, "entries": {}}).to_string();
    for junk in [
        "",
        "{not json",
        "[]",
        r#"{"version":2,"entries":"nope"}"#,
        foreign_version.as_str(),
    ] {
        std::fs::create_dir_all(scratch.index.parent().unwrap()).unwrap();
        std::fs::write(&scratch.index, junk).unwrap();
        assert!(read_index(&scratch.index).is_empty(), "{junk}");
    }
}

#[test]
fn a_sync_replaces_a_corrupt_index() {
    let scratch = Scratch::new("corrupt-sync");
    std::fs::create_dir_all(scratch.index.parent().unwrap()).unwrap();
    std::fs::write(&scratch.index, "{not json").unwrap();
    let registry = MockRegistry::start(three_pages);

    sync_incremental(&scratch, &registry.base_url).unwrap();

    assert_eq!(read_index(&scratch.index).len(), 4);
}

#[test]
fn an_unwritable_index_keeps_the_synced_pages_in_memory() {
    let scratch = Scratch::new("unwritable");
    let blocker = scratch.dir.join("blocker");
    std::fs::write(&blocker, "a file where the directory should be").unwrap();
    let path = blocker.join("plus").join(INDEX_FILE);
    let registry = MockRegistry::start(three_pages);

    let error = sync_at(
        &path,
        &registry.base_url,
        SyncMode::Incremental,
        TEST_TUNING,
        &mut |_| {},
    )
    .unwrap_err();

    assert!(matches!(error, SyncError::Storage(..)), "{error:?}");
    assert!(!path.exists());
    assert_eq!(load(&path).len(), 4, "the synced pages stay searchable");
}

#[test]
fn search_ranks_the_hits_and_caps_them() {
    let mut entries: Vec<_> = (0..150)
        .map(|n| entry(&format!("tool-{n:03}"), "A helper"))
        .collect();
    entries.push(entry("github", "Exact name"));
    entries.push(entry("Some GitHub helper", "Mentions github"));
    let index = index_of(&entries, None, false);

    let hits = index.search("github");
    assert_eq!(hits[0].name, "github");
    assert_eq!(hits.len(), 2);
    assert_eq!(index.search("tool").len(), SEARCH_LIMIT);
    assert!(index.search("zzzz-nothing").is_empty());
}

#[test]
fn loading_picks_up_a_file_another_process_changed() {
    let scratch = Scratch::new("reload");
    write_index(&scratch.index, &index_of(&[entry("Alpha", "x")], None, false));
    let first = load(&scratch.index);
    assert_eq!(first.len(), 1);
    assert!(Arc::ptr_eq(&first, &load(&scratch.index)), "parsed once while unchanged");

    write_index(
        &scratch.index,
        &index_of(&[entry("Alpha", "x"), entry("Beta", "y")], None, false),
    );
    let bumped = SystemTime::now() + Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&scratch.index)
        .unwrap()
        .set_modified(bumped)
        .unwrap();

    assert_eq!(load(&scratch.index).len(), 2);
}

#[test]
fn staleness_follows_the_age_of_the_last_finished_pass() {
    let now = cron::now();
    let at = |hours: i64| index_of(&[entry("A", "x")], Some(&cron::rfc3339(now - hours * 3600)), true);

    assert!(!at(1).needs_live_at(now));
    assert!(!at(23).needs_live_at(now));
    assert!(at(25).needs_live_at(now));
    assert!(at(-2).needs_live_at(now), "a timestamp in the future is stale");
    assert!(index_of(&[entry("A", "x")], None, false).needs_live_at(now));
    assert!(index_of(&[], Some(&hours_ago(1)), true).needs_live_at(now));

    assert!(!at(11).due(now));
    assert!(at(13).due(now));
    let mut unfinished = at(1);
    unfinished.resume_cursor = Some("c2".into());
    assert!(unfinished.due(now));
    assert!(index_of(&[entry("A", "x")], Some(&hours_ago(1)), false).due(now));
}

#[test]
fn a_tick_syncs_only_a_stale_index_and_backs_off_after_a_failure() {
    let now = cron::now();
    let fresh = Scratch::new("tick-fresh");
    write_index(&fresh.index, &index_of(&[entry("A", "x")], Some(&hours_ago(1)), true));
    let registry = MockRegistry::start(|_| (500, "oops".into()));

    let idle = tick_at(&fresh.index, &registry.base_url, TEST_TUNING, false, now);
    assert_eq!((idle["ran"].clone(), idle["reason"].clone()), (json!(false), json!("not-due")));
    assert!(registry.requests().is_empty());

    let stale = Scratch::new("tick-stale");
    write_index(&stale.index, &index_of(&[entry("A", "x")], Some(&hours_ago(13)), true));
    let failed = tick_at(&stale.index, &registry.base_url, TEST_TUNING, false, now);
    assert_eq!(failed["ran"], true);
    assert_eq!(failed["error"]["kind"], "other");
    assert_eq!(failed["count"], 1);
    let requests = registry.requests().len();
    assert!(requests > 0);

    let soon = tick_at(&stale.index, &registry.base_url, TEST_TUNING, false, now + 60);
    assert_eq!(soon["reason"], "not-due");
    assert_eq!(registry.requests().len(), requests);

    let later = tick_at(&stale.index, &registry.base_url, TEST_TUNING, false, now + 11 * 60);
    assert_eq!(later["ran"], true);
    assert!(registry.requests().len() > requests);

    let forced = tick_at(&fresh.index, &registry.base_url, TEST_TUNING, true, now);
    assert_eq!(forced["ran"], true);
}

#[test]
fn a_tick_that_syncs_reports_the_new_state() {
    let scratch = Scratch::new("tick-ok");
    let registry = MockRegistry::start(three_pages);

    let outcome = tick_at(&scratch.index, &registry.base_url, TEST_TUNING, false, cron::now());

    assert_eq!(outcome["ran"], true);
    assert_eq!(outcome["count"], 4);
    assert_eq!(outcome["complete"], true);
    assert_eq!(outcome["report"]["pages"], 3);
    assert!(outcome.get("error").is_none());
}

#[test]
fn the_status_text_names_the_count_and_the_state() {
    let text = |value: Value| status_text(&value);
    assert_eq!(
        text(json!({"count": 12, "lastSyncedAt": "2026-10-07T09:00:00Z", "complete": true, "running": false})),
        "Registry index: 12 servers, synced 2026-10-07T09:00:00Z"
    );
    assert_eq!(
        text(json!({"count": 3, "lastSyncedAt": null, "complete": false, "running": true})),
        "Registry index: 3 servers, never fully synced, a sync is running"
    );
}
