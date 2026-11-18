use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};
use toolport_plus_api::{MemoryHost, MemoryRegistry, MemorySecrets, ServerRecord};

use super::feed_credentials::{self, CredentialInputs, ServerFacts};
use super::feed_library::{self, LibraryInputs};
use super::feed_tasks::{self, TaskFacts, TaskInputs};
use super::feed_updates::UpdateInputs;
use super::store::Store;
use super::{compute, order, Inputs, Item, Screen, Urgency};
use crate::auth::{AuthKind, AuthRow};
use crate::library::remote::LocalState;
use crate::skills::auto::{HeldItem, LastRun, State};
use crate::tasks::cron::rfc3339;
use crate::tasks::model;
use crate::tasks::schedule::MissedRecord;
use crate::tasks::store::{self, AttentionRecord, Run};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Dir(pub(super) PathBuf);

impl Dir {
    pub(super) fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "plus-attention-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) const NOW: i64 = 1_792_000_000;
const HOUR: i64 = 3600;

fn ago(secs: i64) -> String {
    rfc3339(NOW - secs)
}

fn server(id: &str, enabled: bool, missing: &[&str]) -> ServerFacts {
    ServerFacts {
        id: id.into(),
        name: id.into(),
        enabled,
        missing_secrets: missing.iter().map(|k| k.to_string()).collect(),
    }
}

fn login(server: &str, state: AuthKind, reason: &str, since: i64) -> AuthRow {
    AuthRow {
        server: server.into(),
        state,
        reason: reason.into(),
        since,
        expires_at: None,
        ttl_secs: (state == AuthKind::Expiring).then_some(5 * HOUR),
        last_probe: None,
        fix: None,
    }
}

fn credentials(servers: Vec<ServerFacts>, logins: Vec<AuthRow>) -> CredentialInputs {
    CredentialInputs {
        servers,
        vault_error: None,
        logins,
    }
}

fn run(id: &str, task: &str, status: &str, started: i64, steps: Value) -> Run {
    serde_json::from_value(json!({
        "id": id, "task": task, "trigger": "schedule", "status": status,
        "startedAt": rfc3339(started),
        "endedAt": (status != "waiting").then(|| rfc3339(started + 60)),
        "durationMs": null, "steps": steps,
    }))
    .unwrap()
}

fn step(title: &str, status: &str) -> Value {
    json!({"id": title, "title": title, "type": "exec", "status": status,
           "startedAt": null, "endedAt": null, "output": ""})
}

fn task(id: &str) -> TaskFacts {
    TaskFacts {
        id: id.into(),
        title: id.into(),
        enabled: true,
    }
}

fn tasks(tasks: Vec<TaskFacts>, runs: Vec<Run>) -> TaskInputs {
    TaskInputs {
        tasks,
        runs,
        missed: Vec::new(),
        records: Vec::new(),
    }
}

fn find<'a>(items: &'a [Item], id: &str) -> &'a Item {
    items.iter().find(|item| item.id == id).unwrap_or_else(|| {
        panic!(
            "no row {id} in {:?}",
            items.iter().map(|i| &i.id).collect::<Vec<_>>()
        )
    })
}

#[test]
fn test01_missing_secret_row_only_for_an_enabled_server() {
    let inputs = credentials(
        vec![
            server("alpha", true, &["API_KEY"]),
            server("beta", false, &["TOKEN"]),
            server("gamma", true, &[]),
        ],
        Vec::new(),
    );
    let items = feed_credentials::items(&inputs);
    assert_eq!(items.len(), 1);
    let row = &items[0];
    assert_eq!(row.id, "credentials:secret:alpha");
    assert_eq!(row.urgency, Urgency::NeedsYou);
    assert_eq!(row.title, "alpha is missing a secret");
    assert_eq!(
        row.detail,
        "API_KEY is not set, so the server cannot start."
    );
    let action = row.action.as_ref().unwrap();
    assert_eq!(action.screen, Screen::Servers);
    assert_eq!(
        action.command.as_deref().unwrap(),
        ["toolportctl", "secret", "set", "alpha", "API_KEY"]
    );
}

#[test]
fn test02_an_unreadable_vault_is_one_row_and_hides_secret_rows() {
    let mut inputs = credentials(vec![server("alpha", true, &["API_KEY"])], Vec::new());
    inputs.vault_error = Some("keychain is locked\nsecond line".into());
    let items = feed_credentials::items(&inputs);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "credentials:vault");
    assert_eq!(items[0].detail, "keychain is locked");
}

