use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use super::claude::NOT_FOUND;
use super::inventory::{self, HookFilter, World};
use super::ClaudeRunner;
use crate::skills::gitexec::CmdOutput;

struct Tree {
    dir: tempfile::TempDir,
    home: PathBuf,
    claude: PathBuf,
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const HOOK_COMMAND: &str = "node scripts/run-with-flags.js pre:bash:dispatcher scripts/x.js";

fn tree(settings: Value) -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let claude = home.join(".claude");
    let plugin = claude.join("plugins/cache/ecc/ecc/1.2.0");
    put(
        &claude.join("plugins/installed_plugins.json"),
        &json!({"version": 2, "plugins": {
            "ecc@ecc": [{"scope": "user", "version": "1.2.0", "installPath": plugin}],
        }})
        .to_string(),
    );
    put(
        &claude.join("plugins/known_marketplaces.json"),
        &json!({"ecc": {"source": {"source": "github", "repo": "acme/ecc"},
                        "installLocation": claude.join("plugins/marketplaces/ecc")}})
        .to_string(),
    );
    put(
        &claude.join("plugins/marketplaces/ecc/.claude-plugin/marketplace.json"),
        &json!({"name": "ecc", "plugins": [
            {"name": "ecc", "source": {"source": "github", "repo": "acme/ecc"}, "version": "1.10.0"}
        ]})
        .to_string(),
    );
    put(
        &plugin.join(".claude-plugin/plugin.json"),
        &json!({
            "description": "A synthetic plugin",
            "userConfig": {
                "hook_profile": {"type": "string", "title": "Profile", "description": "How strict",
                                 "default": "standard"},
                "api_token": {"type": "string", "title": "Token", "description": "d",
                              "sensitive": true},
            },
        })
        .to_string(),
    );
    put(
        &plugin.join(".mcp.json"),
        &json!({"mcpServers": {
            "chrome": {"command": "npx", "args": ["-y", "chrome-mcp"],
                       "env": {"API_KEY": "SECRET-ENV-VALUE"}},
            "remote": {"type": "http", "url": "https://example.invalid/mcp",
                       "headers": {"Authorization": "Bearer SECRET-HEADER"}},
        }})
        .to_string(),
    );
    put(
        &plugin.join("hooks/hooks.json"),
        &json!({"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [
            {"type": "command", "command": HOOK_COMMAND}]}]}})
        .to_string(),
    );
    put(&claude.join("settings.json"), &settings.to_string());
    Tree { dir, home, claude }
}

fn world(tree: &Tree) -> World {
    World {
        home: tree.home.clone(),
        claude_home: tree.claude.clone(),
        managed: None,
        data_dir: Some(tree.dir.path().join("data")),
        starts: Vec::new(),
    }
}

fn on() -> Value {
    json!({"enabledPlugins": {"ecc@ecc": true}})
}

#[derive(Default)]
struct Claude {
    calls: Mutex<Vec<String>>,
    details: Option<&'static str>,
    configure: Option<String>,
    refresh_fails: bool,
}

impl ClaudeRunner for Claude {
    fn run(&self, args: &[&str], _: Duration) -> Result<CmdOutput, String> {
        let line = args.join(" ");
        self.calls.lock().unwrap().push(line.clone());
        let out = |code: i32, stdout: &str, stderr: &str| {
            Ok(CmdOutput {
                code,
                stdout: stdout.into(),
                stderr: stderr.into(),
            })
        };
        match args {
            ["plugin", "list", ..] => out(1, "", "no list"),
            ["plugin", "details", _] => match self.details {
                Some(text) => out(0, text, ""),
                None => out(1, "", "no details"),
            },
            ["plugin", "configure", _, "--json"] => match &self.configure {
                Some(text) => out(0, text, ""),
                None => out(1, "", "no configure"),
            },
            ["plugin", "marketplace", "update"] if self.refresh_fails => {
                out(1, "", "network is down")
            }
            _ => out(0, "", ""),
        }
    }
}

#[test]
fn test01_the_managed_file_decides_over_every_other_layer() {
    let tree = tree(on());
    let managed = tree.dir.path().join("managed-settings.json");
    put(
        &managed,
        &json!({"enabledPlugins": {"ecc@ecc": false}}).to_string(),
    );
    let mut world = world(&tree);
    world.managed = Some(managed);
    let data = inventory::ls(&world, None, None, false);
    let row = &data["plugins"][0];
    assert_eq!(row["enabled"]["user"], true);
    assert_eq!(row["enabled"]["managed"], false);
    assert_eq!(row["enabled"]["effective"], false);
    assert_eq!(data["managedSettings"]["present"], true);
}

