use crate::context::fsx;
use crate::context::plugin_config::{self, ConfigArgs, McpArgs, McpOp};
use crate::context::testkit::{run_cases, tree_snapshot, Rng, ScratchDir};
use crate::op::{ErrorKind, OpError};
use crate::plugins::claude::{ClaudeRunner, NOT_FOUND};
use crate::plugins::inventory::World;
use crate::skills::gitexec::CmdOutput;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const ID: &str = "ecc@ecc";

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

struct Case {
    _scratch: ScratchDir,
    world: World,
    cwd: PathBuf,
}

impl Case {
    fn new(tag: &str) -> Self {
        let scratch = ScratchDir::new(tag);
        let base = fsx::canonical(scratch.path());
        let home = base.join("home");
        let claude = home.join(".claude");
        let plugin = claude.join("plugins/cache/ecc/ecc/1.0.0");
        put(
            &claude.join("plugins/installed_plugins.json"),
            &json!({"version": 2, "plugins": {
                ID: [{"scope": "user", "version": "1.0.0", "installPath": plugin}],
                "twin@one": [{"scope": "user"}],
                "twin@two": [{"scope": "user"}],
                "plain@own": [{"scope": "user"}],
            }})
            .to_string(),
        );
        put(
            &plugin.join(".mcp.json"),
            &json!({"mcpServers": {"chrome": {"command": "npx"}, "other": {"command": "x"}}})
                .to_string(),
        );
        put(
            &plugin.join("skills/browse/SKILL.md"),
            "uses mcp__plugin_ecc_chrome__click",
        );
        put(
            &claude.join("settings.json"),
            &json!({"enabledPlugins": {ID: true}}).to_string(),
        );
        let cwd = base.join("work/app");
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        put(&cwd.join(".git/info/exclude"), "# local\n");
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        Self {
            world: World {
                home,
                claude_home: claude,
                managed: None,
                data_dir: Some(data),
            },
            _scratch: scratch,
            cwd,
        }
    }

    fn settings(&self) -> PathBuf {
        self.cwd.join(".claude/settings.local.json")
    }

    fn json(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.settings()).unwrap()).unwrap()
    }

    fn config(
        &self,
        set: &[(&str, &str)],
        unset: &[&str],
        dry_run: bool,
    ) -> Result<Value, OpError> {
        self.config_in(Some(&self.cwd), set, unset, dry_run, None)
    }

    fn config_in(
        &self,
        cwd: Option<&Path>,
        set: &[(&str, &str)],
        unset: &[&str],
        dry_run: bool,
        runner: Option<&dyn ClaudeRunner>,
    ) -> Result<Value, OpError> {
        let set: Vec<(String, String)> = set
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let unset: Vec<String> = unset.iter().map(|k| k.to_string()).collect();
        plugin_config::run(
            &self.world,
            runner,
            &ConfigArgs {
                id: ID,
                cwd,
                set: &set,
                unset: &unset,
                dry_run,
            },
        )
    }

    fn mcp(&self, op: McpOp, server: &str, dry_run: bool) -> Result<Value, OpError> {
        plugin_config::mcp(
            &self.world,
            None,
            &McpArgs {
                op,
                id: ID,
                server,
                cwd: &self.cwd,
                dry_run,
            },
        )
    }
}

#[derive(Default)]
struct Claude {
    calls: Mutex<Vec<(String, Option<String>)>>,
    fail: bool,
}

impl ClaudeRunner for Claude {
    fn run(&self, args: &[&str], t: Duration) -> Result<CmdOutput, String> {
        self.run_input(args, None, t)
    }

    fn run_input(
        &self,
        args: &[&str],
        input: Option<&str>,
        _: Duration,
    ) -> Result<CmdOutput, String> {
        if args.starts_with(&["plugin", "list"]) {
            return Ok(CmdOutput {
                code: 1,
                ..CmdOutput::default()
            });
        }
        self.calls
            .lock()
            .unwrap()
            .push((args.join(" "), input.map(String::from)));
        Ok(CmdOutput {
            code: i32::from(self.fail),
            stdout: String::new(),
            stderr: if self.fail {
                "refused".into()
            } else {
                String::new()
            },
        })
    }
}

