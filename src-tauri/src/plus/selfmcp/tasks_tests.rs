use super::*;
use crate::plus::tasks::store::{self, Status};
use crate::plus::testutil::DataDirFx;
use serde_json::json;

fn kind(result: Result<serde_json::Value, ToolError>) -> &'static str {
    result.unwrap_err().kind
}

fn task() -> crate::plus::tasks::model::Task {
    crate::plus::tasks::model::parse(
        &json!({
            "id": "portal-token", "title": "Refresh the portal token", "description": "", "enabled": true,
            "requires": {"servers": ["acme"], "commands": []},
            "writesSecrets": [{"server": "acme", "key": "API_KEY"}],
            "steps": [
                {"id": "sign-in", "type": "needs-you", "title": "Sign in", "instructions": "Sign in first"},
                {"id": "read", "type": "mcp", "title": "Read", "server": "acme", "tool": "get", "args": {}, "capture": ["token"]},
                {"id": "store", "type": "secret-set", "title": "Store", "server": "acme", "key": "API_KEY", "from": "token"}
            ],
            "triggers": {"manual": true, "cli": true, "selfMcp": {"enabled": true, "approval": "every-run"}, "schedule": null, "onAuthFailure": []}
        })
        .to_string(),
    )
    .unwrap()
}

#[test]
fn task_tools_follow_their_tiers_and_a_run_without_an_approval_starts_nothing() {
    let _fx = DataDirFx::new("selfmcp-tasks", "tiers");
    for (name, tier) in [("tasks_list", 1), ("tasks_get", 1), ("tasks_history", 1), ("tasks_run", 2), ("tasks_cancel", 2)] {
        let tool = find_tool(name).unwrap();
        assert_eq!((tool.tier, tool.gate), (tier, Gate::None), "{name}");
    }
    for absent in ["tasks_add", "tasks_edit", "tasks_rm", "tasks_resume"] {
        assert!(find_tool(absent).is_none(), "{absent} must not be exposed");
    }
    assert_eq!(catalog::input_schema(find_tool("tasks_run").unwrap())["required"], json!(["id"]));
    assert!(catalog::input_schema(find_tool("tasks_run").unwrap())["properties"].get("dry_run").is_some());
    assert_eq!(catalog::input_schema(find_tool("tasks_cancel").unwrap())["required"], json!(["run"]));

    let t = task();
    store::save_task(&t).unwrap();
    let listed = call_tool("tasks_list", &json!({})).unwrap();
    assert_eq!(listed["tasks"][0]["id"], "portal-token");
    let got = call_tool("tasks_get", &json!({"id": "portal-token"})).unwrap();
    assert_eq!(got["task"]["writesSecrets"][0]["key"], "API_KEY", "{got}");
    assert_eq!(kind(call_tool("tasks_get", &json!({"id": "no-such-task"}))), "not_found");
    assert_eq!(kind(call_tool("tasks_get", &json!({}))), "invalid_arguments");

    let dry = call_tool("tasks_run", &json!({"id": "portal-token", "dry_run": true})).unwrap();
    assert_eq!(dry["dryRun"], true);
    assert!(dry["plan"]["steps"].as_array().unwrap().len() >= 3);

    assert_eq!(kind(call_tool("tasks_run", &json!({"id": "portal-token"}))), "approval_unavailable");
    assert!(store::list_runs(None).unwrap().is_empty());

    let run = store::new_run(&t, "manual");
    store::create_run(&run).unwrap();
    store::update_run(&run.id, |r| r.runner_pid = Some(2_147_000_000)).unwrap();
    let history = call_tool("tasks_history", &json!({"id": "portal-token", "limit": 1})).unwrap();
    assert_eq!(history["runs"][0]["id"], run.id);
    assert_eq!(call_tool("tasks_history", &json!({"run": run.id})).unwrap()["run"]["status"], "running");
    let cancelled = call_tool("tasks_cancel", &json!({"run": run.id})).unwrap();
    assert_eq!(cancelled["run"]["status"], "cancelled");
    assert_eq!(store::load_run(&run.id).unwrap().status, Status::Cancelled);
}
