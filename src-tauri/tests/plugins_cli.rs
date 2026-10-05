//! `toolportctl plugins` and `hooks` as a user runs them (MIG-PLG-1): the real binary over the
//! plugin and hook fixture of the contract (section 14), with the recorded `claude` stub and with
//! no `claude` at all. A sentinel script stands for every command a hook, a plugin or an MCP server
//! declares: if any read path started one, it would leave a mark.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/plugins_world.rs"]
mod plugins_world;

use ctl_world::CtlWorld;

struct Fixture {
    world: CtlWorld,
    plugins: plugins_world::PluginsWorld,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let world = CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"));
        let plugins = plugins_world::build_in(&world.base, &world.claude);
        Self { world, plugins }
    }

    fn sentinel(&self) -> PathBuf {
        self.world.base.join("sentinel.sh")
    }

    fn mark(&self) -> PathBuf {
        self.world.base.join("sentinel.log")
    }

    /// Runs `toolportctl --json <args>` with the world's environment, `env` replacing or (with
    /// `None`) removing variables, and returns the envelope and the exit code.
    fn run(&self, args: &[&str], env: &[(&str, Option<&str>)]) -> (Value, i32) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
        command.arg("--json").args(args).env_clear();
        let mut vars: Vec<(String, String)> = self
            .world
            .env()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        for (key, value) in env {
            vars.retain(|(k, _)| k != key);
            if let Some(value) = value {
                vars.push((key.to_string(), value.to_string()));
            }
        }
        let output = command
            .envs(vars)
            .stdin(Stdio::null())
            .output()
            .expect("run toolportctl");
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "not an envelope ({e}): {:?} / {:?}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (envelope, output.status.code().unwrap_or(-1))
    }

    fn data(&self, args: &[&str], env: &[(&str, Option<&str>)]) -> Value {
        let (envelope, code) = self.run(args, env);
        assert_eq!(code, 0, "{args:?}: {envelope}");
        envelope["data"].clone()
    }

    fn project(&self, name: &str) -> String {
        self.plugins
            .claude_home
            .parent()
            .unwrap()
            .join("work")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    fn edit_json(&self, path: &Path, edit: impl FnOnce(&mut Value)) {
        let mut doc: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        edit(&mut doc);
        std::fs::write(path, doc.to_string()).unwrap();
    }

    fn plant_sentinels(&self) {
        let script = self.sentinel();
        let mark = self.mark();
        exec::write_executable(
            &script,
            &format!("#!/bin/sh\necho ran >> \"{}\"\n", mark.display()),
        );
        let command = script.to_string_lossy().into_owned();
        let hook = json!({"matcher": "Bash", "hooks": [{"type": "command", "command": command}]});
        self.edit_json(&self.plugins.claude_home.join("settings.json"), |doc| {
            doc["hooks"]["PreToolUse"].as_array_mut().unwrap().push(hook.clone());
            doc["hooks"]["SessionStart"] = json!([hook]);
            doc["statusLine"] = json!({"type": "command", "command": command});
        });
        self.edit_json(&self.plugins.plugin.join("hooks/hooks.json"), |doc| {
            doc["hooks"]["PostToolUse"].as_array_mut().unwrap().push(hook.clone());
        });
        self.edit_json(&self.plugins.plugin.join(".mcp.json"), |doc| {
            doc["mcpServers"]["sentinel"] = json!({"command": command, "args": ["--serve"]});
        });
        std::fs::write(
            self.plugins.plugin.join(".lsp.json"),
            json!({"lspServers": {"sentinel": {"command": command}}}).to_string(),
        )
        .unwrap();
        std::fs::write(
            self.plugins.side.join(".claude/settings.json"),
            json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": command}]}]}})
                .to_string(),
        )
        .unwrap();
    }
}

const NO_CLAUDE: &[(&str, Option<&str>)] = &[("TOOLPORT_CLAUDE_BIN", None), ("PATH", Some("/nonexistent"))];

fn hooks_of(data: &Value) -> usize {
    data["hooks"].as_array().unwrap().len()
}

#[test]
fn the_sentinel_is_a_working_positive_control() {
    let fx = Fixture::new("plg-control");
    fx.plant_sentinels();
    assert!(!fx.mark().exists());
    let status = Command::new(fx.sentinel()).status().unwrap();
    assert!(status.success());
    assert!(fx.mark().exists(), "running the sentinel leaves its mark");
}

