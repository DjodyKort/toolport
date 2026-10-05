use super::host::Host;
use super::model::{self, Task};
use super::store::{self, Status};
use super::{runner, triggers};
use crate::plus::testutil::DataDirFx;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CANARY: &str = "CANARY-9f3a61c07b2d";

#[derive(Default)]
struct Fake {
    vault: Mutex<HashMap<(String, String), String>>,
}

impl Host for Fake {
    fn call_tool(&self, _s: &str, _t: &str, _a: Value) -> Result<Value, String> {
        Ok(json!({"content": [{"type": "text", "text": format!("{{\"pw\":\"{CANARY}\",\"note\":\"logged in with {CANARY}\"}}")}]}))
    }
    fn run_routine(&self, _: Option<&str>, _: Option<&str>, _: Value) -> Result<Value, String> {
        Err("no routines".into())
    }
    fn set_secret(&self, server: &str, key: &str, value: &str) -> Result<(), String> {
        self.vault.lock().unwrap().insert((server.into(), key.into()), value.into());
        Ok(())
    }
    fn secret_is_set(&self, server: &str, key: &str) -> bool {
        self.vault.lock().unwrap().contains_key(&(server.to_string(), key.to_string()))
    }
    fn restart_server(&self, s: &str) -> Result<String, String> {
        Ok(format!("restarted {s}"))
    }
    fn claude_program(&self) -> String {
        "claude".into()
    }
    fn spawn_runner(&self, run_id: &str) -> Result<(), String> {
        let id = run_id.to_string();
        std::thread::spawn(move || {
            let _ = runner::execute(&id, Fake::shared());
        });
        Ok(())
    }
}

static SHARED: std::sync::OnceLock<Fake> = std::sync::OnceLock::new();

impl Fake {
    fn shared() -> &'static Fake {
        SHARED.get_or_init(Fake::default)
    }
}

fn task(extra: Value) -> Task {
    let mut base = json!({
        "id": "moodle-token", "title": "Refresh the token", "description": "", "enabled": true,
        "requires": {"servers": ["acme"], "commands": ["sleep"]},
        "writesSecrets": [{"server": "acme", "key": "API_PASSWORD"}],
        "steps": [
            {"id": "sign-in", "type": "needs-you", "title": "Sign in", "instructions": "Sign in in the browser window"},
            {"id": "read", "type": "mcp", "title": "Read", "server": "acme", "tool": "get", "args": {}, "capture": ["pw"]},
            {"id": "store", "type": "secret-set", "title": "Store", "server": "acme", "key": "API_PASSWORD", "from": "pw"}
        ],
        "triggers": {"manual": true, "cli": true, "selfMcp": {"enabled": true, "approval": "every-run"}, "schedule": null, "onAuthFailure": []}
    });
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    let t = model::parse(&base.to_string()).unwrap();
    model::validate(&t).unwrap();
    t
}

fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !ok() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn world(tag: &str) -> DataDirFx {
    std::env::set_var("TOOLPORT_TASK_POLL_MS", "20");
    DataDirFx::new("tasks", tag).with_secret_key("synthetic-test-key-0123456789")
}

#[test]
fn a_needs_you_step_pauses_shows_its_instructions_resumes_and_finishes_without_leaking() {
    let _fx = world("pause");
    let t = task(json!({}));
    store::save_task(&t).unwrap();
    let run = triggers::start(&t, "manual", Fake::shared()).unwrap();
    until("waiting", || store::load_run(&run.id).unwrap().status == Status::Waiting);
    let waiting = store::load_run(&run.id).unwrap();
    assert_eq!(waiting.steps[0].instructions.as_deref(), Some("Sign in in the browser window"));
    store::update_run(&run.id, |r| r.resume_requested = true).unwrap();
    until("finished", || store::load_run(&run.id).unwrap().status.finished());
    let done = store::load_run(&run.id).unwrap();
    assert_eq!(done.status, Status::Ok, "{done:?}");
    assert_eq!(Fake::shared().vault.lock().unwrap()[&("acme".into(), "API_PASSWORD".into())], CANARY);
    let everything = serde_json::to_string(&done).unwrap() + &store::attention().len().to_string();
    assert!(!everything.contains(CANARY), "{everything}");
    assert!(done.steps[1].output.contains("[redacted]"));
}

#[test]
fn an_undeclared_key_is_refused_at_validation_and_at_run_time() {
    let _fx = world("undeclared");
    let mut bad = serde_json::to_value(task(json!({}))).unwrap();
    bad["steps"][2]["key"] = json!("OTHER");
    let err = model::validate(&model::parse(&bad.to_string()).unwrap()).unwrap_err();
    assert!(err.contains("acme/OTHER is not listed in writesSecrets"), "{err}");
    let t = model::parse(&bad.to_string()).unwrap();
    std::fs::create_dir_all(store::tasks_dir().unwrap()).unwrap();
    store::write(&store::task_path("moodle-token").unwrap(), &bad.to_string()).unwrap();
    let mut no_pause = t.clone();
    no_pause.steps.remove(0);
    store::save_task(&no_pause).unwrap();
    let run = store::new_run(&no_pause, "manual");
    store::create_run(&run).unwrap();
    let done = runner::execute(&run.id, Fake::shared()).unwrap();
    assert_eq!(done.status, Status::Failed);
    assert!(done.steps[1].output.contains("not listed in writesSecrets"));
    assert!(!Fake::shared().secret_is_set("acme", "OTHER"));
}

#[cfg(unix)]
#[test]
fn cancel_kills_the_child_process() {
    let _fx = world("cancel");
    let t = task(json!({"steps": [{"id": "wait", "type": "exec", "title": "Wait", "program": "sleep", "args": ["30"]}], "writesSecrets": []}));
    store::save_task(&t).unwrap();
    let run = triggers::start(&t, "manual", Fake::shared()).unwrap();
    until("a child", || store::load_run(&run.id).unwrap().child_pid.is_some());
    let pid = store::load_run(&run.id).unwrap().child_pid.unwrap();
    store::update_run(&run.id, |r| r.cancel_requested = true).unwrap();
    until("cancelled", || store::load_run(&run.id).unwrap().status == Status::Cancelled);
    until("the child to be gone", || unsafe { libc::kill(pid as i32, 0) != 0 });
}

#[test]
fn schedule_and_auth_failure_never_start_a_task_with_a_needs_you_step() {
    let _fx = world("triggers");
    let sched = json!({"cron": "* * * * *", "autoRun": false});
    let t = task(json!({"triggers": {"manual": true, "cli": false, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": sched, "onAuthFailure": ["acme"]}}));
    store::save_task(&t).unwrap();
    let raised = triggers::on_auth_failure("acme", Fake::shared());
    assert_eq!(raised, vec![triggers::Raised::Attention("moodle-token".into())]);
    let due = triggers::run_due(super::cron::now(), Fake::shared());
    assert_eq!(due, vec![triggers::Raised::Attention("moodle-token".into())]);
    assert!(store::list_runs(None).unwrap().is_empty());
    let ids: Vec<String> = store::attention().into_iter().map(|a| a.id).collect();
    assert_eq!(ids, ["tasks:moodle-token:auth:acme", "tasks:moodle-token:due"]);
    let mut auto = serde_json::to_value(&t).unwrap();
    auto["triggers"]["schedule"]["autoRun"] = json!(true);
    assert!(model::validate(&model::parse(&auto.to_string()).unwrap()).unwrap_err().contains("autoRun is not allowed"));
}