#[test]
fn test02_a_missing_managed_file_is_named_and_not_an_error() {
    let tree = tree(on());
    let mut world = world(&tree);
    world.managed = Some(tree.dir.path().join("none.json"));
    let data = inventory::ls(&world, None, None, false);
    assert_eq!(data["managedSettings"]["present"], false);
    assert_eq!(data["plugins"][0]["enabled"]["managed"], Value::Null);
}

#[test]
fn test03_a_newer_marketplace_version_is_an_update_by_number_not_by_text() {
    let tree = tree(on());
    let data = inventory::ls(&world(&tree), None, None, false);
    let row = &data["plugins"][0];
    assert_eq!(
        row["update"],
        json!({"state": "update", "available": "1.10.0"})
    );
    assert_eq!(data["updatesAvailable"], 1);
    assert_eq!(row["source"], "github:acme/ecc");
    assert_eq!(data["marketplaces"][0]["name"], "ecc");
    assert_eq!(data["marketplaces"][0]["installed"], 1);

    put(
        &tree
            .claude
            .join("plugins/marketplaces/ecc/.claude-plugin/marketplace.json"),
        &json!({"name": "ecc", "plugins": [{"name": "ecc", "source": "x", "version": "1.2.0"}]})
            .to_string(),
    );
    let same = inventory::ls(&world(&tree), None, None, false);
    assert_eq!(same["plugins"][0]["update"]["state"], "current");
    std::fs::remove_dir_all(tree.claude.join("plugins/marketplaces")).unwrap();
    let none = inventory::ls(&world(&tree), None, None, false);
    assert_eq!(none["plugins"][0]["update"]["state"], "unknown");
}

#[test]
fn test04_show_lists_servers_without_their_environment_and_where_each_is_denied() {
    let tree = tree(json!({
        "enabledPlugins": {"ecc@ecc": true},
        "deniedMcpServers": [{"serverName": "plugin:ecc:chrome"}],
    }));
    let folder = tree.dir.path().join("proj");
    put(
        &folder.join(".claude/settings.local.json"),
        &json!({"deniedMcpServers": [{"serverName": "plugin:ecc:remote"}]}).to_string(),
    );
    let data = inventory::show(&world(&tree), None, "ecc@ecc", Some(&folder)).unwrap();
    let text = data.to_string();
    assert!(
        !text.contains("SECRET-ENV-VALUE") && !text.contains("SECRET-HEADER"),
        "{text}"
    );
    let chrome = &data["mcpServers"][0];
    assert_eq!(chrome["command"], json!(["npx", "-y", "chrome-mcp"]));
    assert_eq!(chrome["denied"]["user"], true);
    assert_eq!(chrome["denied"]["local"], false);
    let remote = &data["mcpServers"][1];
    assert_eq!(remote["url"], "https://example.invalid/mcp");
    assert_eq!(remote["denied"]["local"], true);
    assert_eq!(remote["toolPrefix"], "mcp__plugin_ecc_remote__*");
    assert_eq!(data["mcpOutsideGateway"][0]["denied"], true);
}

#[test]
fn test05_options_come_from_the_manifest_and_never_show_a_sensitive_value() {
    let tree = tree(json!({
        "enabledPlugins": {"ecc@ecc": true},
        "pluginConfigs": {"ecc@ecc": {"options": {"hook_profile": "strict", "api_token": "SECRET-TOKEN"}}},
    }));
    let data = inventory::show(&world(&tree), None, "ecc@ecc", None).unwrap();
    assert!(!data.to_string().contains("SECRET-TOKEN"));
    assert_eq!(data["optionsFrom"], "files");
    let options = data["options"].as_array().unwrap();
    let profile = options.iter().find(|o| o["key"] == "hook_profile").unwrap();
    assert_eq!(profile["current"], "strict");
    assert_eq!(profile["default"], "standard");
    let token = options.iter().find(|o| o["key"] == "api_token").unwrap();
    assert_eq!(token["sensitive"], true);
    assert_eq!(token["current"], Value::Null);
    assert_eq!(token["configured"], Value::Null);
}