#[test]
fn test01_a_dry_run_names_the_file_and_writes_nothing() {
    let w = Case::new("pc-dry");
    let before = tree_snapshot(&w.cwd);
    let out = w.config(&[("hook_profile", "strict")], &[], true).unwrap();
    assert_eq!(tree_snapshot(&w.cwd), before);
    assert_eq!(out["dryRun"], true);
    assert_eq!(out["result"], Value::Null);
    assert_eq!(out["changes"][0]["env"], "ECC_HOOK_PROFILE");
    assert_eq!(out["changes"][0]["value"], "strict");
    let warnings = out["plan"]["warnings"].to_string();
    assert!(warnings.contains("still starts a process"), "{warnings}");
}

#[test]
fn test02_set_writes_env_keys_and_unset_puts_the_folder_back() {
    let w = Case::new("pc-set");
    let before = tree_snapshot(&w.cwd);
    let out = w
        .config(
            &[("hook_profile", "strict"), ("gateguard", "off")],
            &[],
            false,
        )
        .unwrap();
    assert_eq!(out["result"]["applied"], true);
    assert_eq!(
        w.json()["env"],
        json!({"ECC_HOOK_PROFILE": "strict", "ECC_GATEGUARD": "off"})
    );
    let shown = plugin_config::read(&w.world, None, ID, Some(&w.cwd)).unwrap();
    assert_eq!(shown["writtenByToolport"].as_array().unwrap().len(), 2);
    assert_eq!(
        shown["knobs"][0]["current"],
        json!({"value": "strict", "from": "folder-env"})
    );
    w.config(&[], &["hook_profile", "gateguard"], false)
        .unwrap();
    assert_eq!(tree_snapshot(&w.cwd), before);
}

#[test]
fn test03_a_second_set_keeps_what_the_first_wrote_and_turning_a_bool_off_value_on_removes_it() {
    let w = Case::new("pc-more");
    w.config(&[("hook_profile", "minimal")], &[], false)
        .unwrap();
    w.config(&[("gateguard", "off")], &[], false).unwrap();
    assert_eq!(
        w.json()["env"],
        json!({"ECC_HOOK_PROFILE": "minimal", "ECC_GATEGUARD": "off"})
    );
    w.config(&[("gateguard", "on")], &[], false).unwrap();
    assert_eq!(w.json()["env"], json!({"ECC_HOOK_PROFILE": "minimal"}));
}

#[test]
fn test04_values_and_keys_the_adapter_does_not_allow_are_usage_errors() {
    let w = Case::new("pc-refuse");
    let message = |r: Result<Value, OpError>| {
        let e = r.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Usage);
        e.message
    };
    assert!(message(w.config(&[("nope", "1")], &[], false)).contains("not a knob of ecc@ecc"));
    assert!(message(w.config(&[("hook_profile", "wild")], &[], false)).contains("one of"));
    assert!(message(w.config(&[], &[], false)).contains("nothing to change"));
    assert!(message(w.config(&[("gateguard", "off")], &["gateguard"], false)).contains("both"));
    let outside = plugin_config::run(
        &w.world,
        None,
        &ConfigArgs {
            id: "plain@own",
            cwd: Some(&w.cwd),
            set: &[("x".into(), "1".into())],
            unset: &[],
            dry_run: true,
        },
    );
    assert!(message(outside).contains("has no adapter"));
}

#[test]
fn test05_an_unknown_plugin_is_not_found_and_a_bare_name_must_be_unique() {
    let w = Case::new("pc-id");
    let ghost = plugin_config::read(&w.world, None, "ghost@x", None).unwrap_err();
    assert_eq!(ghost.kind, ErrorKind::NotFound);
    let twin = plugin_config::read(&w.world, None, "twin", None).unwrap_err();
    assert_eq!(twin.kind, ErrorKind::Usage);
    assert!(twin.message.contains("twin@one"));
    assert_eq!(
        plugin_config::read(&w.world, None, "ecc", None).unwrap()["id"],
        ID
    );
    let flag = plugin_config::read(&w.world, None, "--help", None).unwrap_err();
    assert_eq!(flag.kind, ErrorKind::Usage);
}