#[test]
fn test03_login_states_map_to_urgency_and_skip_healthy_or_switched_off_servers() {
    let inputs = credentials(
        vec![
            server("alpha", true, &[]),
            server("beta", true, &[]),
            server("gamma", true, &[]),
            server("delta", true, &[]),
            server("epsilon", true, &[]),
            server("off", false, &[]),
        ],
        vec![
            login(
                "alpha",
                AuthKind::NeedsReauth,
                "rejected with HTTP 401",
                NOW - 3 * HOUR,
            ),
            login("beta", AuthKind::Expiring, "token expires soon", NOW - HOUR),
            login("gamma", AuthKind::Unreachable, "no answer", NOW - HOUR),
            login("delta", AuthKind::Ok, "", NOW),
            login("epsilon", AuthKind::Revoked, "revoked", NOW - HOUR),
            login("off", AuthKind::NeedsReauth, "rejected", NOW - HOUR),
        ],
    );
    let items = feed_credentials::items(&inputs);
    assert_eq!(items.len(), 4);
    assert_eq!(
        find(&items, "credentials:login:alpha").title,
        "alpha needs a new sign-in"
    );
    assert_eq!(
        find(&items, "credentials:login:alpha").since,
        Some(NOW - 3 * HOUR)
    );
    assert_eq!(
        find(&items, "credentials:login:beta").urgency,
        Urgency::WorthALook
    );
    assert_eq!(
        find(&items, "credentials:login:beta").title,
        "beta sign-in expires in 5 h"
    );
    assert_eq!(
        find(&items, "credentials:login:gamma").urgency,
        Urgency::WorthALook
    );
    assert_eq!(
        find(&items, "credentials:login:epsilon").urgency,
        Urgency::NeedsYou
    );
}

#[test]
fn test04_a_waiting_run_is_a_needs_you_row_naming_the_step() {
    let waiting = run(
        "run-w",
        "export",
        "waiting",
        NOW - 1500,
        json!([
            step("fetch", "ok"),
            step("confirm upload", "waiting"),
            step("send", "pending")
        ]),
    );
    let items = feed_tasks::items(&tasks(vec![task("export")], vec![waiting]), NOW);
    let row = find(&items, "tasks:waiting:run-w");
    assert_eq!(row.urgency, Urgency::NeedsYou);
    assert_eq!(row.title, "Task export is waiting for you");
    assert_eq!(row.detail, "Stopped at step 2 of 3: confirm upload.");
    assert_eq!(
        row.action.as_ref().unwrap().command.as_deref().unwrap(),
        ["toolportctl", "task", "resume", "run-w"]
    );
}

#[test]
fn test05_one_failure_is_worth_a_look_and_two_in_a_row_need_you() {
    let failed = |id: &str, ago_secs: i64| {
        run(
            id,
            "nightly",
            "failed",
            NOW - ago_secs,
            json!([step("update", "failed")]),
        )
    };
    let once = tasks(vec![task("nightly")], vec![failed("r1", HOUR)]);
    let row = &feed_tasks::items(&once, NOW)[0];
    assert_eq!(
        (row.id.as_str(), row.urgency),
        ("tasks:failed:nightly", Urgency::WorthALook)
    );
    assert_eq!(row.detail, "Step 1 of 1 (update) ended with an error.");

    let twice = tasks(
        vec![task("nightly")],
        vec![failed("r1", HOUR), failed("r2", 5 * HOUR)],
    );
    let row = &feed_tasks::items(&twice, NOW)[0];
    assert_eq!(row.urgency, Urgency::NeedsYou);
    assert_eq!(row.title, "Task nightly failed 2 times in a row");
}

#[test]
fn test06_a_later_ok_run_clears_the_failure_row() {
    let runs = vec![
        run(
            "r3",
            "nightly",
            "ok",
            NOW - HOUR,
            json!([step("update", "ok")]),
        ),
        run(
            "r2",
            "nightly",
            "failed",
            NOW - 5 * HOUR,
            json!([step("update", "failed")]),
        ),
        run(
            "r1",
            "nightly",
            "failed",
            NOW - 9 * HOUR,
            json!([step("update", "failed")]),
        ),
    ];
    assert!(feed_tasks::items(&tasks(vec![task("nightly")], runs), NOW).is_empty());
}

