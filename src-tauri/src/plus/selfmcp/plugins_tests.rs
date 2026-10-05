use super::*;
use crate::plus::direct::tests::{tree, world};
use serde_json::{json, Value};
use std::path::Path;

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn install(home: &Path) {
    let claude = home.join(".claude");
    let dir = claude.join("plugins/cache/ecc/ecc/1.0.0");
    put(&dir.join(".claude-plugin/plugin.json"), r#"{"name": "ecc", "version": "1.0.0"}"#);
    put(&dir.join(".mcp.json"), r#"{"mcpServers": {"chrome-devtools": {"command": "/bin/true"}}}"#);
    put(
        &claude.join("plugins/installed_plugins.json"),
        &json!({"version": 2, "plugins": {"ecc@ecc": [{"scope": "user", "installPath": dir, "version": "1.0.0"}]}})
            .to_string(),
    );
}

fn settings(cwd: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap()).unwrap()
}

#[test]
fn plugin_control_tools_preview_by_default_and_write_only_inside_the_folder() {
    world(|w| {
        for name in ["plugins_config", "plugins_mcp"] {
            let tool = find_tool(name).unwrap();
            assert_eq!((tool.tier, tool.gate), (2, Gate::None), "{name}");
            assert!(previews_by_default(tool), "{name}");
            assert!(tool.description.contains("dry_run is on by default"), "{name}");
        }
        let schema = catalog::input_schema(find_tool("plugins_mcp").unwrap());
        assert_eq!(schema["required"], json!(["action", "id", "server", "cwd"]));

        let _claude = crate::clients::EnvRestore::set("TOOLPORT_CLAUDE_BIN", &w.home.join("no-claude"));
        install(&w.home);
        let cwd = w.home.join("work/client-repo");
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        put(&cwd.join(".claude/settings.local.json"), "{\n  \"model\": \"opus\"\n}\n");
        let before = tree(&cwd);

        let plan = call_tool("plugins_config", &json!({"id": "ecc@ecc", "cwd": cwd, "set": {"hook_profile": "minimal", "gateguard": false}})).unwrap();
        assert_eq!(plan["dryRun"], true);
        assert_eq!(plan["scope"], "folder");
        assert_eq!(tree(&cwd), before, "no dry_run argument means a preview");

        let applied = call_tool(
            "plugins_config",
            &json!({"id": "ecc@ecc", "cwd": cwd, "set": {"hook_profile": "minimal", "gateguard": false}, "dry_run": false}),
        )
        .unwrap();
        assert_eq!(applied["result"]["applied"], true);
        let written = settings(&cwd);
        assert_eq!(written["env"]["ECC_HOOK_PROFILE"], "minimal");
        assert_eq!(written["env"]["ECC_GATEGUARD"], "off");
        assert_eq!(written["model"], "opus");

        let off = call_tool("plugins_config", &json!({"id": "ecc@ecc", "cwd": cwd, "unset": ["hook_profile", "gateguard"], "dry_run": false})).unwrap();
        assert_eq!(off["result"]["applied"], true);
        assert_eq!(tree(&cwd), before);

        let dry = call_tool("plugins_mcp", &json!({"action": "deny", "id": "ecc@ecc", "server": "chrome-devtools", "cwd": cwd})).unwrap();
        assert_eq!(dry["dryRun"], true);
        assert_eq!(dry["serverName"], "plugin:ecc:chrome-devtools");
        assert_eq!(tree(&cwd), before);
        call_tool("plugins_mcp", &json!({"action": "deny", "id": "ecc@ecc", "server": "chrome-devtools", "cwd": cwd, "dry_run": false})).unwrap();
        assert_eq!(settings(&cwd)["deniedMcpServers"], json!([{"serverName": "plugin:ecc:chrome-devtools"}]));
        call_tool("plugins_mcp", &json!({"action": "allow", "id": "ecc@ecc", "server": "chrome-devtools", "cwd": cwd, "dry_run": false})).unwrap();
        assert_eq!(tree(&cwd), before);

        for (tool, args) in [
            ("plugins_config", json!({"cwd": cwd, "set": {"hook_profile": "minimal"}})),
            ("plugins_config", json!({"id": "ecc@ecc", "cwd": cwd})),
            ("plugins_config", json!({"id": "ecc@ecc", "cwd": cwd, "set": {"no_such_knob": "x"}})),
            ("plugins_config", json!({"id": "ecc@ecc", "cwd": cwd, "set": {"hook_profile": "loud"}})),
            ("plugins_config", json!({"id": "ecc@ecc", "set": {"gateguard": false}})),
            ("plugins_config", json!({"id": "ecc@ecc", "cwd": cwd, "set": {"hook_profile": {"nested": 1}}})),
            ("plugins_mcp", json!({"action": "block", "id": "ecc@ecc", "server": "chrome-devtools", "cwd": cwd})),
            ("plugins_mcp", json!({"action": "deny", "id": "ecc@ecc", "server": "chrome-devtools"})),
        ] {
            let err = call_tool(tool, &args).unwrap_err();
            assert_eq!(err.kind, "invalid_arguments", "{tool} {args}: {}", err.message);
        }
        let missing = call_tool("plugins_mcp", &json!({"action": "deny", "id": "ecc@ecc", "server": "nope", "cwd": cwd})).unwrap_err();
        assert_eq!(missing.kind, "not_found");
        let unknown = call_tool("plugins_config", &json!({"id": "nope@nowhere", "cwd": cwd, "set": {"gateguard": true}})).unwrap_err();
        assert_eq!(unknown.kind, "not_found");
        assert_eq!(tree(&cwd), before);
    });
}