#[test]
fn no_read_path_starts_a_hook_a_plugin_or_an_mcp_server() {
    let fx = Fixture::new("plg-sentinel");
    fx.plant_sentinels();
    let folders = [fx.project("acme-erp"), fx.project("side-project"), fx.project("quiet")];
    let sets: [&[(&str, Option<&str>)]; 2] = [&[], NO_CLAUDE];
    for env in sets {
        for cwd in &folders {
            fx.data(&["plugins", "ls", "--cwd", cwd], env);
            fx.data(&["plugins", "show", "ecc@ecc", "--cwd", cwd], env);
            fx.data(&["hooks", "ls", "--cwd", cwd], env);
            fx.data(&["hooks", "ls", "--cwd", cwd, "--tool", "Bash", "--owner", "plugin"], env);
        }
        fx.data(&["plugins", "ls"], env);
        fx.data(&["hooks", "ls"], env);
    }
    fx.data(&["plugins", "ls", "--refresh"], &[]);
    assert!(
        !fx.mark().exists(),
        "a hook, plugin or MCP server command was started: {:?}",
        std::fs::read_to_string(fx.mark())
    );
    let listed = fx.data(&["hooks", "ls", "--cwd", &folders[0]], &[]);
    assert!(
        listed["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["command"].as_str().unwrap().ends_with("sentinel.sh")),
        "the sentinel hooks are listed, only as text"
    );
}

#[test]
fn without_claude_the_rows_fall_back_to_files_with_the_documented_nulls() {
    let fx = Fixture::new("plg-files");
    let cwd = fx.project("acme-erp");
    let via_cli = fx.data(&["plugins", "show", "ecc@ecc", "--cwd", &cwd], &[]);
    let via_files = fx.data(&["plugins", "show", "ecc@ecc", "--cwd", &cwd], NO_CLAUDE);
    assert_eq!(via_cli["from"], "claude-cli");
    assert_eq!(via_cli["cost"]["projected"], json!({"basis": "projected", "value": 70}));
    assert_eq!(via_files["from"], "files");
    assert_eq!(via_files["cost"]["projected"], Value::Null);
    assert_eq!(via_files["cost"]["measured"], via_cli["cost"]["measured"]);
    for key in ["id", "version", "brings", "enabled", "update", "mcpOutsideGateway", "components"] {
        assert_eq!(via_files[key], via_cli[key], "{key} does not depend on the CLI");
    }
    let listed = fx.data(&["plugins", "ls"], NO_CLAUDE);
    assert_eq!(listed["plugins"][0]["from"], "files");
    assert_eq!(listed["partial"], false);
}

#[test]
fn project_local_and_managed_settings_decide_the_effective_state() {
    let fx = Fixture::new("plg-enabled");
    let cwd = fx.project("acme-erp");
    let enabled = |fx: &Fixture, env: &[(&str, Option<&str>)]| {
        let data = fx.data(&["plugins", "show", "ecc@ecc", "--cwd", &cwd], env);
        (data["enabled"].clone(), data["warnings"].clone())
    };
    let (state, _) = enabled(&fx, &[]);
    assert_eq!(state, json!({"user": true, "project": null, "local": null, "effective": true}));

    let project = fx.plugins.plain.join(".claude/settings.json");
    fx.edit_json(&project, |doc| doc["enabledPlugins"] = json!({"ecc@ecc": false}));
    let (state, _) = enabled(&fx, &[]);
    assert_eq!(state["project"], false);
    assert_eq!(state["effective"], false, "a project file switches the plugin off");
    let off = fx.data(&["hooks", "ls", "--cwd", &cwd], &[]);
    assert!(off["hooks"].as_array().unwrap().iter().all(|h| h["owner"]["kind"] != "plugin"));

    let local = fx.plugins.plain.join(".claude/settings.local.json");
    std::fs::write(&local, json!({"enabledPlugins": {"ecc@ecc": true}}).to_string()).unwrap();
    let (state, _) = enabled(&fx, &[]);
    assert_eq!(state["local"], true);
    assert_eq!(state["effective"], true, "the local file wins over the project file");

    let managed = fx.world.base.join("managed.json");
    std::fs::write(&managed, json!({"enabledPlugins": {"ecc@ecc": false}}).to_string()).unwrap();
    let (state, warnings) =
        enabled(&fx, &[("TOOLPORT_CLAUDE_MANAGED_SETTINGS", Some(managed.to_str().unwrap()))]);
    assert_eq!(state["effective"], false, "managed settings win");
    assert!(
        warnings.as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("managed")),
        "{warnings}"
    );
}