#[test]
fn test07_a_failure_older_than_a_week_and_a_disabled_task_raise_nothing() {
    let old = run(
        "r1",
        "nightly",
        "failed",
        NOW - 8 * 86_400,
        json!([step("update", "failed")]),
    );
    let recent = run(
        "r2",
        "paused",
        "failed",
        NOW - HOUR,
        json!([step("update", "failed")]),
    );
    let mut paused = task("paused");
    paused.enabled = false;
    let inputs = tasks(vec![task("nightly"), paused], vec![old, recent]);
    assert!(feed_tasks::items(&inputs, NOW).is_empty());
}

fn too_old(task: &str, at: i64) -> MissedRecord {
    MissedRecord {
        task: task.into(),
        slots: 3,
        first_due: rfc3339(at - 20 * HOUR),
        last_due: rfc3339(at - 8 * HOUR),
        outcome: "too-old".into(),
        at: rfc3339(at),
        run: None,
    }
}

#[test]
fn test08_missed_slots_show_until_the_task_runs_again() {
    let mut inputs = tasks(vec![task("nightly")], Vec::new());
    inputs.missed = vec![too_old("nightly", NOW - 2 * HOUR)];
    let row = &feed_tasks::items(&inputs, NOW)[0];
    assert_eq!(row.id, "tasks:missed:nightly");
    assert_eq!(row.title, "Task nightly missed 3 scheduled runs");
    assert_eq!(row.urgency, Urgency::WorthALook);

    inputs.runs = vec![run(
        "r1",
        "nightly",
        "ok",
        NOW - HOUR,
        json!([step("update", "ok")]),
    )];
    assert!(feed_tasks::items(&inputs, NOW).is_empty());

    inputs.runs.clear();
    inputs.missed = vec![too_old("nightly", NOW - 4 * 86_400)];
    assert!(feed_tasks::items(&inputs, NOW).is_empty());
}

#[test]
fn test09_caught_up_and_busy_slots_are_not_missed_rows() {
    let mut inputs = tasks(vec![task("nightly")], Vec::new());
    let mut caught = too_old("nightly", NOW - HOUR);
    caught.outcome = "caught-up".into();
    let mut busy = too_old("nightly", NOW - HOUR);
    busy.outcome = "busy".into();
    inputs.missed = vec![caught, busy];
    assert!(feed_tasks::items(&inputs, NOW).is_empty());
}

fn record(id: &str, task: &str) -> AttentionRecord {
    AttentionRecord {
        id: id.into(),
        level: "needs-you".into(),
        title: "Task export is due".into(),
        detail: "the task has no autoRun schedule".into(),
        from: "tasks".into(),
        task: task.into(),
        since: ago(HOUR),
        action: Some(json!({"label": "Run task", "command": ["toolportctl", "task", "run", task]})),
    }
}

#[test]
fn test10_raised_task_records_show_only_for_tasks_that_still_exist() {
    let mut inputs = tasks(vec![task("export")], Vec::new());
    inputs.records = vec![
        record("tasks:export:due", "export"),
        record("tasks:gone:due", "gone"),
    ];
    let items = feed_tasks::items(&inputs, NOW);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "tasks:export:due");
    assert_eq!(items[0].action.as_ref().unwrap().label, "Run task");
    assert_eq!(items[0].since, Some(NOW - HOUR));
}

#[test]
fn test11_the_library_sync_task_is_left_to_the_library_feed() {
    let failed = run(
        "r1",
        "skills-auto-sync",
        "failed",
        NOW - HOUR,
        json!([step("sync", "failed")]),
    );
    let mut inputs = tasks(vec![task("skills-auto-sync")], vec![failed]);
    inputs.records = vec![record("tasks:skills-auto-sync:due", "skills-auto-sync")];
    assert!(feed_tasks::items(&inputs, NOW).is_empty());
}

fn last_run(held: usize, error: Option<&str>) -> State {
    State {
        last_run: Some(LastRun {
            at: ago(2 * HOUR),
            applied: 4,
            held: (0..held)
                .map(|n| HeldItem {
                    skill: format!("skill-{n}"),
                    client: "claude-code".into(),
                    path: "/x".into(),
                    op: "overwrite".into(),
                    reason: "the file differs from the library".into(),
                })
                .collect(),
            skipped: 0,
            error: error.map(String::from),
        }),
        ..State::default()
    }
}

#[test]
fn test12_held_changes_and_a_failed_auto_sync_are_needs_you_rows() {
    let inputs = LibraryInputs {
        auto: Ok(last_run(2, Some("git pull failed"))),
        repo: None,
    };
    let items = feed_library::items(&inputs);
    let held = find(&items, "library:held");
    assert_eq!(held.title, "2 changes wait for you in the library");
    assert_eq!(
        held.detail,
        "Auto-sync held back skill-0 for claude-code: the file differs from the library."
    );
    assert_eq!(held.action.as_ref().unwrap().screen, Screen::Library);
    assert_eq!(
        find(&items, "library:sync-failed").urgency,
        Urgency::NeedsYou
    );

    let clean = LibraryInputs {
        auto: Ok(last_run(0, None)),
        repo: None,
    };
    assert!(feed_library::items(&clean).is_empty());
}

