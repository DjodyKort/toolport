use super::*;
use crate::plus::direct::tests::{tree, world};
use serde_json::json;
use std::path::Path;

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn bundle_tools_follow_their_tiers_and_apply_and_undo_only_inside_the_folder() {
    world(|w| {
        for (name, tier) in [
            ("context_bundle_ls", 1),
            ("context_bundle_status", 1),
            ("context_bundle_apply", 2),
            ("context_bundle_undo", 2),
        ] {
            let tool = find_tool(name).unwrap();
            assert_eq!((tool.tier, tool.gate), (tier, Gate::None), "{name}");
        }
        let schema = catalog::input_schema(find_tool("context_bundle_apply").unwrap());
        assert_eq!(schema["required"], json!(["name", "cwd"]));
        assert!(schema["properties"].get("dry_run").is_some());

        let repo = w.home.join(".config/mcpm/skills_repo");
        put(&repo.join("skills/notes-helper/SKILL.md"), "---\nname: notes-helper\ndescription: d\n---\nbody\n");
        put(
            &repo.join("profiles/ops.yaml"),
            "format: 1\nname: ops\nskills:\n  off: [notes-helper]\nplugins:\n  off: [\"tools-pack@tools-market\"]\n",
        );
        let cwd = w.home.join("work/client-repo");
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        put(&cwd.join(".claude/settings.local.json"), "{\n  \"model\": \"opus\"\n}\n");
        let before = tree(&cwd);

        let listed = call_tool("context_bundle_ls", &json!({})).unwrap();
        assert_eq!(listed["bundles"][0]["name"], "ops");
        assert_eq!(listed["bundles"][0]["skills"]["off"], 1);

        let dry = call_tool("context_bundle_apply", &json!({"name": "ops", "cwd": cwd, "dry_run": true})).unwrap();
        assert_eq!(dry["dryRun"], true);
        assert_eq!(tree(&cwd), before);

        let applied = call_tool("context_bundle_apply", &json!({"name": "ops", "cwd": cwd})).unwrap();
        assert_eq!(applied["result"]["applied"], true);
        let text = std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap();
        assert!(text.contains("\"notes-helper\": \"off\"") && text.contains("\"model\": \"opus\""));
        let status = call_tool("context_bundle_status", &json!({"cwd": cwd})).unwrap();
        assert_eq!(status["applied"]["bundle"], "ops");
        assert_eq!(status["applied"]["drift"], false);

        let undone = call_tool("context_bundle_undo", &json!({"cwd": cwd, "dry_run": true})).unwrap();
        assert_eq!(undone["dryRun"], true);
        assert!(std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap().contains("notes-helper"));
        call_tool("context_bundle_undo", &json!({"cwd": cwd})).unwrap();
        assert_eq!(tree(&cwd), before);
        assert!(call_tool("context_bundle_status", &json!({"cwd": cwd})).unwrap()["applied"].is_null());

        for (tool, args) in [
            ("context_bundle_apply", json!({"name": "ops"})),
            ("context_bundle_apply", json!({"cwd": cwd})),
            ("context_bundle_undo", json!({})),
            ("context_bundle_status", json!({})),
        ] {
            assert_eq!(call_tool(tool, &args).unwrap_err().kind, "invalid_arguments", "{tool}");
        }
        assert_eq!(call_tool("context_bundle_apply", &json!({"name": "nope", "cwd": cwd})).unwrap_err().kind, "not_found");
        assert_eq!(call_tool("context_bundle_undo", &json!({"cwd": cwd})).unwrap_err().kind, "conflict");
    });
}
