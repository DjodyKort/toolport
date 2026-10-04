use super::report::{self, Failure, Opts};
use super::{hooks, ClaudeRunner, Env};
use crate::plus::update::exec::CmdOutput;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

struct Fx {
    base: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let raw = std::env::temp_dir().join(format!("plugins-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raw);
        std::fs::create_dir_all(&raw).unwrap();
        Self {
            base: std::fs::canonicalize(&raw).unwrap(),
        }
    }

    fn env(&self) -> Env {
        Env {
            home: self.base.join("home"),
            claude_home: self.base.join("home/.claude"),
            data_dir: Some(self.base.join("data")),
            managed_settings: Some(self.base.join("managed-settings.json")),
            profiles_root: self.base.join("config/claude-profiles"),
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.base.join(rel)
    }

    fn put(&self, rel: &str, body: &str) -> PathBuf {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    }

    fn put_json(&self, rel: &str, body: Value) -> PathBuf {
        self.put(rel, &body.to_string())
    }

    fn user_settings(&self, body: Value) {
        self.put_json("home/.claude/settings.json", body);
    }

    fn project(&self, rel: &str) -> PathBuf {
        let dir = self.path(rel);
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        dir
    }

    /// An installed plugin with two skills, an agent, two commands, three hook handlers and two
    /// MCP servers. `sentinel` is what every command in it points at.
    fn plugin(&self, name: &str, marketplace: &str, sentinel: &str) -> PathBuf {
        let id = format!("{name}@{marketplace}");
        let dir = self.path(&format!("home/.claude/plugins/cache/{marketplace}/{name}/1.0.0"));
        let at = |rel: &str| dir.join(rel);
        let write = |rel: &str, body: String| {
            std::fs::create_dir_all(at(rel).parent().unwrap()).unwrap();
            std::fs::write(at(rel), body).unwrap();
        };
        write(
            ".claude-plugin/plugin.json",
            json!({
                "name": name, "version": "1.0.0", "description": "A synthetic plugin",
                "userConfig": {
                    "hook_profile": {"type": "string", "title": "Hook profile", "description": "d", "default": "standard"},
                    "api_token": {"type": "string", "title": "Token", "description": "t", "sensitive": true},
                    "verbose": {"type": "boolean", "title": "Verbose", "default": true}
                }
            })
            .to_string(),
        );
        for skill in ["alpha", "beta"] {
            write(&format!("skills/{skill}/SKILL.md"), format!("---\nname: {skill}\ndescription: d\n---\nbody\n"));
        }
        write("agents/reviewer.md", "---\nname: reviewer\ndescription: d\n---\nbody\n".into());
        write("commands/go.md", "---\ndescription: d\n---\nbody\n".into());
        write("commands/team/up.md", "---\ndescription: d\n---\nbody\n".into());
        write(
            "hooks/hooks.json",
            json!({"hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": format!("{sentinel} pre-bash"), "timeout": 10}]},
                    {"matcher": "Edit|Write", "hooks": [{"type": "command", "command": format!("{sentinel} pre-edit")}]}
                ],
                "SessionStart": [{"hooks": [{"type": "command", "command": format!("{sentinel} start"), "async": true}]}]
            }})
            .to_string(),
        );
        write(
            ".mcp.json",
            json!({"mcpServers": {
                "chrome-devtools": {"command": sentinel, "args": ["--serve", "--api-key", "abc"], "env": {"SECRET_VALUE": "hunter2"}},
                "remote": {"url": "https://user:pw@example.test/mcp?token=abc"}
            }})
            .to_string(),
        );
        let installed = self.path("home/.claude/plugins/installed_plugins.json");
        let mut doc: Value = std::fs::read_to_string(&installed)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| json!({"version": 2, "plugins": {}}));
        doc["plugins"][&id] = json!([{"scope": "user", "installPath": dir, "version": "1.0.0"}]);
        self.put_json("home/.claude/plugins/installed_plugins.json", doc);
        dir
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    list: Option<Result<CmdOutput, String>>,
    details: String,
    configure: String,
}

fn out(stdout: &str) -> Result<CmdOutput, String> {
    Ok(CmdOutput {
        code: 0,
        stdout: stdout.into(),
        stderr: String::new(),
    })
}