#[test]
fn test13_a_diverged_library_needs_you_and_a_behind_one_is_worth_a_look() {
    let repo = |ahead, behind| LibraryInputs {
        auto: Ok(State::default()),
        repo: Some(LocalState {
            ahead,
            behind,
            head: Some("aaaaaaaa11112222".into()),
            upstream_head: Some("bbbbbbbb33334444".into()),
            ..LocalState::default()
        }),
    };
    let items = feed_library::items(&repo(1, 2));
    assert_eq!(
        (items[0].id.as_str(), items[0].urgency),
        ("library:diverged", Urgency::NeedsYou)
    );
    let items = feed_library::items(&repo(0, 3));
    assert_eq!(
        (items[0].id.as_str(), items[0].urgency),
        ("library:behind:aaaaaaaa>bbbbbbbb", Urgency::WorthALook)
    );
    assert!(feed_library::items(&repo(2, 0)).is_empty());
}

#[test]
fn test14_an_unreadable_auto_sync_state_is_a_row_not_silence() {
    let inputs = LibraryInputs {
        auto: Err("skills-auto.json is not valid".into()),
        repo: None,
    };
    assert_eq!(feed_library::items(&inputs)[0].id, "library:auto-state");
}

fn no_updates() -> UpdateInputs {
    UpdateInputs {
        servers: Vec::new(),
        taps: Vec::new(),
        failures: Vec::new(),
        checked: 0,
    }
}

fn item(id: &str, urgency: Urgency, since: Option<i64>) -> Item {
    Item {
        id: id.into(),
        urgency,
        title: id.into(),
        detail: String::new(),
        source: "Tasks",
        skip: None,
        since,
        action: None,
    }
}

#[test]
fn test15_rows_are_ordered_needs_you_first_then_newest_first_then_undated() {
    let ordered = order(vec![
        item("look-new", Urgency::WorthALook, Some(30)),
        item("need-old", Urgency::NeedsYou, Some(10)),
        item("need-undated", Urgency::NeedsYou, None),
        item("need-new", Urgency::NeedsYou, Some(20)),
    ]);
    let ids: Vec<&str> = ordered.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["need-new", "need-old", "need-undated", "look-new"]);
}

#[test]
fn test16_one_row_per_id_and_the_more_urgent_duplicate_wins() {
    let ordered = order(vec![
        item("same", Urgency::WorthALook, Some(50)),
        item("same", Urgency::NeedsYou, Some(10)),
        item("other", Urgency::WorthALook, Some(5)),
    ]);
    assert_eq!(ordered.len(), 2);
    assert_eq!(ordered[0].urgency, Urgency::NeedsYou);
}

#[test]
fn test17_a_feed_that_cannot_be_read_is_named_and_the_others_still_answer() {
    let inputs = Inputs {
        now: NOW,
        store: Ok(Store::default()),
        updates: Ok(no_updates()),
        credentials: Ok(credentials(
            vec![server("alpha", true, &["API_KEY"])],
            Vec::new(),
        )),
        tasks: Err("the data directory could not be resolved".into()),
        library: Ok(LibraryInputs {
            auto: Ok(State::default()),
            repo: None,
        }),
    };
    let snapshot = compute(&inputs);
    assert_eq!(snapshot.items.len(), 1);
    assert!(!snapshot.all_checked());
    let json = snapshot.to_json();
    assert_eq!(
        json["checks"][2],
        json!({"name": "tasks", "ok": false, "error": "the data directory could not be resolved"})
    );
    assert_eq!(json["counts"], json!({"needsYou": 1, "worthALook": 0}));
    assert!(super::human(&snapshot).contains("Not checked: tasks"));
}

pub(super) fn secret_server(id: &str, enabled: bool) -> ServerRecord {
    let mut record = ServerRecord {
        id: id.into(),
        name: id.into(),
        enabled,
        ..Default::default()
    };
    record.extra.insert(
        "env".into(),
        json!([{"key": "API_KEY", "secret": true}, {"key": "REGION", "value": "eu"}]),
    );
    record
}

