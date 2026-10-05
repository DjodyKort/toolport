use super::tests::{task, until, world, Fake, CANARY};
use super::store::{self, Status};
use super::{api, model};
use serde_json::{json, Value};

fn write_json(path: &std::path::Path, value: &Value) -> String {
    std::fs::write(path, value.to_string()).unwrap();
    path.display().to_string()
}

#[test]
fn ls_show_and_history_have_the_documented_shapes_and_hide_disabled_tasks_by_default() {
    let fx = world("api-ls");
    store::save_task(&task(json!({}))).unwrap();
    let mut draft = task(json!({"id": "draft", "enabled": false}));
    draft.enabled = false;
    store::save_task(&draft).unwrap();
    std::fs::write(store::tasks_dir().unwrap().join("broken.json"), "{").unwrap();
    let listed = api::ls(false).unwrap();
    assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(listed["tasks"][0]["id"], "moodle-token");
    assert_eq!(listed["tasks"][0]["lastRun"], Value::Null);
    assert_eq!(listed["tasks"][0]["waiting"], false);
    assert_eq!(listed["invalid"][0]["id"], "broken");
    assert_eq!(api::ls(true).unwrap()["tasks"].as_array().unwrap().len(), 2);
    let shown = api::show("moodle-token").unwrap();
    assert_eq!(shown["task"]["id"], "moodle-token");
    assert_eq!(shown["runs"], json!([]));
    assert_eq!(api::history(None, None, None).unwrap()["runs"], json!([]));
    assert_eq!(api::show("nope").unwrap_err().code(), "not_found");
    drop(fx);
}

#[test]
fn a_dry_run_lists_steps_secrets_and_servers_and_starts_nothing() {
    let _fx = world("api-dry");
    store::save_task(&task(json!({}))).unwrap();
    let out = api::run("moodle-token", "manual", true, Fake::shared()).unwrap();
    assert_eq!(out["dryRun"], true);
    assert_eq!(out["run"], Value::Null);
    let details: Vec<&str> = out["plan"]["steps"].as_array().unwrap().iter().map(|s| s["detail"].as_str().unwrap()).collect();
    assert!(details.iter().any(|d| d.contains("pause for you")), "{details:?}");
    assert!(details.iter().any(|d| d.contains("may write the secret acme/API_PASSWORD")), "{details:?}");
    assert!(details.iter().any(|d| d.contains("needs the server acme")), "{details:?}");
    assert_eq!(out["writesSecrets"][0]["key"], "API_PASSWORD");
    assert_eq!(out["requires"]["servers"][0], "acme");
    assert!(store::list_runs(None).unwrap().is_empty());
    let text = out.to_string();
    assert!(!text.contains(CANARY));
}

#[test]
fn resume_needs_a_waiting_run_and_cancel_marks_a_dead_runner_cancelled() {
    let _fx = world("api-resume");
    let t = task(json!({}));
    store::save_task(&t).unwrap();
    let run = store::new_run(&t, "manual");
    store::create_run(&run).unwrap();
    assert_eq!(api::resume(&run.id).unwrap_err().code(), "conflict");
    store::update_run(&run.id, |r| {
        r.status = Status::Waiting;
        r.steps[0].status = Status::Waiting;
        r.runner_pid = Some(4_194_000);
    })
    .unwrap();
    assert_eq!(api::resume(&run.id).unwrap_err().code(), "runner_gone");
    let out = api::cancel(&run.id).unwrap();
    assert_eq!(out["run"]["status"], "cancelled");
    assert_eq!(out["run"]["steps"][0]["status"], "cancelled");
    assert_eq!(out["run"]["steps"][2]["status"], "skipped");
    assert!(out["run"]["durationMs"].is_number());
    assert_eq!(api::cancel(&run.id).unwrap_err().code(), "conflict");
    assert!(out["run"].get("runnerPid").is_none() && out["run"].get("cancelRequested").is_none());
}