#[test]
fn the_contract_counts_come_out_of_the_fixture() {
    let fx = Fixture::new("plg-counts");
    let data = fx.data(&["hooks", "ls", "--cwd", &fx.project("acme-erp")], NO_CLAUDE);
    let per_tool = &data["counts"]["perTool"];
    assert_eq!(
        [
            per_tool["Bash"]["total"].as_u64().unwrap(),
            per_tool["Edit"]["total"].as_u64().unwrap(),
            per_tool["Write"]["total"].as_u64().unwrap(),
            per_tool["Read"]["total"].as_u64().unwrap(),
        ],
        [12, 13, 14, 9]
    );
    let owners = &data["counts"]["byOwner"];
    assert_eq!(owners["plugin:ecc@ecc"], 23);
    assert_eq!(owners["user:settings.json"], 26);
    let others = |owner: &str| -> u64 {
        data["counts"]["otherEventsByOwner"][owner]
            .as_object()
            .unwrap()
            .values()
            .map(|n| n.as_u64().unwrap())
            .sum()
    };
    assert_eq!((others("plugin:ecc@ecc"), others("user:settings.json")), (13, 18));
    let conflict = data["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["tools"] == json!(["Bash"]))
        .expect("a conflict on Bash");
    let names: Vec<&str> = conflict["hooks"].as_array().unwrap().iter().map(|h| h.as_str().unwrap()).collect();
    assert!(names.iter().any(|n| n.starts_with("settings.json: gh-write-gate")), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("ecc@ecc: run-with-flags.js pre:bash:dispatcher")), "{names:?}");
    assert_eq!(hooks_of(&data), 50);

    let quiet = fx.data(&["hooks", "ls", "--cwd", &fx.project("quiet")], NO_CLAUDE);
    assert_eq!(quiet["disabledAll"], true);
    assert_eq!(hooks_of(&quiet), 0, "disableAllHooks empties the effective list");
}

fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