impl ClaudeRunner for Fake {
    fn run(&self, args: &[&str], _t: Duration) -> Result<CmdOutput, String> {
        self.calls.lock().unwrap().push(args.join(" "));
        match args {
            ["plugin", "list", "--json"] => self.list.clone().unwrap_or_else(|| out("[]")),
            ["plugin", "details", _] => out(&self.details),
            ["plugin", "configure", _, "--json"] => out(&self.configure),
            ["plugin", "marketplace", "update"] => out("refreshed"),
            other => Err(format!("unexpected call: {other:?}")),
        }
    }
}

fn row<'a>(data: &'a Value, id: &str) -> &'a Value {
    data["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no row for {id}: {data}"))
}

fn cwd_opts(cwd: &Path) -> Opts<'_> {
    Opts {
        cwd: Some(cwd),
        refresh: false,
    }
}

#[test]
fn files_fallback_reads_the_plugin_folder_and_nulls_the_cli_only_figures() {
    let fx = Fx::new("files");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true}}));
    let data = report::ls(&fx.env(), None, &Opts::default());
    let r = row(&data, "ecc@ecc");
    assert_eq!(r["from"], "files");
    assert_eq!(r["version"], "1.0.0");
    assert_eq!(
        r["brings"],
        json!({"skills": 2, "agents": 1, "commands": 2, "hooks": 3, "mcpServers": 2, "lspServers": 0})
    );
    assert_eq!(r["cost"], json!({"projected": null, "measured": null}));
    assert_eq!(r["enabled"], json!({"user": true, "project": null, "local": null, "effective": true}));
    assert_eq!(data["partial"], false);
    assert_eq!(data["refreshError"], Value::Null);
}

#[test]
fn project_and_local_enabled_plugins_decide_the_effective_state() {
    let fx = Fx::new("layers");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true}}));
    let proj = fx.project("work/app");
    let env = fx.env();
    let effective = |cwd: Option<&Path>| {
        let data = report::ls(&env, None, &Opts { cwd, refresh: false });
        row(&data, "ecc@ecc")["enabled"].clone()
    };
    assert_eq!(effective(Some(&proj))["effective"], true);

    std::fs::write(proj.join(".claude/settings.json"), json!({"enabledPlugins": {"ecc@ecc": false}}).to_string()).unwrap();
    let e = effective(Some(&proj));
    assert_eq!((e["effective"].clone(), e["project"].clone(), e["user"].clone()), (json!(false), json!(false), json!(true)));
    assert_eq!(effective(None)["effective"], true, "without --cwd only the user layer counts");

    std::fs::write(proj.join(".claude/settings.local.json"), json!({"enabledPlugins": {"ecc@ecc": true}}).to_string()).unwrap();
    let e = effective(Some(&proj));
    assert_eq!((e["effective"].clone(), e["local"].clone()), (json!(true), json!(true)));
}

#[test]
fn a_git_root_above_the_folder_is_merged_but_folders_above_it_are_not() {
    let fx = Fx::new("gitroot");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": false}}));
    let root = fx.project("work/repo");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".claude/settings.json"), json!({"enabledPlugins": {"ecc@ecc": true}}).to_string()).unwrap();
    let sub = fx.project("work/repo/pkg/app");
    let above = fx.path("work/.claude");
    std::fs::create_dir_all(&above).unwrap();
    std::fs::write(above.join("settings.local.json"), json!({"enabledPlugins": {"ecc@ecc": false}}).to_string()).unwrap();

    let data = report::ls(&fx.env(), None, &cwd_opts(&sub));
    let e = &row(&data, "ecc@ecc")["enabled"];
    assert_eq!((e["project"].clone(), e["local"].clone(), e["effective"].clone()), (json!(true), Value::Null, json!(true)));
}

#[test]
fn managed_settings_win_and_are_reported_as_a_warning() {
    let fx = Fx::new("managed");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true}}));
    fx.put_json("managed-settings.json", json!({"enabledPlugins": {"ecc@ecc": false}}));
    let data = report::ls(&fx.env(), None, &Opts::default());
    assert_eq!(row(&data, "ecc@ecc")["enabled"]["effective"], false);
    let warnings = data["warnings"].to_string();
    assert!(warnings.contains("managed settings") && warnings.contains("ecc@ecc"), "{warnings}");
}

