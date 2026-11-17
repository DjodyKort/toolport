use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};
use toolport_plus_api::{FakeTaskHost, ProcessOutcome, RunDecision, TaskHost};

use super::cron::days_from_civil;
use super::model::{self, Task};
use super::schedule::{self, MAX_CATCH_UPS_PER_TICK};
use super::store::{self, Status};
use super::triggers::Raised;
use super::{api, approval, runner};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "plus-tasks-{tag}-{}-{}",
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

fn at(day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    days_from_civil(2026, 11, day) * 86400 + hour * 3600 + minute * 60 + second
}

fn task(id: &str, cron: &str, auto_run: bool, steps: Value) -> Task {
    model::parse(
        &json!({
            "id": id, "title": id, "enabled": true,
            "requires": {"servers": ["srv-alpha"], "commands": ["echo"]},
            "writesSecrets": [{"server": "srv-alpha", "key": "API_TOKEN"}],
            "steps": steps,
            "triggers": {"manual": true, "cli": true,
                "selfMcp": {"enabled": true, "approval": "every-run"},
                "schedule": {"cron": cron, "autoRun": auto_run}, "onAuthFailure": []}
        })
        .to_string(),
    )
    .unwrap()
}

fn echo_steps() -> Value {
    json!([{"id": "say", "title": "Say", "type": "exec", "program": "echo", "args": ["hi"]}])
}

fn fixture(tag: &str, now: i64) -> (Dir, FakeTaskHost) {
    let dir = Dir::new(tag);
    let host = FakeTaskHost::at(dir.0.clone(), now);
    (dir, host)
}

fn save(host: &FakeTaskHost, task: &Task) {
    store::save_task(&host.tasks_base().unwrap(), task).unwrap();
}

fn runs(host: &FakeTaskHost, id: &str) -> Vec<store::Run> {
    store::list_runs(&host.tasks_base().unwrap(), Some(id))
}

#[test]
fn test01_a_due_task_starts_from_a_tick_with_no_client_open() {
    let (_dir, host) = fixture("due", at(17, 8, 6, 30));
    save(
        &host,
        &task("plugins-update", "7 */4 * * *", true, echo_steps()),
    );
    assert!(
        schedule::tick(&host).raised.is_empty(),
        "first sight is not back-filled"
    );

    host.clock.set(at(17, 8, 7, 10));
    let report = schedule::tick(&host);
    assert!(
        matches!(report.raised.as_slice(), [Raised::Started(_)]),
        "{report:?}"
    );
    assert!(report.missed.is_empty());
    let started = runs(&host, "plugins-update");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].trigger, "schedule");
    assert_eq!(started[0].missed, None);
    assert_eq!(*host.spawned.borrow(), [started[0].id.clone()]);
}

#[test]
fn test02_the_app_tick_and_the_daemon_tick_in_one_minute_start_one_run() {
    let (_dir, host) = fixture("single", at(17, 7, 6, 0));
    save(
        &host,
        &task("plugins-update", "7 * * * *", true, echo_steps()),
    );
    schedule::tick(&host);
    host.clock.set(at(17, 7, 7, 5));
    schedule::tick(&host);
    host.advance(20);
    schedule::tick(&host);
    host.advance(30);
    schedule::tick(&host);
    assert_eq!(runs(&host, "plugins-update").len(), 1);
    assert_eq!(host.spawned.borrow().len(), 1);
}

#[test]
fn test03_a_run_missed_while_nothing_ticked_is_caught_up_once_and_recorded() {
    let (dir, host) = fixture("catch-up", at(17, 7, 8, 0));
    save(
        &host,
        &task("plugins-update", "7 */4 * * *", true, echo_steps()),
    );
    schedule::tick(&host);

    host.clock.set(at(17, 20, 30, 0) + 0);
    let report = schedule::tick(&host);
    assert!(
        matches!(report.raised.as_slice(), [Raised::Started(_)]),
        "{report:?}"
    );
    let run = &runs(&host, "plugins-update")[0];
    assert_eq!(run.trigger, "catch-up");
    let missed = run.missed.clone().unwrap();
    assert_eq!(missed.slots, 4, "08:07, 12:07, 16:07 and 20:07 passed");
    assert_eq!(missed.first_due, "2026-11-17T08:07:00Z");
    assert_eq!(missed.last_due, "2026-11-17T20:07:00Z");

    let ledger = schedule::read_ledger(&dir.0);
    assert_eq!(ledger.missed.len(), 1);
    assert_eq!(ledger.missed[0].outcome, schedule::OUTCOME_CAUGHT_UP);
    assert_eq!(ledger.missed[0].run.as_deref(), Some(run.id.as_str()));

    store::update_run(&dir.0, &run.id.clone(), |r| r.status = Status::Ok).unwrap();
    host.advance(60);
    schedule::tick(&host);
    assert_eq!(
        runs(&host, "plugins-update").len(),
        1,
        "the slots are handled once"
    );
}