pub(super) fn host(dir: &Dir, servers: Vec<ServerRecord>) -> MemoryHost {
    let host = MemoryHost {
        registry: MemoryRegistry::with_servers(servers),
        secrets: MemorySecrets {
            data_dir: Some(dir.0.clone()),
            ..Default::default()
        },
        ..Default::default()
    };
    host.now.set(NOW);
    host
}

#[test]
fn test18_the_secret_row_follows_the_vault_and_is_computed_fresh_each_call() {
    let dir = Dir::new("vault");
    let host = host(
        &dir,
        vec![secret_server("alpha", true), secret_server("off", false)],
    );
    let first = super::snapshot(&host);
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].id, "credentials:secret:alpha");

    host.secrets
        .vault
        .borrow_mut()
        .insert(("alpha".into(), "API_KEY".into()), "synthetic".into());
    assert!(super::snapshot(&host).items.is_empty());
}

#[test]
fn test19_a_vault_that_cannot_be_read_shows_the_vault_row_through_the_host() {
    let dir = Dir::new("locked");
    let host = host(&dir, vec![secret_server("alpha", true)]);
    *host.secrets.vault_error.borrow_mut() = Some("login keychain is locked".into());
    let snapshot = super::snapshot(&host);
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].id, "credentials:vault");
}

#[test]
fn test20_task_rows_come_from_the_run_store_in_the_data_dir() {
    let dir = Dir::new("store");
    let host = host(&dir, Vec::new());
    let definition = model::parse(
        &json!({
            "id": "export", "title": "Export notes", "enabled": true,
            "requires": {"servers": [], "commands": []}, "writesSecrets": [],
            "steps": [{"id": "ask", "title": "Confirm", "type": "needs-you",
                       "instructions": "confirm", "waitFor": {"kind": "manual", "timeoutSec": 900}}],
            "triggers": {"manual": true, "cli": true,
                "selfMcp": {"enabled": false, "approval": "every-run"},
                "schedule": null, "onAuthFailure": []}
        })
        .to_string(),
    )
    .unwrap();
    store::save_task(&dir.0, &definition).unwrap();
    let waiting = run(
        "run-fixture-w",
        "export",
        "waiting",
        NOW - 600,
        json!([step("Confirm", "waiting")]),
    );
    store::create_run(&dir.0, &waiting).unwrap();

    let items = super::snapshot(&host).items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Task Export notes is waiting for you");

    store::update_run(&dir.0, "run-fixture-w", |r| {
        r.status = store::Status::Ok;
    })
    .unwrap();
    assert!(super::snapshot(&host).items.is_empty());
}

#[test]
fn test21_the_snapshot_json_carries_rfc3339_times_and_the_action() {
    let dir = Dir::new("json");
    let host = host(&dir, vec![secret_server("alpha", true)]);
    let json = super::snapshot(&host).to_json();
    assert_eq!(json["checkedAt"], rfc3339(NOW));
    let row = &json["items"][0];
    assert_eq!(row["urgency"], "needs-you");
    assert_eq!(row["source"], "Credentials");
    assert_eq!(row["action"]["screen"], "servers");
    assert!(row.get("since").is_none());
}

#[test]
fn test22_a_login_row_comes_from_the_status_file_of_a_server_that_still_has_a_probe() {
    let dir = Dir::new("login");
    let mut probed = ServerRecord {
        id: "alpha".into(),
        name: "alpha".into(),
        enabled: true,
        ..Default::default()
    };
    probed.extra.insert("transport".into(), json!("stdio"));
    probed.extra.insert("command".into(), json!("/bin/true"));
    probed.fork.plus =
        Some(serde_json::from_value(json!({"authProbe": {"kind": "stdio"}})).unwrap());
    let host = host(&dir, vec![probed]);
    let status = json!({"version": 1, "servers": {
        "alpha": {"tracked": {"state": "needs_reauth", "reason": "the key was rejected",
                              "since": NOW - 3 * HOUR, "transient": null},
                  "lastProbeAt": NOW - 60, "nextDueAt": NOW + 600},
        "removed": {"tracked": {"state": "revoked", "reason": "gone",
                                "since": NOW - HOUR, "transient": null},
                    "lastProbeAt": NOW - 60, "nextDueAt": NOW + 600},
    }});
    std::fs::create_dir_all(dir.0.join("auth")).unwrap();
    std::fs::write(dir.0.join("auth/status.json"), status.to_string()).unwrap();

    let items = super::snapshot(&host).items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "credentials:login:alpha");
    assert_eq!(items[0].since, Some(NOW - 3 * HOUR));
}