#[test]
fn a_run_through_the_api_pauses_resumes_and_leaks_nothing_into_the_views() {
    let _fx = world("api-run");
    store::save_task(&task(json!({}))).unwrap();
    let started = api::run("moodle-token", "manual", false, Fake::shared()).unwrap();
    let run_id = started["run"]["id"].as_str().unwrap().to_string();
    assert_eq!(started["result"]["applied"], true);
    assert_eq!(api::run("moodle-token", "manual", false, Fake::shared()).unwrap_err().code(), "conflict");
    until("waiting", || store::load_run(&run_id).unwrap().status == Status::Waiting);
    let listed = api::ls(false).unwrap();
    assert_eq!(listed["tasks"][0]["waiting"], true);
    let resumed = api::resume(&run_id).unwrap();
    assert_eq!(resumed["result"]["applied"], true);
    until("finished", || store::load_run(&run_id).unwrap().status.finished());
    let one = api::history(None, Some(&run_id), None).unwrap();
    assert_eq!(one["run"]["status"], "ok");
    assert!(one["run"]["steps"][1]["output"].as_str().unwrap().contains("[redacted]"));
    let everything = [one.to_string(), api::history(Some("moodle-token"), None, Some(3)).unwrap().to_string(), api::show("moodle-token").unwrap().to_string(), api::ls(true).unwrap().to_string(), started.to_string(), resumed.to_string()].join("\n");
    assert!(!everything.contains(CANARY), "{everything}");
    assert!(api::history(Some("other"), None, None).is_err());
}

#[test]
fn add_edit_and_rm_preview_first_and_apply_with_backups() {
    let fx = world("api-write");
    let file = write_json(&fx.dir.join("t.json"), &serde_json::to_value(task(json!({"id": "fresh"}))).unwrap());
    let dry = api::add("fresh", api::Source::File(&file), true).unwrap();
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["result"], Value::Null);
    assert!(!store::task_path("fresh").unwrap().exists());
    let warnings: Vec<&str> = dry["plan"]["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(warnings.iter().any(|w| w.contains("acme/API_PASSWORD")), "{warnings:?}");
    let applied = api::add("fresh", api::Source::File(&file), false).unwrap();
    assert_eq!(applied["result"]["applied"], true);
    assert_eq!(api::add("fresh", api::Source::File(&file), false).unwrap_err().code(), "conflict");
    assert_eq!(api::add("other", api::Source::File(&file), true).unwrap_err().code(), "usage");

    let mut changed = serde_json::to_value(task(json!({"id": "fresh"}))).unwrap();
    changed["title"] = json!("Changed title");
    let edit_file = write_json(&fx.dir.join("e.json"), &changed);
    let preview = api::edit("fresh", &edit_file, true).unwrap();
    assert_eq!(store::load_task("fresh").unwrap().title, "Refresh the token");
    assert!(preview["plan"]["steps"][0]["diff"]["after"].as_str().unwrap().contains("Changed title"));
    let done = api::edit("fresh", &edit_file, false).unwrap();
    assert_eq!(store::load_task("fresh").unwrap().title, "Changed title");
    let backup = done["result"]["backups"][0].as_str().unwrap();
    assert!(std::fs::read_to_string(backup).unwrap().contains("Refresh the token"));
    assert_eq!(api::edit("nope", &edit_file, true).unwrap_err().code(), "usage");

    changed["steps"][2]["key"] = json!("UNDECLARED");
    let bad = write_json(&fx.dir.join("bad.json"), &changed);
    let err = api::edit("fresh", &bad, true).unwrap_err();
    assert_eq!(err.code(), "invalid_task");
    assert!(err.message.contains("not listed in writesSecrets"), "{}", err.message);

    let plan = api::rm("fresh", true).unwrap();
    assert_eq!(plan["plan"]["steps"][0]["op"], "delete");
    assert!(store::task_path("fresh").unwrap().exists());
    let gone = api::rm("fresh", false).unwrap();
    assert!(!store::task_path("fresh").unwrap().exists());
    assert!(std::path::Path::new(gone["result"]["backups"][0].as_str().unwrap()).exists());
    assert_eq!(api::rm("fresh", false).unwrap_err().code(), "not_found");
}

#[test]
fn rm_refuses_a_task_with_an_active_run() {
    let _fx = world("api-rm-busy");
    let t = task(json!({}));
    store::save_task(&t).unwrap();
    let run = store::new_run(&t, "manual");
    store::create_run(&run).unwrap();
    assert_eq!(api::rm("moodle-token", false).unwrap_err().code(), "conflict");
}