#[test]
fn test04_a_first_tick_after_a_gap_uses_the_last_scheduled_run_as_its_start() {
    let (dir, host) = fixture("seed", at(17, 12, 30, 0));
    let t = task("plugins-update", "7 */4 * * *", true, echo_steps());
    save(&host, &t);
    let mut old = store::new_run(&t, "schedule", at(17, 8, 7, 3));
    old.status = Status::Ok;
    store::create_run(&dir.0, &old).unwrap();
    let report = schedule::tick(&host);
    assert!(
        matches!(report.raised.as_slice(), [Raised::Started(_)]),
        "{report:?}"
    );
    assert_eq!(runs(&host, "plugins-update")[0].trigger, "catch-up");
}

#[test]
fn test05_at_most_three_catch_ups_start_per_tick_and_the_rest_follow() {
    let (_dir, host) = fixture("cap", at(17, 0, 0, 0));
    for n in 0..5 {
        save(
            &host,
            &task(&format!("job-{n}"), "30 * * * *", true, echo_steps()),
        );
    }
    schedule::tick(&host);
    host.clock.set(at(17, 5, 0, 0));
    let first = schedule::tick(&host);
    assert_eq!(first.raised.len(), MAX_CATCH_UPS_PER_TICK);
    host.advance(60);
    let second = schedule::tick(&host);
    assert_eq!(second.raised.len(), 5 - MAX_CATCH_UPS_PER_TICK);
    host.advance(60);
    assert!(schedule::tick(&host).raised.is_empty());
}

#[test]
fn test06_slots_older_than_the_window_are_recorded_and_not_run() {
    let (dir, host) = fixture("too-old", at(10, 12, 0, 0));
    save(&host, &task("weekly", "0 9 * * 1", true, echo_steps()));
    schedule::tick(&host);
    host.clock.set(at(18, 12, 0, 0));
    let report = schedule::tick(&host);
    assert!(report.raised.is_empty());
    assert!(runs(&host, "weekly").is_empty());
    let ledger = schedule::read_ledger(&dir.0);
    assert_eq!(ledger.missed.len(), 1);
    assert_eq!(ledger.missed[0].outcome, schedule::OUTCOME_TOO_OLD);
    assert_eq!(ledger.missed[0].slots, 1);
}

#[test]
fn test07_a_task_that_needs_you_is_never_started_and_raises_attention_once() {
    let (dir, host) = fixture("needs-you", at(17, 7, 6, 0));
    let steps = json!([
        {"id": "sign-in", "title": "Sign in", "type": "needs-you", "instructions": "Sign in"},
        {"id": "say", "title": "Say", "type": "exec", "program": "echo", "args": ["x"]}
    ]);
    save(&host, &task("login", "7 * * * *", false, steps));
    schedule::tick(&host);
    host.clock.set(at(17, 7, 7, 5));
    let report = schedule::tick(&host);
    assert_eq!(report.raised, [Raised::Attention("login".into())]);
    assert!(host.spawned.borrow().is_empty());
    host.clock.set(at(17, 8, 7, 5));
    schedule::tick(&host);
    let items = store::attention(&dir.0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "tasks:login:due");
}

#[test]
fn test08_a_slot_that_meets_a_live_run_is_recorded_busy_and_a_manual_start_refuses() {
    let (dir, host) = fixture("busy", at(17, 7, 6, 0));
    let t = task("plugins-update", "7 * * * *", true, echo_steps());
    save(&host, &t);
    schedule::tick(&host);
    host.clock.set(at(17, 7, 7, 5));
    schedule::tick(&host);
    host.clock.set(at(17, 8, 7, 5));
    let report = schedule::tick(&host);
    assert!(report.raised.is_empty());
    assert_eq!(report.missed[0].outcome, schedule::OUTCOME_BUSY);
    assert_eq!(runs(&host, "plugins-update").len(), 1);

    let error = api::run("plugins-update", "manual", false, &host).unwrap_err();
    assert!(
        error.message.contains("already running"),
        "{}",
        error.message
    );
    assert_eq!(schedule::read_ledger(&dir.0).missed.len(), 1);
}