#[test]
fn the_cli_is_preferred_and_only_fixed_plugin_calls_are_made() {
    let fx = Fx::new("cli");
    let dir = fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true}}));
    let list = json!([{"id": "ecc@ecc", "version": "1.0.0", "scope": "user", "enabled": true, "installPath": dir}]);
    let fake = Fake {
        list: Some(out(&list.to_string())),
        details: "ecc 1.0.0\nProjected token cost\n  Always-on:   ~40,639 tok   added to every session\n".into(),
        configure: json!({
            "schema": {"hook_profile": {"type": "string", "title": "Hook profile", "description": "d", "default": "standard"},
                       "api_token": {"type": "string", "title": "Token", "sensitive": true}},
            "inputs": {"hook_profile": "strict", "api_token": "s3cret-value"},
            "choices": {"hook_profile": ["minimal", "standard", "strict"]},
            "configured": ["hook_profile", "api_token"], "unconfigured": []
        })
        .to_string(),
        ..Fake::default()
    };
    let env = fx.env();
    let data = report::ls(&env, Some(&fake), &Opts { cwd: None, refresh: true });
    let r = row(&data, "ecc@ecc");
    assert_eq!(r["from"], "claude-cli");
    assert_eq!(r["cost"]["projected"], json!({"value": 40639, "basis": "projected"}));
    assert_eq!(data["refreshError"], Value::Null);

    let shown = report::show(&env, Some(&fake), "ecc@ecc", &Opts::default()).unwrap();
    let options = shown["options"].as_array().unwrap();
    let by = |k: &str| options.iter().find(|o| o["key"] == k).unwrap();
    assert_eq!(by("hook_profile")["current"], "strict");
    assert_eq!(by("hook_profile")["choices"], json!(["minimal", "standard", "strict"]));
    assert_eq!(by("api_token")["current"], Value::Null);
    assert_eq!(by("api_token")["configured"], true);
    assert!(!shown.to_string().contains("s3cret-value"));

    for call in fake.calls.lock().unwrap().iter() {
        let parts: Vec<&str> = call.split(' ').collect();
        assert_eq!(parts[0], "plugin", "{call}");
        let fixed = call == "plugin list --json"
            || call == "plugin marketplace update"
            || call.starts_with("plugin details ")
            || (call.starts_with("plugin configure ") && call.ends_with(" --json"));
        assert!(fixed, "{call}");
    }
}

#[test]
fn a_failing_cli_falls_back_to_the_files_with_a_warning() {
    let fx = Fx::new("clifail");
    fx.plugin("ecc", "ecc", "/bin/true");
    let broken = Fake {
        list: Some(Ok(CmdOutput {
            code: 3,
            stdout: String::new(),
            stderr: "boom\n".into(),
        })),
        ..Fake::default()
    };
    let data = report::ls(&fx.env(), Some(&broken), &Opts::default());
    assert_eq!(row(&data, "ecc@ecc")["from"], "files");
    assert_eq!(data["partial"], true);
    assert!(data["warnings"].to_string().contains("boom"));

    let absent = Fake {
        list: Some(Err(super::claude::NOT_FOUND.to_string())),
        ..Fake::default()
    };
    let data = report::ls(&fx.env(), Some(&absent), &Opts::default());
    assert_eq!(row(&data, "ecc@ecc")["from"], "files");
    assert_eq!(data["partial"], false);
    assert_eq!(data["warnings"], json!([]));
}

#[test]
fn show_resolves_a_bare_name_and_reports_unknown_ids() {
    let fx = Fx::new("resolve");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.plugin("ecc", "other", "/bin/true");
    fx.plugin("solo", "ecc", "/bin/true");
    let env = fx.env();
    let shown = report::show(&env, None, "solo", &Opts::default()).unwrap();
    assert_eq!(shown["id"], "solo@ecc");
    assert!(matches!(report::show(&env, None, "ecc", &Opts::default()), Err(Failure::Failed(m)) if m.contains("ambiguous")));
    assert!(matches!(report::show(&env, None, "nope@x", &Opts::default()), Err(Failure::Missing(_))));
}

