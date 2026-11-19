use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use toolport_plus_api::RunDecision;

use super::plugins::{config_with, mcp_with, supplied_values};
use super::ToolError;
use crate::plugins::ClaudeRunner;
use crate::setup::tests::World;
use crate::skills::gitexec::CmdOutput;

const ID: &str = "ecc@ecc";
const VALUE: &str = "SENTINEL-knob-value-3c9e";

/// A `claude` that is not there: nothing in these tests may start the real one.
struct NoClaude;

impl ClaudeRunner for NoClaude {
    fn run(&self, _: &[&str], _: Duration) -> Result<CmdOutput, String> {
        Err(crate::plugins::claude::NOT_FOUND.to_string())
    }

    fn present(&self) -> bool {
        false
    }
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

struct Case {
    world: World,
    cwd: PathBuf,
}

fn case() -> Case {
    let world = World::new();
    let claude = world.home.join(".claude");
    let plugin = claude.join("plugins/cache/ecc");
    put(
        &claude.join("plugins/installed_plugins.json"),
        &json!({"version": 2, "plugins": {ID: [{"scope": "user", "installPath": plugin}]}})
            .to_string(),
    );
    put(
        &plugin.join(".mcp.json"),
        &json!({"mcpServers": {"chrome": {"command": "chrome-mcp"}}}).to_string(),
    );
    put(
        &claude.join("settings.json"),
        r#"{"enabledPlugins": {"ecc@ecc": true}}"#,
    );
    let cwd = world.dir.path().join("proj");
    std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
    Case { world, cwd }
}

impl Case {
    fn settings(&self) -> PathBuf {
        self.cwd.join(".claude/settings.local.json")
    }

    fn config(&self, extra: Value) -> Result<Value, ToolError> {
        let mut args = json!({"id": ID, "cwd": self.cwd});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        config_with(&self.world.host, &args, &NoClaude)
    }

    fn mcp(&self, extra: Value) -> Result<Value, ToolError> {
        let mut args = json!({"id": ID, "cwd": self.cwd});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        mcp_with(&self.world.host, &args, &NoClaude)
    }

    fn decide(&self, decision: RunDecision) {
        self.world.host.tasks.decision.set(decision);
    }

