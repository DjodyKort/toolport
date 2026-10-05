use super::host::Host;
use super::model::{self, Step};
use super::store::{self, Status};
use super::tests::world;
use super::{api, builtin, runner, triggers};
use serde_json::{json, Value};

struct NoPlaywright;

impl Host for NoPlaywright {
    fn call_tool(&self, _: &str, _: &str, _: Value) -> Result<Value, String> {
        Err("no tools".into())
    }
    fn run_routine(&self, _: Option<&str>, _: Option<&str>, _: Value) -> Result<Value, String> {
        Err("no routines".into())
    }
    fn set_secret(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Err("no vault".into())
    }
    fn secret_is_set(&self, _: &str, _: &str) -> bool {
        false
    }
    fn restart_server(&self, _: &str) -> Result<String, String> {
        Err("no servers".into())
    }
    fn claude_program(&self) -> String {
        "claude".into()
    }
    fn spawn_runner(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn server_installed(&self, server: &str) -> bool {
        server != "playwright"
    }
}

fn shipped() -> model::Task {
    let text = builtin::unclaimed_text("moodle-token").expect("moodle-token ships");
    model::parse(text).unwrap()
}

fn enabled_copy(dir: &std::path::Path) -> String {
    let mut value = serde_json::to_value(shipped()).unwrap();
    value["enabled"] = json!(true);
    let path = dir.join("moodle-token.edit.json");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(&path, value.to_string()).unwrap();
    path.display().to_string()
}

#[test]
fn the_shipped_moodle_token_task_is_valid_disabled_and_never_started_on_its_own() {
    let task = shipped();
    model::validate(&task).unwrap();
    assert!(!task.enabled);
    assert_eq!(task.requires.servers, ["playwright", "moodle"]);
    assert_eq!(task.writes_secrets.len(), 1);
    assert!(task.declares("moodle", "MOODLE_TOKEN"));
    let kinds: Vec<&str> = task.steps.iter().map(Step::kind).collect();
    assert_eq!(kinds, ["needs-you", "routine", "secret-set", "restart-server"]);
    assert!(matches!(&task.steps[3], Step::RestartServer { server, .. } if server == "moodle"));
    assert!(task.has_needs_you() && !triggers::auto_runs(&task));
    assert!(triggers::allowed(&task, "manual").is_err(), "a disabled task must not run");
    assert!(matches!(&task.steps[1], Step::Routine { script: Some(s), .. } if s.contains("<set on the Mac, runbook G5>")));
}

#[test]
fn a_shipped_task_is_listed_without_writing_and_copied_on_the_first_change() {
    let _fx = world("builtin-list");
    assert!(api::ls(false).unwrap()["tasks"].as_array().unwrap().is_empty(), "disabled tasks stay hidden");
    let all = api::ls(true).unwrap();
    assert_eq!(all["tasks"][0]["id"], "moodle-token");
    assert_eq!(all["tasks"][0]["enabled"], false);
    assert_eq!(api::show("moodle-token").unwrap()["task"]["id"], "moodle-token");
    assert!(!store::tasks_dir().unwrap().exists(), "listing and showing must not write");

    let file = enabled_copy(&store::plus_dir().unwrap().join("edits"));
    let edited = api::edit("moodle-token", &file, false).unwrap();
    assert_eq!(edited["created"], false);
    assert!(store::task_path("moodle-token").unwrap().exists());
    assert!(store::load_task("moodle-token").unwrap().enabled);
    assert!(builtin::unclaimed_text("moodle-token").is_none());

    api::rm("moodle-token", false).unwrap();
    assert!(api::ls(true).unwrap()["tasks"].as_array().unwrap().is_empty(), "a removed shipped task stays removed");
    assert!(store::load_task("moodle-token").is_err());
}

#[test]
fn a_missing_playwright_server_stops_the_run_at_the_first_step_with_the_install_action() {
    let _fx = world("builtin-missing");
    api::edit("moodle-token", &enabled_copy(&store::plus_dir().unwrap().join("edits")), false).unwrap();
    let task = store::load_task("moodle-token").unwrap();
    let run = store::new_run(&task, "manual");
    store::create_run(&run).unwrap();
    runner::execute(&run.id, &NoPlaywright).unwrap();
    let done = store::load_run(&run.id).unwrap();
    assert_eq!(done.status, Status::Failed);
    assert_eq!(done.steps[0].status, Status::Failed);
    assert!(done.steps[0].output.contains("install the Playwright server"), "{}", done.steps[0].output);
    assert!(done.steps[1..].iter().all(|s| s.status == Status::Skipped));
    assert!(done.error.as_deref().is_some_and(|e| e.contains("install the Playwright server")));
    let view = api::run_view(&done, true);
    assert_eq!(view["action"]["command"], json!(["toolportctl", "server", "install", "Playwright"]));
    assert_eq!(view["action"]["label"], "Install Playwright");
}