#[test]
fn test06_claude_configure_gives_the_choices_and_the_configured_list() {
    let tree = tree(on());
    let claude = Claude {
        configure: Some(
            json!({
                "schema": {"hook_profile": {"type": "string", "title": "Profile"},
                           "api_token": {"type": "string", "sensitive": true}},
                "inputs": {"hook_profile": "minimal", "api_token": "SECRET-TOKEN"},
                "choices": {"hook_profile": ["minimal", "standard"]},
                "configured": ["hook_profile", "api_token"],
            })
            .to_string(),
        ),
        ..Claude::default()
    };
    let data = inventory::show(&world(&tree), Some(&claude), "ecc@ecc", None).unwrap();
    assert!(!data.to_string().contains("SECRET-TOKEN"));
    assert_eq!(data["optionsFrom"], "claude-cli");
    let profile = &data["options"][1];
    assert_eq!(profile["key"], "hook_profile");
    assert_eq!(profile["current"], "minimal");
    assert_eq!(profile["choices"], json!(["minimal", "standard"]));
    assert_eq!(data["options"][0]["configured"], true);
    assert_eq!(data["options"][0]["current"], Value::Null);
}

#[test]
fn test07_knobs_show_the_value_and_which_layer_it_comes_from() {
    let tree = tree(json!({
        "enabledPlugins": {"ecc@ecc": true},
        "env": {"GATEGUARD_EXEMPT_GLOBS": "a/**"},
        "pluginConfigs": {"ecc@ecc": {"options": {"hook_profile": "strict"}}},
    }));
    let folder = tree.dir.path().join("proj");
    put(
        &folder.join(".claude/settings.local.json"),
        &json!({"env": {"ECC_GATEGUARD": "off"}}).to_string(),
    );
    let data = inventory::show(&world(&tree), None, "ecc@ecc", Some(&folder)).unwrap();
    let knob = |key: &str| {
        data["knobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["key"] == key)
            .unwrap()
            .clone()
    };
    assert_eq!(
        knob("gateguard")["current"],
        json!({"value": "off", "from": "folder-env"})
    );
    assert_eq!(
        knob("gateguard_exempt_globs")["current"],
        json!({"value": "a/**", "from": "user-env"})
    );
    assert_eq!(
        knob("hook_profile")["current"],
        json!({"value": "strict", "from": "option"})
    );
    assert_eq!(
        knob("hooks_enabled")["current"],
        json!({"value": "true", "from": "default"})
    );
    assert_eq!(data["adapter"], "ecc@ecc");
}

#[test]
fn test08_the_projected_cost_comes_from_claude_details_and_is_labelled() {
    let tree = tree(on());
    let claude = Claude {
        details: Some("ecc 1.2.0\n\nProjected token cost\n  Always-on:   ~40,639 tok   added\n"),
        ..Claude::default()
    };
    let data = inventory::ls(&world(&tree), Some(&claude), None, false);
    assert_eq!(
        data["plugins"][0]["cost"]["projected"],
        json!({"value": 40639, "basis": "projected"})
    );
    assert_eq!(data["plugins"][0]["cost"]["measured"], Value::Null);
}

#[test]
fn test09_ls_is_partial_only_while_claude_could_not_fill_the_cost() {
    let tree = tree(on());
    let without = inventory::ls(&world(&tree), None, None, false);
    assert_eq!(without["partial"], true);
    assert_eq!(without["plugins"][0]["cost"]["projected"], Value::Null);
    let empty = tempfile::tempdir().unwrap();
    let nothing = inventory::ls(
        &World {
            home: empty.path().to_path_buf(),
            claude_home: empty.path().join(".claude"),
            managed: None,
            data_dir: None,
            starts: Vec::new(),
        },
        None,
        None,
        false,
    );
    assert_eq!(nothing["partial"], false);
    assert_eq!(nothing["plugins"], json!([]));
}

#[test]
fn test10_refresh_updates_the_marketplaces_first_and_names_a_failure() {
    let tree = tree(on());
    let claude = Claude::default();
    let data = inventory::ls(&world(&tree), Some(&claude), None, true);
    assert_eq!(data["refreshed"], true);
    assert!(claude
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|c| c == "plugin marketplace update"));
    let failing = Claude {
        refresh_fails: true,
        ..Claude::default()
    };
    let failed = inventory::ls(&world(&tree), Some(&failing), None, true);
    assert_eq!(failed["refreshed"], false);
    assert_eq!(failed["refreshError"], "network is down");
    let plain = Claude::default();
    inventory::ls(&world(&tree), Some(&plain), None, false);
    assert!(!plain
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.contains("marketplace")));
    let missing = inventory::ls(&world(&tree), None, None, true);
    assert_eq!(missing["refreshError"], NOT_FOUND);
}

#[test]
fn test11_an_adapter_names_the_runtime_id_of_a_plugin_hook_and_its_switch() {
    let tree = tree(on());
    let data = inventory::hooks(&world(&tree), None, None, &HookFilter::default());
    let hook = &data["hooks"][0];
    assert_eq!(hook["runtimeId"], "pre:bash:dispatcher");
    assert_eq!(hook["switch"]["method"], "plugin-setting");
    assert!(hook["switch"]["detail"]
        .as_str()
        .unwrap()
        .contains("disabled_hooks=pre:bash:dispatcher"));
    let show = inventory::show(&world(&tree), None, "ecc@ecc", None).unwrap();
    assert_eq!(show["hooks"][0]["runtimeId"], "pre:bash:dispatcher");
}

