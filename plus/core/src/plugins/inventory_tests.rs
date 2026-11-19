use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::inventory::{self, HookFilter, World};

struct Tree {
    dir: tempfile::TempDir,
    home: PathBuf,
    claude: PathBuf,
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn tree(settings: Value) -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let claude = home.join(".claude");
    let plugin = claude.join("plugins/cache/ecc");
    put(
        &claude.join("plugins/installed_plugins.json"),
        &json!({"version": 2, "plugins": {
            "ecc@ecc": [{"scope": "user", "version": "1.2.0", "installPath": plugin}],
            "idle@ecc": [{"scope": "user", "version": "0.1.0"}],
        }})
        .to_string(),
    );
    put(
        &plugin.join("skills/review/SKILL.md"),
        "---\nname: review\n---\n",
    );
    put(&plugin.join("agents/planner.md"), "x");
    put(&plugin.join("commands/ship.md"), "x");
    put(
        &plugin.join(".claude-plugin/plugin.json"),
        &json!({"description": "A synthetic plugin"}).to_string(),
    );
    put(
        &plugin.join(".mcp.json"),
        &json!({"mcpServers": {"chrome": {"command": "chrome-mcp"}}}).to_string(),
    );
    put(
        &plugin.join("hooks/hooks.json"),
        &json!({"hooks": {
            "PreToolUse": [{"matcher": "Bash|Edit", "hooks": [
                {"type": "command", "command": "node guard.js", "timeout": 5}]}],
            "SessionStart": [{"hooks": [{"type": "command", "command": "node start.js"}]}],
        }})
        .to_string(),
    );
    put(&claude.join("settings.json"), &settings.to_string());
    Tree { dir, home, claude }
}

fn world(tree: &Tree) -> World<'_> {
    World {
        home: &tree.home,
        claude_home: &tree.claude,
    }
}

fn enabled() -> Value {
    json!({"enabledPlugins": {"ecc@ecc": true}})
}

#[test]
fn test01_ls_lists_each_plugin_with_where_it_is_on_and_what_it_brings() {
    let tree = tree(enabled());
    let data = inventory::ls(&world(&tree), None, None);
    let rows = data["plugins"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let ecc = rows.iter().find(|r| r["id"] == "ecc@ecc").unwrap();
    assert_eq!(ecc["version"], "1.2.0");
    assert_eq!(ecc["enabled"]["user"], true);
    assert_eq!(ecc["enabled"]["effective"], true);
    assert_eq!(
        ecc["brings"],
        json!({"skills": 1, "agents": 1, "commands": 1, "hooks": 2, "mcpServers": 1, "lspServers": 0})
    );
    assert_eq!(ecc["from"], "files");
    let idle = rows.iter().find(|r| r["id"] == "idle@ecc").unwrap();
    assert_eq!(idle["enabled"]["effective"], false);
}

#[test]
fn test02_a_folder_setting_wins_over_the_user_setting() {
    let tree = tree(enabled());
    let folder = tree.dir.path().join("proj");
    put(
        &folder.join(".claude/settings.local.json"),
        &json!({"enabledPlugins": {"ecc@ecc": false}}).to_string(),
    );
    let data = inventory::ls(&world(&tree), None, Some(&folder));
    let ecc = &data["plugins"][0];
    assert_eq!(ecc["enabled"]["local"], false);
    assert_eq!(ecc["enabled"]["effective"], false);
}

#[test]
fn test03_show_names_components_hooks_and_the_mcp_server_key() {
    let tree = tree(enabled());
    let data = inventory::show(&world(&tree), None, "ecc@ecc", None).unwrap();
    assert_eq!(data["description"], "A synthetic plugin");
    assert_eq!(data["components"]["skills"], json!(["review"]));
    assert_eq!(data["mcpServers"][0]["key"], "plugin:ecc:chrome");
    assert_eq!(data["hooks"].as_array().unwrap().len(), 2);
    assert_eq!(data["hooks"][0]["switch"]["method"], "disable-plugin");
    assert_eq!(
        inventory::show(&world(&tree), None, "ghost@x", None)
            .unwrap_err()
            .message,
        "plugin not found: ghost@x"
    );
}

#[test]
fn test04_hooks_cover_settings_and_enabled_plugins_and_count_per_tool() {
    let tree = tree(json!({
        "enabledPlugins": {"ecc@ecc": true},
        "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [
            {"type": "command", "command": "~/bin/own.sh"}]}]},
    }));
    let data = inventory::hooks(&world(&tree), None, None, &HookFilter::default());
    assert_eq!(data["disabledAll"], false);
    assert_eq!(data["counts"]["byOwner"], json!({"plugin": 2, "user": 1}));
    assert_eq!(
        data["counts"]["perTool"]["Bash"],
        json!({"pre": 2, "post": 0, "total": 2})
    );
    assert_eq!(
        data["counts"]["perTool"]["Edit"],
        json!({"pre": 1, "post": 0, "total": 1})
    );
    assert_eq!(data["counts"]["otherEvents"], json!({"SessionStart": 1}));
    assert_eq!(data["conflicts"][0]["tools"], json!(["Bash"]));
}

