use super::*;
use crate::plus::direct::tests::{installed, read, tree, world};
use serde_json::{json, Value};

fn call(name: &str, args: Value) -> Result<Value, ToolError> {
    call_tool(name, &args)
}

fn kind(result: Result<Value, ToolError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(e) => e.kind,
    }
}

#[test]
fn the_direct_tools_are_tiered_and_dry_run_by_default() {
    for (name, tier) in [
        ("client_direct_ls", 1),
        ("client_direct_add", 2),
        ("client_direct_rm", 2),
    ] {
        let tool = find_tool(name).unwrap();
        assert_eq!(tool.tier, tier, "{name}");
        assert_eq!(tool.gate, Gate::None, "{name}");
        let schema = catalog::input_schema(tool);
        let has_dry_run = schema["properties"].get("dry_run").is_some();
        assert_eq!(has_dry_run, tier == 2, "{name}");
        if has_dry_run {
            assert_eq!(schema["properties"]["dry_run"]["default"], true, "{name}");
            assert_eq!(schema["required"], json!(["server", "client"]), "{name}");
        }
    }
    assert!(find_tool("client_direct_add")
        .unwrap()
        .description
        .contains("bypasses"));
}

#[test]
fn a_default_call_plans_and_writes_nothing() {
    world(|w| {
        let path = installed("claude-code");
        let before = (tree(&w.home), tree(&w.data));
        let planned = call(
            "client_direct_add",
            json!({"server": "alpha", "client": "claude-code"}),
        )
        .unwrap();
        assert_eq!(planned["dryRun"], true);
        assert_eq!(planned["action"], "added");
        assert!(planned["tradeoff"].as_str().unwrap().contains("gateway"));
        assert!(!read("claude-code").contains("alpha"));
        assert!(path.exists());
        assert_eq!((tree(&w.home), tree(&w.data)), before);

        let removal = call(
            "client_direct_rm",
            json!({"server": "alpha", "client": "claude-code"}),
        )
        .unwrap();
        assert_eq!(removal["dryRun"], true);
        assert_eq!(removal["action"], "absent");
        assert_eq!((tree(&w.home), tree(&w.data)), before);
    });
}

#[test]
fn add_ls_and_rm_apply_only_with_dry_run_false() {
    world(|w| {
        installed("claude-code");
        installed("codex");
        for client in ["claude-code", "codex"] {
            let added = call(
                "client_direct_add",
                json!({"server": "alpha", "client": client, "dry_run": false}),
            )
            .unwrap();
            assert_eq!(added["action"], "added", "{client}");
            assert_eq!(added["dryRun"], false, "{client}");
            assert!(read(client).contains("alpha"), "{client}");
        }
        let again = call(
            "client_direct_add",
            json!({"server": "alpha", "client": "codex", "dry_run": false}),
        )
        .unwrap();
        assert_eq!(again["action"], "unchanged");

        let all = call("client_direct_ls", json!({})).unwrap();
        let rows = all["entries"].as_array().unwrap();
        assert!(rows
            .iter()
            .any(|r| r["client"] == "codex" && r["server"] == "alpha"));
        let one = call("client_direct_ls", json!({"client": "claude-code"})).unwrap();
        assert!(one["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["client"] == "claude-code"));

        let kept = call(
            "client_direct_rm",
            json!({"server": "alpha", "client": "codex"}),
        )
        .unwrap();
        assert_eq!(kept["dryRun"], true);
        assert!(read("codex").contains("alpha"));
        let gone = call(
            "client_direct_rm",
            json!({"server": "alpha", "client": "codex", "dry_run": false}),
        )
        .unwrap();
        assert_eq!(gone["action"], "removed");
        assert!(!read("codex").contains("alpha"));
        assert!(read("claude-code").contains("alpha"));
        let _ = w;
    });
}

#[test]
fn errors_keep_the_core_kinds() {
    world(|_| {
        installed("claude-code");
        let add = |server: &str, client: &str| {
            call(
                "client_direct_add",
                json!({"server": server, "client": client, "dry_run": false}),
            )
        };
        assert_eq!(kind(add("no-such", "claude-code")), "not_found");
        assert_eq!(kind(add("alpha", "no-such-client")), "not_found");
        assert_eq!(kind(add("beta", "claude-code")), "invalid_arguments");
        assert_eq!(kind(add("cred", "claude-code")), "invalid_arguments");
        assert_eq!(kind(add("rooted", "claude-code")), "invalid_arguments");
        assert_eq!(kind(add("-alpha", "claude-code")), "invalid_arguments");
        assert_eq!(kind(add("alpha", "")), "invalid_arguments");
        assert_eq!(
            kind(call("client_direct_add", json!({"server": "alpha"}))),
            "invalid_arguments"
        );
        assert_eq!(
            kind(call("client_direct_ls", json!({"client": "no-such-client"}))),
            "not_found"
        );
        assert!(!read("claude-code").contains("beta"));

        std::fs::write(
            crate::plus::direct::tests::config_path("claude-code"),
            r#"{"mcpServers": {"alpha": {"command": "someone-else"}}}"#,
        )
        .unwrap();
        assert_eq!(kind(add("alpha", "claude-code")), "conflict");
        assert!(read("claude-code").contains("someone-else"));
    });
}