    fn asked(&self) -> String {
        format!("{:?}", self.world.host.tasks.approvals.borrow())
    }
}

#[test]
fn test01_without_set_the_tool_reads_and_asks_nobody() {
    let case = case();
    let data = case.config(json!({})).unwrap();
    assert_eq!(data["id"], ID);
    assert!(data["knobs"].as_array().is_some_and(|k| !k.is_empty()));
    assert!(case.world.host.tasks.approvals.borrow().is_empty());
}

#[test]
fn test02_a_set_previews_by_default_writes_nothing_and_returns_no_value() {
    let case = case();
    let data = case
        .config(json!({"set": {"hook_profile": VALUE}}))
        .unwrap_err();
    assert_eq!(data.kind, "invalid_arguments");
    assert!(!data.message.contains(VALUE), "{}", data.message);
    let preview = case
        .config(json!({"set": {"hook_profile": "strict"}}))
        .unwrap();
    assert_eq!(preview["dryRun"], true);
    assert!(!case.settings().exists());
    assert!(case.world.host.tasks.approvals.borrow().is_empty());
    let text = preview.to_string();
    assert!(!text.contains("strict"), "{text}");
    assert_eq!(preview["changes"][0]["knob"], "hook_profile");
    assert!(preview["changes"][0].get("value").is_none());
    assert!(preview["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s.get("diff").is_none()));
}

#[test]
fn test03_an_apply_without_a_running_app_writes_nothing() {
    let case = case();
    let error = case
        .config(json!({"set": {"hook_profile": "strict"}, "dry_run": false}))
        .unwrap_err();
    assert_eq!(error.kind, "approval_unavailable");
    assert!(!case.settings().exists());
}

#[test]
fn test04_a_denial_and_a_timeout_write_nothing() {
    let case = case();
    for (decision, kind) in [
        (RunDecision::Denied, "approval_denied"),
        (RunDecision::Timeout, "approval_denied"),
        (RunDecision::Stale, "approval_denied"),
    ] {
        case.decide(decision);
        let error = case
            .config(json!({"set": {"hook_profile": "strict"}, "dry_run": false}))
            .unwrap_err();
        assert_eq!(error.kind, kind);
        assert!(!case.settings().exists());
    }
}

#[test]
fn test05_an_approved_apply_writes_and_the_person_sees_the_value_the_model_does_not() {
    let case = case();
    case.decide(RunDecision::Approved);
    let out = case
        .config(json!({"set": {"hook_profile": "strict"}, "dry_run": false}))
        .unwrap();
    assert_eq!(out["result"]["applied"], true);
    assert!(!out.to_string().contains("strict"), "{out}");
    let written = std::fs::read_to_string(case.settings()).unwrap();
    assert!(written.contains("ECC_HOOK_PROFILE"), "{written}");
    let asked = case.asked();
    assert!(asked.contains("set hook_profile = strict"), "{asked}");
    assert_eq!(case.world.host.tasks.approvals.borrow().len(), 1);
}

#[test]
fn test06_every_apply_asks_again() {
    let case = case();
    case.decide(RunDecision::Approved);
    case.config(json!({"set": {"hook_profile": "strict"}, "dry_run": false}))
        .unwrap();
    case.config(json!({"unset": ["hook_profile"], "dry_run": false}))
        .unwrap();
    assert_eq!(case.world.host.tasks.approvals.borrow().len(), 2);
}

#[test]
fn test07_a_value_shape_the_tool_does_not_take_is_refused_and_lists_and_booleans_are_joined() {
    let case = case();
    let error = case
        .config(json!({"set": {"hook_profile": 4}}))
        .unwrap_err();
    assert_eq!(error.kind, "invalid_arguments");
    let error = case.config(json!({"set": ["x"]})).unwrap_err();
    assert_eq!(error.kind, "invalid_arguments");
    let args = json!({"set": {"a": ["x", "y"], "b": true, "c": "text"}});
    let mut values = supplied_values(&args);
    values.sort();
    assert_eq!(values, vec!["text", "true", "x,y"]);
    let preview = case
        .config(json!({"set": {"gateguard_exempt_globs": ["a/**", "b/**"], "gateguard": false}}))
        .unwrap();
    assert_eq!(preview["changes"].as_array().unwrap().len(), 2);
}

#[test]
fn test08_a_knob_the_adapter_does_not_have_is_invalid_arguments() {
    let case = case();
    let error = case.config(json!({"set": {"nope": "x"}})).unwrap_err();
    assert_eq!(error.kind, "invalid_arguments");
    assert!(error.message.contains("not a knob"), "{}", error.message);
}

#[test]
fn test09_mcp_lists_by_default_and_deny_needs_a_server_and_a_folder() {
    let case = case();
    let listed = case.mcp(json!({})).unwrap();
    assert_eq!(listed["servers"][0]["key"], "plugin:ecc:chrome");
    assert_eq!(
        case.mcp(json!({"action": "deny"})).unwrap_err().kind,
        "invalid_arguments"
    );
    let no_cwd = mcp_with(
        &case.world.host,
        &json!({"id": ID, "action": "deny", "server": "chrome"}),
        &NoClaude,
    )
    .unwrap_err();
    assert_eq!(no_cwd.kind, "invalid_arguments");
    assert_eq!(
        case.mcp(json!({"action": "wipe"})).unwrap_err().kind,
        "invalid_arguments"
    );
}

#[test]
fn test10_deny_previews_then_needs_approval_every_time_and_allow_puts_it_back() {
    let case = case();
    let preview = case
        .mcp(json!({"action": "deny", "server": "chrome"}))
        .unwrap();
    assert_eq!(preview["dryRun"], true);
    assert!(!case.settings().exists());
    let none = case
        .mcp(json!({"action": "deny", "server": "chrome", "dry_run": false}))
        .unwrap_err();
    assert_eq!(none.kind, "approval_unavailable");
    assert!(!case.settings().exists());
    case.decide(RunDecision::Approved);
    case.mcp(json!({"action": "deny", "server": "chrome", "dry_run": false}))
        .unwrap();
    assert!(std::fs::read_to_string(case.settings())
        .unwrap()
        .contains("plugin:ecc:chrome"));
    case.mcp(json!({"action": "allow", "server": "chrome", "dry_run": false}))
        .unwrap();
    assert!(!case.settings().exists());
    assert_eq!(case.world.host.tasks.approvals.borrow().len(), 3);
}