#[test]
fn test06_without_a_folder_only_plugin_options_are_set_and_the_values_go_in_on_stdin() {
    let w = Case::new("pc-global");
    let claude = Claude::default();
    let folder_only = w
        .config_in(None, &[("gateguard", "off")], &[], false, Some(&claude))
        .unwrap_err();
    assert_eq!(folder_only.kind, ErrorKind::Usage);
    assert!(folder_only.message.contains("--cwd"));
    assert!(claude.calls.lock().unwrap().is_empty());

    let preview = w
        .config_in(
            None,
            &[("hook_profile", "strict")],
            &[],
            true,
            Some(&claude),
        )
        .unwrap();
    assert_eq!(preview["scope"], "global");
    assert!(
        claude.calls.lock().unwrap().is_empty(),
        "a dry run starts nothing"
    );

    let out = w
        .config_in(
            None,
            &[("hook_profile", "strict")],
            &[],
            false,
            Some(&claude),
        )
        .unwrap();
    assert_eq!(out["result"]["applied"], true);
    let calls = claude.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "plugin configure ecc@ecc --values-stdin");
    assert_eq!(calls[0].1.as_deref(), Some(r#"{"hook_profile":"strict"}"#));
    assert!(
        !calls[0].0.contains("strict"),
        "the value is never in the arguments"
    );
}

#[test]
fn test07_a_global_unset_writes_the_default_and_says_so() {
    let w = Case::new("pc-unset");
    let claude = Claude::default();
    let out = w
        .config_in(None, &[], &["hook_profile"], false, Some(&claude))
        .unwrap();
    assert_eq!(out["changes"][0]["value"], "standard");
    assert!(out["plan"]["warnings"].to_string().contains("default"));
    assert_eq!(
        claude.calls.lock().unwrap()[0].1.as_deref(),
        Some(r#"{"hook_profile":"standard"}"#)
    );
}

#[test]
fn test08_a_missing_or_failing_claude_is_a_conflict_and_a_failure() {
    let w = Case::new("pc-claude");
    let none = w
        .config_in(None, &[("hook_profile", "strict")], &[], false, None)
        .unwrap_err();
    assert_eq!(none.kind, ErrorKind::Conflict);
    assert_eq!(none.message, NOT_FOUND);
    let failing = Claude {
        fail: true,
        ..Claude::default()
    };
    let failed = w
        .config_in(
            None,
            &[("hook_profile", "strict")],
            &[],
            false,
            Some(&failing),
        )
        .unwrap_err();
    assert!(failed.message.contains("refused"));
}

#[test]
fn test09_a_list_knob_replaces_the_user_value_in_the_folder_and_the_plan_says_so() {
    let w = Case::new("pc-list");
    put(
        &w.world.claude_home.join("settings.json"),
        &json!({"enabledPlugins": {ID: true}, "env": {"GATEGUARD_EXEMPT_GLOBS": "a/**"}})
            .to_string(),
    );
    let out = w
        .config(&[("gateguard_exempt_globs", "b/**, c/**")], &[], true)
        .unwrap();
    assert_eq!(out["changes"][0]["value"], "b/**,c/**");
    assert!(out["plan"]["warnings"]
        .to_string()
        .contains("replaced, not appended"));
}

#[test]
fn test10_deny_writes_the_plugin_server_name_and_allow_removes_only_that_entry() {
    let w = Case::new("pc-mcp");
    put(
        &w.settings(),
        &json!({"deniedMcpServers": [{"serverName": "mine"}], "model": "m"}).to_string(),
    );
    let before = std::fs::read_to_string(w.settings()).unwrap();
    let preview = w.mcp(McpOp::Deny, "chrome", true).unwrap();
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), before);
    assert_eq!(preview["serverName"], "plugin:ecc:chrome");
    assert_eq!(preview["toolPrefix"], "mcp__plugin_ecc_chrome__*");
    assert!(preview["plan"]["warnings"]
        .to_string()
        .contains("skills/browse"));

    let out = w.mcp(McpOp::Deny, "chrome", false).unwrap();
    assert_eq!(out["changes"][0]["action"], "add");
    assert_eq!(
        w.json()["deniedMcpServers"],
        json!([{"serverName": "mine"}, {"serverName": "plugin:ecc:chrome"}])
    );
    let listed = plugin_config::mcp_read(&w.world, None, ID, Some(&w.cwd)).unwrap();
    assert_eq!(listed["servers"][0]["denied"]["local"], true);
    assert_eq!(listed["servers"][0]["deniedByToolport"], true);
    assert_eq!(listed["servers"][1]["denied"]["local"], false);

    let again = w.mcp(McpOp::Deny, "chrome", false).unwrap();
    assert_eq!(again["changes"][0]["action"], "none");
    w.mcp(McpOp::Allow, "chrome", false).unwrap();
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), before);
}