#[test]
fn test12_a_hook_a_skill_installed_belongs_to_the_skill() {
    let script = format!("{}/skills/guard/run.sh", "/synthetic/.claude");
    let tree = tree(json!({
        "enabledPlugins": {"ecc@ecc": true},
        "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [
            {"type": "command", "command": script}]}]},
    }));
    let data_dir = tree.dir.path().join("data");
    put(
        &data_dir.join("mcpm-skills.lock"),
        &json!({
            "version": 1, "synced_at": "t", "scope": "global", "output_root": "/",
            "skills": {"guard": {"source": "local", "version": null, "hash": "h",
                "clients_synced": ["claude-code"], "warnings": [],
                "output_files": {}, "hooks_installed": {"claude-code": [script]}}},
            "rules": {},
        })
        .to_string(),
    );
    let data = inventory::hooks(
        &world(&tree),
        None,
        None,
        &HookFilter {
            owner: Some("skill"),
            ..HookFilter::default()
        },
    );
    assert_eq!(data["hooks"].as_array().unwrap().len(), 1);
    assert_eq!(
        data["hooks"][0]["owner"],
        json!({"kind": "skill", "name": "guard"})
    );
    assert_eq!(data["hooks"][0]["switch"]["method"], "skills-sync");
}

#[test]
fn test13_managed_hooks_are_read_and_allow_managed_hooks_only_is_reported() {
    let tree = tree(on());
    let managed = tree.dir.path().join("managed-settings.json");
    put(
        &managed,
        &json!({
            "allowManagedHooksOnly": true,
            "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "corp.sh"}]}]},
        })
        .to_string(),
    );
    let mut world = world(&tree);
    world.managed = Some(managed);
    let data = inventory::hooks(&world, None, None, &HookFilter::default());
    assert_eq!(data["managedOnly"], true);
    assert_eq!(data["counts"]["byOwner"]["managed"], 1);
    assert_eq!(data["managedSettings"]["present"], true);
}

#[test]
fn test14_env_assignments_in_front_of_a_hook_command_are_not_printed() {
    let tree = tree(json!({"hooks": {"Stop": [{"hooks": [
        {"type": "command", "command": "GH_TOKEN=SECRET-VALUE node run.js"}]}]}}));
    let data = inventory::hooks(&world(&tree), None, None, &HookFilter::default());
    assert!(!data.to_string().contains("SECRET-VALUE"));
    assert_eq!(data["hooks"][0]["command"], "GH_TOKEN=… node run.js");
}

#[test]
fn test15_hooks_in_the_manifest_are_listed_with_those_in_the_hooks_file() {
    let tree = tree(on());
    let plugin = tree.claude.join("plugins/cache/ecc/ecc/1.2.0");
    put(
        &plugin.join(".claude-plugin/plugin.json"),
        &json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "inline.sh"}]}]}})
            .to_string(),
    );
    let data = inventory::show(&world(&tree), None, "ecc@ecc", None).unwrap();
    let commands: Vec<&str> = data["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h["command"].as_str())
        .collect();
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert!(commands.contains(&"inline.sh"));
    assert_eq!(data["brings"]["hooks"], 2);
}

#[test]
fn test16_a_plugin_of_a_tap_marketplace_names_its_selective_loading_source() {
    let tree = tree(on());
    let data = tree.dir.path().join("data");
    crate::skills::taps::save_taps(
        &data,
        &[crate::skills::taps::Tap {
            name: "acme-tap".into(),
            repo: "acme/ecc".into(),
            url: "https://github.com/acme/ecc".into(),
        }],
    )
    .unwrap();
    put(
        &data.join("taps/acme-tap/.claude-plugin/marketplace.json"),
        &json!({"name": "ecc", "plugins": [{"name": "ecc", "source": "./ecc"}]}).to_string(),
    );
    std::fs::create_dir_all(data.join("taps/acme-tap/ecc")).unwrap();
    let linked = inventory::ls(&world(&tree), None, None, false);
    assert_eq!(linked["plugins"][0]["selectSource"], "acme-tap/ecc");
    std::fs::remove_dir_all(data.join("taps")).unwrap();
    let alone = inventory::ls(&world(&tree), None, None, false);
    assert_eq!(alone["plugins"][0]["selectSource"], Value::Null);
}
