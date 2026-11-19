use serde_json::{json, Value};
use toolport_plus_api::RunDecision;

use super::{call_tool, ToolError};
use crate::setup::tests::World;

const TRANSCRIPT: &str = "TRANSCRIPT-text-that-must-not-leave-the-disk-4471";

fn world() -> World {
    let world = World::new();
    let web = world.repo("web");
    world.session_of(&web, "abc", &format!("{TRANSCRIPT}\n"));
    world.repo("docs");
    world
}

fn export(world: &World, extra: Value) -> Result<Value, ToolError> {
    let mut args = json!({"roots": [world.home.join("code")], "sessions": true});
    args.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    call_tool(&world.host, "setup_export", &args)
}

#[test]
fn test01_sessions_list_repositories_with_sizes_after_approval_and_never_their_text() {
    let world = world();
    world.host.tasks.decision.set(RunDecision::Approved);
    let data = export(&world, json!({})).unwrap();
    let history = data["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["id"] == "history")
        .unwrap();
    assert_eq!(history["count"], 1);
    let repos = history["info"]["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["name"], "web");
    assert!(repos[0]["fileSize"].as_u64().unwrap() > 0);
    assert!(!data.to_string().contains(TRANSCRIPT));
    let approvals = world.host.tasks.approvals.borrow();
    assert_eq!(approvals.len(), 1);
    let shown = format!("{approvals:?}");
    assert!(shown.contains("web: 1 conversations"));
    assert!(!shown.contains(TRANSCRIPT));
}

#[test]
fn test02_sessions_without_approval_return_nothing_about_the_repositories() {
    for (decision, kind) in [
        (RunDecision::Unreachable, "approval_unavailable"),
        (RunDecision::Denied, "approval_denied"),
        (RunDecision::Timeout, "approval_denied"),
    ] {
        let world = world();
        world.host.tasks.decision.set(decision);
        let error = export(&world, json!({})).unwrap_err();
        assert_eq!(error.kind, kind);
        assert!(!error.message.contains("web"));
    }
}

#[test]
fn test03_every_sessions_call_asks_again() {
    let world = world();
    world.host.tasks.decision.set(RunDecision::Approved);
    export(&world, json!({})).unwrap();
    export(&world, json!({"session_repo": ["web"]})).unwrap();
    assert_eq!(world.host.tasks.approvals.borrow().len(), 2);
}

#[test]
fn test04_sessions_never_write_a_file() {
    let world = world();
    world.host.tasks.decision.set(RunDecision::Approved);
    let out = world.out_dir().join("a.zip");
    let before = world.snapshot();
    for extra in [
        json!({"dry_run": false, "confirm": true, "out": out}),
        json!({"dry_run": false, "confirm": true}),
    ] {
        assert_eq!(export(&world, extra).unwrap_err().kind, "refused");
    }
    assert!(world.host.tasks.approvals.borrow().is_empty());
    world.assert_unchanged(&before);
}

#[test]
fn test05_session_repo_narrows_the_listing_and_needs_sessions() {
    let world = world();
    world.host.tasks.decision.set(RunDecision::Approved);
    let data = export(&world, json!({"session_repo": ["docs"]})).unwrap();
    let history = data["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["id"] == "history")
        .unwrap();
    assert_eq!(history["info"]["repos"], json!([]));
    let error = call_tool(
        &world.host,
        "setup_export",
        &json!({"session_repo": ["web"]}),
    )
    .unwrap_err();
    assert_eq!(error.kind, "invalid_arguments");
}

#[test]
fn test06_the_parts_argument_still_cannot_name_sessions_or_secrets() {
    let world = world();
    world.host.tasks.decision.set(RunDecision::Approved);
    for parts in ["history", "all,secrets"] {
        let error = call_tool(
            &world.host,
            "setup_export",
            &json!({"parts": parts, "roots": [world.home.join("code")]}),
        )
        .unwrap_err();
        assert_eq!(error.kind, "refused", "{parts}");
    }
    assert!(world.host.tasks.approvals.borrow().is_empty());
}

#[test]
fn test07_without_sessions_no_approval_is_asked() {
    let world = world();
    let data = call_tool(
        &world.host,
        "setup_export",
        &json!({"roots": [world.home.join("code")]}),
    )
    .unwrap();
    assert_eq!(data["dryRun"], true);
    assert!(world.host.tasks.approvals.borrow().is_empty());
}
