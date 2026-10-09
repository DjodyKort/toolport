use super::*;
use crate::plus::direct::tests::{tree, world};
use serde_json::json;
use std::path::Path;

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn leave_out_tool_previews_by_default_and_writes_only_inside_the_folder() {
    world(|w| {
        let tool = find_tool("context_leave_out").unwrap();
        assert_eq!((tool.tier, tool.gate), (2, Gate::None));
        let schema = catalog::input_schema(tool);
        assert_eq!(schema["required"], json!(["action", "cwd"]));
        assert_eq!(schema["properties"]["dry_run"]["default"], true);

        put(&w.home.join(".claude/CLAUDE.md"), "global rules\n");
        let cwd = w.home.join("work/client-repo");
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        put(&cwd.join("CLAUDE.md"), "project rules\n");
        put(&cwd.join(".claude/settings.local.json"), "{\n  \"model\": \"opus\"\n}\n");
        let before = tree(&cwd);
        let global = w.home.join(".claude/CLAUDE.md");

        let preview = call_tool("context_leave_out", &json!({"action": "add", "path": global, "cwd": cwd})).unwrap();
        assert_eq!(preview["dryRun"], true);
        assert!(preview["result"].is_null());
        assert_eq!(tree(&cwd), before);

        let done = call_tool(
            "context_leave_out",
            &json!({"action": "add", "path": global, "cwd": cwd, "dry_run": false}),
        )
        .unwrap();
        assert_eq!(done["result"]["applied"], true);
        assert_eq!(done["leftOut"].as_array().unwrap().len(), 1);
        let text = std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap();
        assert!(text.contains("claudeMdExcludes") && text.contains("CLAUDE.md") && text.contains("\"model\": \"opus\""));
        assert_eq!(std::fs::read_to_string(&global).unwrap(), "global rules\n");

        call_tool("context_leave_out", &json!({"action": "add", "memory": true, "cwd": cwd, "dry_run": false})).unwrap();
        let text = std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap();
        assert!(text.contains("\"autoMemoryEnabled\": false"));

        call_tool("context_leave_out", &json!({"action": "rm", "path": global, "cwd": cwd, "dry_run": false})).unwrap();
        call_tool("context_leave_out", &json!({"action": "rm", "memory": true, "cwd": cwd, "dry_run": false})).unwrap();
        assert_eq!(tree(&cwd), before);

        for args in [
            json!({"action": "add", "path": global}),
            json!({"action": "add", "cwd": cwd}),
            json!({"action": "add", "path": global, "memory": true, "cwd": cwd}),
            json!({"action": "block", "path": global, "cwd": cwd}),
        ] {
            assert_eq!(call_tool("context_leave_out", &args).unwrap_err().kind, "invalid_arguments", "{args}");
        }
        let missing = call_tool("context_leave_out", &json!({"action": "add", "path": "/nowhere/CLAUDE.md", "cwd": cwd})).unwrap_err();
        assert_eq!(missing.kind, "not_found");
        let relative = call_tool("context_leave_out", &json!({"action": "add", "path": "CLAUDE.md", "cwd": cwd})).unwrap_err();
        assert_eq!(relative.kind, "invalid_arguments");
    });
}