fn ecc_effective(fx: &Fixture, cwd: &str) -> Value {
    let data = fx.data(&["plugins", "ls", "--cwd", cwd], NO_CLAUDE);
    data["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "ecc@ecc")
        .expect("ecc@ecc is listed")["enabled"]["effective"]
        .clone()
}

#[test]
fn off_and_on_take_a_plugin_out_of_one_folder_and_bring_it_back() {
    let fx = Fixture::new("plg-off-on");
    let cwd = fx.project("acme-erp");
    let folder = Path::new(&cwd);
    let before = files(folder);
    let user_before = std::fs::read(fx.plugins.claude_home.join("settings.json")).unwrap();
    assert_eq!(ecc_effective(&fx, &cwd), json!(true));
    let hooks_on = hooks_of(&fx.data(&["hooks", "ls", "--cwd", &cwd], NO_CLAUDE));

    let plan = fx.data(&["plugins", "off", "ecc@ecc", "--cwd", &cwd, "--dry-run"], NO_CLAUDE);
    assert_eq!((plan["dryRun"].clone(), plan["scope"].clone()), (json!(true), json!("folder")));
    assert_eq!(files(folder), before, "a dry run writes nothing");

    let done = fx.data(&["plugins", "off", "ecc@ecc", "--cwd", &cwd], NO_CLAUDE);
    assert_eq!(done["result"]["applied"], true);
    let file = folder.join(".claude/settings.local.json");
    let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(written, json!({"enabledPlugins": {"ecc@ecc": false}}));
    assert_eq!(ecc_effective(&fx, &cwd), json!(false));
    assert!(hooks_of(&fx.data(&["hooks", "ls", "--cwd", &cwd], NO_CLAUDE)) < hooks_on, "the plugin's hooks no longer count here");
    assert_eq!(std::fs::read(fx.plugins.claude_home.join("settings.json")).unwrap(), user_before, "user settings are not touched");

    let back = fx.data(&["plugins", "on", "ecc@ecc", "--cwd", &cwd], NO_CLAUDE);
    assert_eq!(back["result"]["applied"], true);
    assert_eq!(files(folder), before, "on puts the folder back");
    assert_eq!(ecc_effective(&fx, &cwd), json!(true));
    assert_eq!(hooks_of(&fx.data(&["hooks", "ls", "--cwd", &cwd], NO_CLAUDE)), hooks_on);

    let (missing, code) = fx.run(&["plugins", "off", "ecc@ecc"], NO_CLAUDE);
    assert_eq!((code, missing["error"]["code"].clone()), (2, json!("usage")));
    let (unknown, code) = fx.run(&["plugins", "on", "nope@nowhere", "--cwd", &cwd], NO_CLAUDE);
    assert_eq!((code, unknown["error"]["code"].clone()), (1, json!("not_found")));
}

#[test]
fn off_and_on_leave_the_foreign_keys_of_the_settings_file_byte_identical() {
    let fx = Fixture::new("plg-off-foreign");
    let cwd = fx.project("acme-erp");
    let file = Path::new(&cwd).join(".claude/settings.local.json");
    let original = "{\n  \"model\": \"opus\",\n  \"enabledPlugins\": {\n    \"mine@own\": true\n  },\n  \"env\": {\n    \"CANARY_ONE\": \"x y\"\n  }\n}\n";
    std::fs::write(&file, original).unwrap();
    fx.data(&["plugins", "off", "ecc@ecc", "--cwd", &cwd], NO_CLAUDE);
    let now: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(now["enabledPlugins"], json!({"mine@own": true, "ecc@ecc": false}));
    assert_eq!((now["model"].clone(), now["env"].clone()), (json!("opus"), json!({"CANARY_ONE": "x y"})));
    fx.data(&["plugins", "on", "ecc@ecc", "--cwd", &cwd], NO_CLAUDE);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);

    fx.data(&["plugins", "off", "ecc@ecc", "--cwd", &cwd], NO_CLAUDE);
    let mut edited: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    edited["enabledPlugins"]["ecc@ecc"] = json!(true);
    std::fs::write(&file, edited.to_string()).unwrap();
    let changed = std::fs::read(&file).unwrap();
    let out = fx.data(&["plugins", "on", "ecc@ecc", "--cwd", &cwd], NO_CLAUDE);
    assert_eq!(out["conflicts"], json!(["enabledPlugins.ecc@ecc"]));
    assert_eq!(std::fs::read(&file).unwrap(), changed, "a changed owned key is left alone");
}

#[test]
fn disable_and_enable_ask_claude_for_the_user_scope_and_nothing_else() {
    let fx = Fixture::new("plg-user-scope");
    let log = fx.plugins.recorded.join("user-scope.log");
    let user_before = std::fs::read(fx.plugins.claude_home.join("settings.json")).unwrap();

    let plan = fx.data(&["plugins", "disable", "ecc@ecc", "--dry-run"], &[]);
    assert_eq!((plan["scope"].clone(), plan["dryRun"].clone()), (json!("user"), json!(true)));
    assert!(!log.exists(), "a dry run does not start claude");

    fx.data(&["plugins", "disable", "ecc@ecc"], &[]);
    fx.data(&["plugins", "enable", "ecc@ecc"], &[]);
    let calls: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().map(String::from).collect();
    assert_eq!(calls, ["plugin disable ecc@ecc --scope user", "plugin enable ecc@ecc --scope user"]);
    assert_eq!(std::fs::read(fx.plugins.claude_home.join("settings.json")).unwrap(), user_before, "Toolport writes no settings file itself");

    let (envelope, code) = fx.run(&["plugins", "disable", "ecc@ecc"], NO_CLAUDE);
    assert_eq!((code, envelope["error"]["code"].clone()), (1, json!("conflict")));
    let (envelope, code) = fx.run(&["plugins", "enable", "nope@nowhere"], &[]);
    assert_eq!((code, envelope["error"]["code"].clone()), (1, json!("not_found")));
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 2);
}