#[test]
fn plugin_mcp_servers_are_keyed_by_plugin_and_checked_against_the_deny_list() {
    let fx = Fx::new("deny");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true},
        "deniedMcpServers": [{"serverName": "chrome-devtools"}]}));
    let proj = fx.project("work/app");
    let env = fx.env();
    let data = report::ls(&env, None, &cwd_opts(&proj));
    let servers = &row(&data, "ecc@ecc")["mcpOutsideGateway"];
    assert_eq!(servers[0], json!({"key": "plugin:ecc:chrome-devtools", "name": "chrome-devtools", "denied": false}), "the bare name denies nothing");

    std::fs::write(
        proj.join(".claude/settings.local.json"),
        json!({"deniedMcpServers": [{"serverName": "plugin:ecc:chrome-devtools"}]}).to_string(),
    )
    .unwrap();
    let shown = report::show(&env, None, "ecc@ecc", &cwd_opts(&proj)).unwrap();
    let s = &shown["mcpServers"][0];
    assert_eq!(s["key"], "plugin:ecc:chrome-devtools");
    assert_eq!(s["toolPrefix"], "mcp__plugin_ecc_chrome-devtools__");
    assert_eq!(s["denied"], json!({"user": false, "project": false, "local": true, "managed": false}));
    assert_eq!(shown["mcpOutsideGateway"][0]["denied"], true);
    assert_eq!(shown["mcpOutsideGateway"][1]["denied"], false);
}

#[test]
fn secrets_in_plugin_files_never_reach_the_output() {
    let fx = Fx::new("secrets");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true},
        "pluginConfigs": {"ecc@ecc": {"options": {"api_token": "s3cret-value", "hook_profile": "strict"}}}}));
    let shown = report::show(&fx.env(), None, "ecc@ecc", &Opts::default()).unwrap();
    let text = shown.to_string();
    for leak in ["s3cret-value", "hunter2", "abc", "pw@"] {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
    let options = shown["options"].as_array().unwrap();
    let by = |k: &str| options.iter().find(|o| o["key"] == k).unwrap();
    assert_eq!(by("hook_profile")["current"], "strict");
    assert_eq!(by("api_token")["configured"], true);
    assert_eq!(by("api_token")["current"], Value::Null);
    assert_eq!(shown["mcpServers"][0]["command"], json!(["/bin/true", "--serve", "--api-key", "[redacted]"]));
    assert_eq!(shown["mcpServers"][1]["url"], "https://example.test/mcp");
}

#[cfg(unix)]
mod no_process {
    use super::*;

    fn sentinel(fx: &Fx) -> (String, PathBuf) {
        let marker = fx.path("sentinel.ran");
        let script = fx.path("sentinel.sh");
        crate::plus::testutil::exec::write_executable(
            &script,
            &format!("#!/bin/sh\necho ran >> \"{}\"\nexit 9\n", marker.display()),
        );
        (script.display().to_string(), marker)
    }

    #[test]
    fn no_read_path_starts_a_hook_a_plugin_or_an_mcp_server() {
        let fx = Fx::new("sentinel");
        let (script, marker) = sentinel(&fx);
        fx.plugin("ecc", "ecc", &script);
        fx.user_settings(json!({
            "enabledPlugins": {"ecc@ecc": true},
            "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": script}]}]}
        }));
        let proj = fx.project("work/app");
        std::fs::write(
            proj.join(".claude/settings.local.json"),
            json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": script}]}]}}).to_string(),
        )
        .unwrap();
        let env = fx.env();
        let fake = Fake {
            list: Some(out(&json!([{"id": "ecc@ecc", "version": "1.0.0", "scope": "user"}]).to_string())),
            details: "Always-on: ~1 tok".into(),
            configure: "has no options to set".into(),
            ..Fake::default()
        };

        for runner in [None, Some(&fake as &dyn ClaudeRunner)] {
            report::ls(&env, runner, &cwd_opts(&proj));
            report::show(&env, runner, "ecc@ecc", &cwd_opts(&proj)).unwrap();
        }
        let layers = env.layers(Some(&proj));
        let inv = hooks::collect(&env, Some(&proj), &layers);
        assert_eq!(inv.entries.len(), 5, "two plugin PreToolUse, one SessionStart, user, local");
        let _ = hooks::to_value(&inv, &hooks::Filter::default());
        assert!(!marker.exists(), "something ran the sentinel script");
    }
}