#[test]
fn test09_a_failing_spawn_marks_the_run_failed_and_the_slot_stays_handled() {
    let (_dir, host) = fixture("spawn", at(17, 7, 6, 0));
    save(
        &host,
        &task("plugins-update", "7 * * * *", true, echo_steps()),
    );
    schedule::tick(&host);
    *host.spawn_error.borrow_mut() = Some("no runner binary".into());
    host.clock.set(at(17, 7, 7, 5));
    schedule::tick(&host);
    let run = &runs(&host, "plugins-update")[0];
    assert_eq!(run.status, Status::Failed);
    assert_eq!(run.error.as_deref(), Some("no runner binary"));
}

#[test]
fn test10_a_stored_routine_by_id_is_refused_and_an_inline_script_keeps_its_stored_type() {
    let by_id = json!({
        "id": "old", "title": "Old",
        "steps": [{"id": "r", "title": "R", "type": "routine", "routineId": "daily"}]
    });
    let error = model::parse(&by_id.to_string()).unwrap_err();
    assert!(error.contains("saved routine"), "{error}");

    let inline = json!({
        "id": "inline", "title": "Inline", "enabled": true,
        "requires": {"servers": ["srv-alpha"], "commands": []},
        "steps": [{"id": "r", "title": "R", "type": "routine", "script": "return 1;", "capture": ["x"]}]
    });
    let parsed = model::parse(&inline.to_string()).unwrap();
    assert_eq!(parsed.steps[0].kind(), "script");
    let written = serde_json::to_value(&parsed).unwrap();
    assert_eq!(written["steps"][0]["type"], "routine");
    model::validate(&parsed).unwrap();
}

fn start_manual(host: &FakeTaskHost, t: &Task) -> String {
    save(host, t);
    let value = api::run(&t.id, "manual", false, host).unwrap();
    value["run"]["id"].as_str().unwrap().to_string()
}

#[test]
fn test11_the_runner_captures_a_value_into_the_vault_and_no_record_holds_it() {
    const VALUE: &str = "synthetic-token-value-1234567890";
    let (dir, host) = fixture("runner", at(17, 9, 0, 0));
    let steps = json!([
        {"id": "read", "title": "Read", "type": "mcp", "server": "srv-alpha", "tool": "get", "args": {}, "capture": ["token"]},
        {"id": "store", "title": "Store", "type": "secret-set", "server": "srv-alpha", "key": "API_TOKEN", "from": "token"},
        {"id": "say", "title": "Say", "type": "exec", "program": "echo", "args": ["hi"]},
        {"id": "restart", "title": "Restart", "type": "restart-server", "server": "srv-alpha"}
    ]);
    let t = task("portal", "7 * * * *", true, steps);
    let run_id = start_manual(&host, &t);
    host.tool_results.borrow_mut().push_back(Ok(
        json!({"content": [{"type": "text", "text": json!({"token": VALUE}).to_string()}]}),
    ));
    host.process_results
        .borrow_mut()
        .push_back(ProcessOutcome::Exited {
            code: Some(0),
            output: format!("echoed {VALUE}\n"),
        });
    let done = runner::execute(&host, &dir.0, &run_id).unwrap();
    assert_eq!(done.status, Status::Ok, "{done:?}");
    assert_eq!(
        host.secrets
            .borrow()
            .get(&("srv-alpha".into(), "API_TOKEN".into()))
            .map(String::as_str),
        Some(VALUE)
    );
    assert_eq!(*host.restarts.borrow(), ["srv-alpha"]);
    let record = std::fs::read_to_string(store::run_path(&dir.0, &run_id).unwrap()).unwrap();
    assert!(!record.contains(VALUE), "{record}");
    assert!(record.contains("[redacted]"));
}