#[test]
fn test11_a_server_the_plugin_does_not_start_is_not_found_and_allow_without_a_record_changes_nothing(
) {
    let w = Case::new("pc-mcp-ghost");
    let ghost = w.mcp(McpOp::Deny, "ghost", false).unwrap_err();
    assert_eq!(ghost.kind, ErrorKind::NotFound);
    assert!(ghost.message.contains("chrome, other"));
    let before = tree_snapshot(&w.cwd);
    let out = w.mcp(McpOp::Allow, "chrome", false).unwrap();
    assert_eq!(out["changes"][0]["action"], "none");
    assert_eq!(tree_snapshot(&w.cwd), before);
}

#[test]
fn test12_an_entry_written_by_hand_is_left_alone_by_deny_and_allow() {
    let w = Case::new("pc-mcp-hand");
    put(
        &w.settings(),
        &json!({"deniedMcpServers": [{"serverName": "plugin:ecc:chrome"}]}).to_string(),
    );
    let before = std::fs::read_to_string(w.settings()).unwrap();
    let out = w.mcp(McpOp::Deny, "chrome", false).unwrap();
    assert_eq!(out["changes"][0]["action"], "none");
    w.mcp(McpOp::Allow, "chrome", false).unwrap();
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), before);
}

fn gen_foreign(rng: &mut Rng) -> String {
    let mut parts = vec![format!("\"model\": \"{}\"", rng.slug(8))];
    if rng.chance(60) {
        parts.push(format!(
            "\"env\": {{\"{}\": \"x\"}}",
            "OWN_".to_string() + &rng.slug(5).replace('-', "_")
        ));
    }
    if rng.chance(50) {
        parts.push("\"deniedMcpServers\": [{\"serverName\": \"mine\"}]".to_string());
    }
    if rng.chance(50) {
        parts.push(format!(
            "\"permissions\": {{\"allow\": [\"Bash({}:*)\"]}}",
            rng.slug(6)
        ));
    }
    let indent = *rng.pick(&["", "  ", "\t"]);
    format!("{{\n{indent}{}\n}}\n", parts.join(&format!(",\n{indent}")))
}

#[test]
fn config_and_mcp_then_undo_leave_every_foreign_key_byte_identical() {
    run_cases("plugin-config-foreign", 24, |_seed, rng| {
        let w = Case::new("pc-prop");
        let text = gen_foreign(rng);
        put(&w.settings(), &text);
        let before = tree_snapshot(&w.cwd);
        w.config(
            &[("hook_profile", "strict"), ("gateguard", "off")],
            &[],
            false,
        )
        .unwrap();
        w.mcp(McpOp::Deny, "chrome", false).unwrap();
        w.mcp(McpOp::Deny, "other", false).unwrap();
        assert_eq!(w.json()["env"]["ECC_HOOK_PROFILE"], "strict");
        w.mcp(McpOp::Allow, "other", false).unwrap();
        w.config(&[], &["gateguard"], false).unwrap();
        w.mcp(McpOp::Allow, "chrome", false).unwrap();
        w.config(&[], &["hook_profile"], false).unwrap();
        assert_eq!(tree_snapshot(&w.cwd), before);
        assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), text);
    });
}

#[test]
fn a_changed_owned_key_is_a_conflict_and_is_left_alone() {
    let w = Case::new("pc-conflict");
    w.config(&[("hook_profile", "strict")], &[], false).unwrap();
    let mut doc = w.json();
    doc["env"]["ECC_HOOK_PROFILE"] = json!("hand-edited");
    put(&w.settings(), &serde_json::to_string_pretty(&doc).unwrap());
    let out = w.config(&[], &["hook_profile"], false).unwrap();
    assert!(!out["conflicts"].as_array().unwrap().is_empty());
    assert_eq!(w.json()["env"]["ECC_HOOK_PROFILE"], "hand-edited");
}