#[test]
fn disable_all_hooks_empties_the_effective_list() {
    let fx = Fx::new("disable");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({
        "enabledPlugins": {"ecc@ecc": true},
        "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "/bin/true"}]}]}
    }));
    let proj = fx.project("work/app");
    let env = fx.env();
    let inv = hooks::collect(&env, Some(&proj), &env.layers(Some(&proj)));
    assert_eq!(inv.entries.len(), 4);
    assert!(!inv.disabled_all);

    std::fs::write(proj.join(".claude/settings.local.json"), json!({"disableAllHooks": true}).to_string()).unwrap();
    let inv = hooks::collect(&env, Some(&proj), &env.layers(Some(&proj)));
    assert!(inv.disabled_all && inv.entries.is_empty());
    let data = hooks::to_value(&inv, &hooks::Filter::default());
    assert_eq!(data["hooks"], json!([]));
    assert_eq!(data["counts"]["perTool"]["Bash"]["total"], 0);
    assert!(data["warnings"].to_string().contains("disableAllHooks"));

    fx.put_json("managed-settings.json", json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "/opt/managed.sh"}]}]}}));
    let inv = hooks::collect(&env, Some(&proj), &env.layers(Some(&proj)));
    assert_eq!(inv.entries.len(), 1, "a managed hook survives a local disableAllHooks");
    assert_eq!(inv.entries[0].owner_kind, "managed");
    assert_eq!(inv.entries[0].switch_method, "none");

    fx.put_json("managed-settings.json", json!({"disableAllHooks": true,
        "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "/opt/managed.sh"}]}]}}));
    let inv = hooks::collect(&env, Some(&proj), &env.layers(Some(&proj)));
    assert!(inv.entries.is_empty(), "a managed disableAllHooks turns everything off");
}

#[test]
fn a_plugin_that_is_off_in_the_folder_brings_no_hooks() {
    let fx = Fx::new("off");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true}}));
    let proj = fx.project("work/app");
    let env = fx.env();
    let count = |cwd: &Path| hooks::collect(&env, Some(cwd), &env.layers(Some(cwd))).entries.len();
    assert_eq!(count(&proj), 3);
    std::fs::write(proj.join(".claude/settings.json"), json!({"enabledPlugins": {"ecc@ecc": false}}).to_string()).unwrap();
    assert_eq!(count(&proj), 0);
}

#[test]
fn hooks_are_owned_by_toolport_skills_settings_and_launch_profiles() {
    let fx = Fx::new("owners");
    let guard = "/opt/toolport/toolport-gateway --toolport-guard claude-code";
    let event = "/opt/toolport/toolport-gateway --toolport-hook SessionStart";
    let skill = fx.path("home/.claude/skills/demo/hook.sh").display().to_string();
    let home = fx.path("home").display().to_string();
    fx.user_settings(json!({"hooks": {
        "PreToolUse": [
            {"matcher": "Bash|Read|mcp__.*", "hooks": [{"type": "command", "command": guard}]},
            {"matcher": "Bash", "hooks": [{"type": "command", "command": format!("DEBUG=1 TOKEN=abc {home}/.claude/hooks/gate.sh")}]}
        ],
        "SessionStart": [{"hooks": [{"type": "command", "command": event},
                                    {"type": "command", "command": skill}]}]
    }}));
    fx.put_json("data/mcpm-skills.lock", json!({"version": 1, "skills": {"demo": {"hooks_installed": {"claude-code": [skill]}}}}));
    fx.put_json("config/claude-profiles/light/settings.json", json!({"hooks": {
        "PreCompact": [{"hooks": [{"type": "command", "command": "/opt/toolport/checkpoint.sh"}]}],
        "SessionStart": [{"hooks": [{"type": "command", "command": event}]}]
    }}));
    let env = fx.env();
    let inv = hooks::collect(&env, None, &env.layers(None));
    let by_cmd = |needle: &str| inv.entries.iter().find(|e| e.command.contains(needle)).unwrap();

    let g = by_cmd("--toolport-guard");
    assert_eq!((g.owner_kind, g.marker.as_deref(), g.switch_method), ("toolport", Some("--toolport-guard"), "toolport-command"));
    assert_eq!(g.tools, vec!["Bash", "Read"]);
    assert_eq!(by_cmd("--toolport-hook").owner_kind, "toolport");

    let s = by_cmd("hook.sh");
    assert_eq!((s.owner_kind, s.owner_name.as_str(), s.marker.as_deref(), s.switch_method), ("skill", "demo", Some("hooks_installed"), "skills-sync"));

    let u = by_cmd("gate.sh");
    assert_eq!((u.owner_kind, u.owner_name.as_str(), u.switch_method), ("user", "settings.json", "edit-settings"));
    assert_eq!(u.command, "DEBUG=… TOKEN=… ~/.claude/hooks/gate.sh");
    assert!(u.source.contains("settings.json#hooks.PreToolUse[1].hooks[0]"), "{}", u.source);

    let profile = by_cmd("checkpoint.sh");
    assert_eq!((profile.owner_name.as_str(), profile.active, profile.marker.as_deref()), ("launch-profile:light", false, Some("launch-profile")));
    assert_eq!(
        inv.entries.iter().filter(|e| e.owner_name.starts_with("launch-profile")).count(),
        1,
        "a profile hook that is also in the user settings is not listed twice"
    );

    let data = hooks::to_value(&inv, &hooks::Filter::default());
    assert_eq!(data["counts"]["byOwner"]["toolport:toolport"], 2);
    assert!(data["counts"]["byOwner"].get("toolport:launch-profile:light").is_none(), "inactive entries are not counted");
}