#[test]
fn test12_a_failed_step_skips_the_rest_and_nothing_real_runs() {
    let (dir, host) = fixture("failed", at(17, 9, 0, 0));
    let steps = json!([
        {"id": "say", "title": "Say", "type": "exec", "program": "echo", "args": ["hi"]},
        {"id": "ask", "title": "Ask", "type": "prompt", "prompt": "Tidy"}
    ]);
    let run_id = start_manual(&host, &task("job", "7 * * * *", true, steps));
    host.process_results
        .borrow_mut()
        .push_back(ProcessOutcome::Exited {
            code: Some(3),
            output: "boom".into(),
        });
    let done = runner::execute(&host, &dir.0, &run_id).unwrap();
    assert_eq!(done.status, Status::Failed);
    assert_eq!(done.steps[1].status, Status::Skipped);
    assert!(done.steps[0].output.contains("exited with 3"));
    assert_eq!(
        host.processes.borrow().len(),
        1,
        "the prompt step never started"
    );
}

#[test]
fn test13_a_missing_server_fails_the_first_step_with_an_install_action() {
    let (dir, host) = fixture("missing", at(17, 9, 0, 0));
    *host.installed.borrow_mut() = Some(Vec::new());
    let steps = json!([{"id": "read", "title": "Read", "type": "mcp", "server": "srv-alpha", "tool": "get", "args": {}}]);
    let t = task("needs-server", "7 * * * *", true, steps);
    save(&host, &t);
    let run_id = api::run("needs-server", "manual", false, &host).unwrap()["run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let done = runner::execute(&host, &dir.0, &run_id).unwrap();
    assert_eq!(done.status, Status::Failed);
    assert_eq!(done.action.unwrap()["command"][2], "install");
    assert!(host.tool_calls.borrow().is_empty());
}

#[test]
fn test14_a_waiting_run_resumes_and_a_cancel_stops_the_runner() {
    let (dir, host) = fixture("wait", at(17, 9, 0, 0));
    let steps = json!([
        {"id": "sign-in", "title": "Sign in", "type": "needs-you", "instructions": "Sign in first"},
        {"id": "say", "title": "Say", "type": "exec", "program": "echo", "args": ["hi"]}
    ]);
    let t = task("login", "7 * * * *", false, steps);
    let first = start_manual(&host, &t);
    let base = dir.0.clone();
    let id = first.clone();
    let waiter = std::thread::spawn(move || {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while store::load_run(&base, &id).map(|r| r.status) != Ok(Status::Waiting) {
            assert!(std::time::Instant::now() < end, "the run never waited");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        store::update_run(&base, &id, |r| r.resume_requested = true).unwrap();
    });
    let done = runner::execute(&host, &dir.0, &first).unwrap();
    waiter.join().unwrap();
    assert_eq!(done.status, Status::Ok);

    let second = api::run("login", "manual", false, &host).unwrap()["run"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    store::update_run(&dir.0, &second, |r| r.cancel_requested = true).unwrap();
    let cancelled = runner::execute(&host, &dir.0, &second).unwrap();
    assert_eq!(cancelled.status, Status::Cancelled);
    assert_eq!(cancelled.steps[0].status, Status::Skipped);
}

#[test]
fn test15_a_self_mcp_run_starts_only_after_the_person_approves() {
    let (_dir, host) = fixture("approval", at(17, 9, 0, 0));
    save(&host, &task("login", "7 * * * *", true, echo_steps()));
    let error = approval::run("login", false, &host).unwrap_err();
    assert_eq!(error.code(), "approval_unavailable");
    host.decision.set(RunDecision::Denied);
    assert_eq!(
        approval::run("login", false, &host).unwrap_err().code(),
        "approval_denied"
    );
    assert!(runs(&host, "login").is_empty());
    assert_eq!(host.approvals.borrow().len(), 2);

    let dry = approval::run("login", true, &host).unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(host.approvals.borrow().len(), 2, "a dry run asks nobody");

    host.decision.set(RunDecision::Approved);
    let started = approval::run("login", false, &host).unwrap();
    assert_eq!(started["run"]["trigger"], "selfMcp");
    assert_eq!(host.spawned.borrow().len(), 1);
}

#[test]
fn test16_the_scheduler_switch_turns_every_tick_into_a_no_op() {
    let (_dir, host) = fixture("switch", at(17, 7, 6, 0));
    save(
        &host,
        &task("plugins-update", "7 * * * *", true, echo_steps()),
    );
    std::env::set_var("TOOLPORT_TASKS_SCHEDULER", "off");
    schedule::tick(&host);
    host.clock.set(at(17, 7, 7, 5));
    let report = schedule::tick(&host);
    std::env::remove_var("TOOLPORT_TASKS_SCHEDULER");
    assert!(report.raised.is_empty());
    assert!(runs(&host, "plugins-update").is_empty());
}