#[test]
fn add_from_a_command_file_makes_a_disabled_draft() {
    let fx = world("api-command");
    let path = fx.dir.join("moodle-token.md");
    std::fs::write(&path, "---\ndescription: Refresh the token\n---\n# Refresh\n- Sign in to the portal\n- Log in again if asked\nCall mcp__toolport__acme__get and store it.\n").unwrap();
    let out = api::add("moodle-token", api::Source::Command(&path.display().to_string()), false).unwrap();
    let saved = store::load_task("moodle-token").unwrap();
    assert!(!saved.enabled);
    assert!(saved.has_needs_you());
    assert_eq!(saved.requires.servers, ["acme"]);
    assert_eq!(out["created"], true);
    assert!(out["plan"]["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("disabled")));
    assert_eq!(model::validate(&saved), Ok(()));
}

#[test]
fn tasks_run_asks_every_time_and_a_denial_runs_nothing() {
    let _fx = world("approval");
    let t = task(json!({}));
    store::save_task(&t).unwrap();
    let asked = std::sync::Mutex::new(Vec::<crate::approval::ApprovalRequest>::new());
    let deny = |r: crate::approval::ApprovalRequest| {
        asked.lock().unwrap().push(r);
        crate::approval::ApprovalDecision::Denied
    };
    for decision in [crate::approval::ApprovalDecision::Denied, crate::approval::ApprovalDecision::Timeout, crate::approval::ApprovalDecision::Unreachable, crate::approval::ApprovalDecision::StaleState] {
        let err = super::approval::run("moodle-token", false, Fake::shared(), &|r| {
            asked.lock().unwrap().push(r);
            decision
        })
        .unwrap_err();
        assert!(err.code().starts_with("approval_"), "{err:?}");
        assert!(err.message.contains("nothing was started"), "{}", err.message);
    }
    assert!(super::approval::run("moodle-token", false, Fake::shared(), &deny).is_err());
    assert!(store::list_runs(None).unwrap().is_empty());
    let asked = asked.into_inner().unwrap();
    assert_eq!(asked.len(), 5);
    let first = &asked[0];
    assert_eq!((first.server.as_str(), first.tool.as_str()), ("toolport", "tasks_run"));
    assert_eq!(first.arguments["task"], "moodle-token");
    assert!(first.arguments["steps"].as_array().unwrap().len() >= 3);
    assert_eq!(first.arguments["writesSecrets"][0]["key"], "API_PASSWORD");
    assert!(!first.arguments.to_string().contains(CANARY));
}

#[test]
fn an_approved_tasks_run_starts_once_and_never_returns_a_captured_value() {
    let _fx = world("approval-ok");
    store::save_task(&task(json!({}))).unwrap();
    let approve = |_r: crate::approval::ApprovalRequest| crate::approval::ApprovalDecision::Approved;
    let out = super::approval::run("moodle-token", false, Fake::shared(), &approve).unwrap();
    assert_eq!(out["run"]["trigger"], "selfMcp");
    let run_id = out["run"]["id"].as_str().unwrap().to_string();
    until("waiting", || store::load_run(&run_id).unwrap().status == Status::Waiting);
    api::resume(&run_id).unwrap();
    until("finished", || store::load_run(&run_id).unwrap().status.finished());
    let done = api::history(None, Some(&run_id), None).unwrap();
    assert_eq!(done["run"]["status"], "ok");
    assert!(!(out.to_string() + &done.to_string()).contains(CANARY));
}

#[test]
fn a_dry_run_or_a_task_that_refuses_self_mcp_never_asks() {
    let _fx = world("approval-dry");
    store::save_task(&task(json!({}))).unwrap();
    let mut off = serde_json::to_value(task(json!({"id": "closed"}))).unwrap();
    off["triggers"]["selfMcp"]["enabled"] = json!(false);
    store::save_task(&model::parse(&off.to_string()).unwrap()).unwrap();
    let never = |_r: crate::approval::ApprovalRequest| -> crate::approval::ApprovalDecision { panic!("must not ask") };
    let preview = super::approval::run("moodle-token", true, Fake::shared(), &never).unwrap();
    assert_eq!(preview["dryRun"], true);
    let err = super::approval::run("closed", false, Fake::shared(), &never).unwrap_err();
    assert_eq!(err.code(), "conflict");
    assert!(store::list_runs(None).unwrap().is_empty());
}