#[test]
fn counts_conflicts_and_filters_follow_the_matchers() {
    let fx = Fx::new("counts");
    fx.plugin("ecc", "ecc", "/opt/ecc/run-with-flags.js");
    fx.user_settings(json!({
        "enabledPlugins": {"ecc@ecc": true},
        "hooks": {"PreToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "~/.claude/hooks/gh-write-gate.sh"}]},
            {"matcher": "Read", "hooks": [{"type": "command", "command": "~/.claude/hooks/read-once.sh"}]},
            {"matcher": "(", "hooks": [{"type": "command", "command": "~/.claude/hooks/broken.sh"}]}
        ]}
    }));
    let env = fx.env();
    let inv = hooks::collect(&env, None, &env.layers(None));
    let data = hooks::to_value(&inv, &hooks::Filter::default());
    assert_eq!(data["counts"]["perTool"]["Bash"], json!({"pre": 2, "post": 0, "total": 2}));
    assert_eq!(data["counts"]["perTool"]["Edit"], json!({"pre": 1, "post": 0, "total": 1}));
    assert_eq!(data["counts"]["perTool"]["Read"], json!({"pre": 1, "post": 0, "total": 1}));
    assert_eq!(data["counts"]["otherEvents"], json!({"SessionStart": 1}));
    assert_eq!(data["counts"]["byOwner"]["plugin:ecc@ecc"], 3);
    assert_eq!(data["counts"]["byOwner"]["user:settings.json"], 3);

    let conflicts = data["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0]["tools"], json!(["Bash"]));
    let labels = conflicts[0]["hooks"].to_string();
    assert!(labels.contains("gh-write-gate.sh") && labels.contains("run-with-flags.js pre-bash"), "{labels}");
    assert!(data["warnings"].to_string().contains("matcher '(' of settings.json"), "{}", data["warnings"]);

    let only_bash = hooks::to_value(&inv, &hooks::Filter { tool: Some("bash".into()), ..Default::default() });
    assert_eq!(only_bash["hooks"].as_array().unwrap().len(), 2);
    let only_plugin = hooks::to_value(&inv, &hooks::Filter { owner: Some("plugin".into()), ..Default::default() });
    assert_eq!(only_plugin["hooks"].as_array().unwrap().len(), 3);
    let only_event = hooks::to_value(&inv, &hooks::Filter { event: Some("sessionstart".into()), ..Default::default() });
    assert_eq!(only_event["hooks"].as_array().unwrap().len(), 1);
    assert!(hooks::Filter { tool: Some("Grep".into()), ..Default::default() }.validate().is_err());
    assert!(hooks::Filter { owner: Some("nope".into()), ..Default::default() }.validate().is_err());
}

#[test]
fn measured_cost_comes_from_the_cached_measurement_of_the_folder() {
    let fx = Fx::new("measured");
    fx.plugin("ecc", "ecc", "/bin/true");
    fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true}}));
    let proj = fx.project("work/app");
    let doc = |cwd: &Path, at: &str, tokens: i64| {
        json!({"cwd": cwd, "measuredAt": at, "cached": true, "runs": [],
               "deltas": [{"label": "without plugin:ecc@ecc", "tokens": tokens, "percent": 1.0}]})
    };
    fx.put_json("data/plus/cache/measure/a.json", doc(&proj, "2026-10-01T00:00:00Z", -8000));
    fx.put_json("data/plus/cache/measure/b.json", doc(&proj, "2026-10-02T00:00:00Z", -8651));
    fx.put_json("data/plus/cache/measure/c.json", doc(&fx.path("elsewhere"), "2026-10-03T00:00:00Z", -1));
    let env = fx.env();
    let data = report::ls(&env, None, &cwd_opts(&proj));
    assert_eq!(row(&data, "ecc@ecc")["cost"]["measured"], json!({"value": 8651, "basis": "measured"}));
    let data = report::ls(&env, None, &Opts::default());
    assert_eq!(row(&data, "ecc@ecc")["cost"]["measured"], Value::Null);
}