#[test]
fn test05_a_plugin_that_is_off_brings_no_hooks_and_disable_all_is_reported() {
    let tree = tree(json!({"disableAllHooks": true}));
    let data = inventory::hooks(&world(&tree), None, None, &HookFilter::default());
    assert_eq!(data["disabledAll"], true);
    assert_eq!(data["hooks"], json!([]));
}

#[test]
fn test06_toolport_markers_make_the_owner_toolport_and_filters_narrow_the_list() {
    let tree = tree(json!({
        "enabledPlugins": {"ecc@ecc": true},
        "hooks": {"PreToolUse": [{"matcher": "Write", "hooks": [
            {"type": "command", "command": "toolportctl guard --toolport-guard"}]}]},
    }));
    let all = inventory::hooks(&world(&tree), None, None, &HookFilter::default());
    assert_eq!(all["counts"]["byOwner"]["toolport"], 1);
    let by_owner = inventory::hooks(
        &world(&tree),
        None,
        None,
        &HookFilter {
            owner: Some("toolport"),
            ..Default::default()
        },
    );
    assert_eq!(by_owner["hooks"].as_array().unwrap().len(), 1);
    assert_eq!(by_owner["hooks"][0]["marker"], "--toolport-guard");
    assert_eq!(by_owner["hooks"][0]["switch"]["method"], "toolport-command");
    let by_tool = inventory::hooks(
        &world(&tree),
        None,
        None,
        &HookFilter {
            tool: Some("Edit"),
            event: Some("PreToolUse"),
            ..Default::default()
        },
    );
    assert_eq!(by_tool["hooks"].as_array().unwrap().len(), 1);
}

#[test]
fn test07_a_regex_matcher_and_an_exact_list_pick_their_tools() {
    let tree = tree(json!({
        "hooks": {"PostToolUse": [
            {"matcher": "^(Edit|Write)$", "hooks": [{"type": "command", "command": "a"}]},
            {"matcher": "", "hooks": [{"type": "command", "command": "b"}]},
        ]},
    }));
    let data = inventory::hooks(&world(&tree), None, None, &HookFilter::default());
    let per_tool = &data["counts"]["perTool"];
    assert_eq!(per_tool["Edit"]["post"], 2);
    assert_eq!(per_tool["Bash"]["post"], 1);
}

#[test]
fn test08_the_home_folder_is_shortened_in_commands() {
    let tree = tree(json!({}));
    let command = format!("{}/bin/own.sh", tree.home.display());
    let tree2 = {
        put(
            &tree.claude.join("settings.json"),
            &json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": command}]}]}})
                .to_string(),
        );
        tree
    };
    let data = inventory::hooks(&world(&tree2), None, None, &HookFilter::default());
    assert_eq!(data["hooks"][0]["command"], "~/bin/own.sh");
}
