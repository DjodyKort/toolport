use super::config::{self, ConfigArgs, McpOp};
use super::tests::Fx;
use super::{ClaudeRunner, SystemClaude};
use crate::plus::op::{ErrorKind, OpError};
use crate::plus::randutil::{run_cases, Rng};
use crate::plus::testutil::tree_snapshot;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const STUB: &str = include_str!("../../../tests/fixtures/plugins/claude-stub.sh");
const CONFIGURE: &str = include_str!("../../../tests/fixtures/plugins/configure-ecc.json");

struct World {
    fx: Fx,
    cwd: PathBuf,
}

impl World {
    fn new(tag: &str) -> Self {
        let fx = Fx::new(tag);
        let install = fx.plugin("ecc", "ecc", "/bin/true");
        let skill = install.join("skills/ui-check/SKILL.md");
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(&skill, "---\nname: ui-check\ndescription: d\n---\nCall mcp__plugin_ecc_chrome-devtools__take_snapshot first.\n").unwrap();
        let cwd = fx.project("work/app");
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        std::fs::write(cwd.join(".git/info/exclude"), "# local\n").unwrap();
        Self { fx, cwd }
    }

    fn settings(&self) -> PathBuf {
        self.cwd.join(".claude/settings.local.json")
    }

    fn json(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.settings()).unwrap()).unwrap()
    }

    fn config(&self, sets: &[(&str, &str)], unsets: &[&str], cwd: Option<&Path>, dry: bool) -> Result<Value, OpError> {
        let args = ConfigArgs {
            id: "ecc@ecc",
            cwd,
            sets: sets.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            unsets: unsets.iter().map(|k| k.to_string()).collect(),
            dry_run: dry,
        };
        config::config(&self.fx.env(), None, &args)
    }

    fn folder(&self, sets: &[(&str, &str)], unsets: &[&str], dry: bool) -> Value {
        self.config(sets, unsets, Some(&self.cwd), dry).unwrap()
    }

    fn mcp(&self, op: McpOp, server: &str, dry: bool) -> Result<Value, OpError> {
        config::mcp(&self.fx.env(), None, op, "ecc@ecc", server, &self.cwd, dry)
    }
}

fn warnings(data: &Value) -> String {
    data["plan"]["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" | ")
}

#[test]
fn folder_config_plans_writes_and_undoes_through_the_ledger() {
    let w = World::new("cfg-folder");
    let before = tree_snapshot(&w.cwd);
    let plan = w.folder(&[("gateguard", "off"), ("hook_profile", "minimal")], &[], true);
    assert_eq!(plan["dryRun"], true);
    assert_eq!(plan["result"], Value::Null);
    assert!(!w.settings().exists(), "a dry run writes nothing");
    assert!(warnings(&plan).contains("still starts a process"), "{}", warnings(&plan));
    assert_eq!(plan["plan"]["steps"][0]["op"], "create");
    assert_eq!(plan["plan"]["steps"][0]["keys"], json!(["env"]));
    assert!(plan["plan"]["undo"].as_str().unwrap().contains("--unset gateguard"));
    assert_eq!(tree_snapshot(&w.cwd), before);

    let done = w.folder(&[("gateguard", "off"), ("hook_profile", "minimal")], &[], false);
    assert_eq!(done["result"]["applied"], true);
    assert_eq!(w.json(), json!({"env": {"ECC_GATEGUARD": "off", "ECC_HOOK_PROFILE": "minimal"}}));
    let exclude = std::fs::read_to_string(w.cwd.join(".git/info/exclude")).unwrap();
    assert!(exclude.contains(".claude/settings.local.json"));

    let on = w.folder(&[("gateguard", "on")], &[], false);
    assert_eq!(w.json(), json!({"env": {"ECC_HOOK_PROFILE": "minimal"}}), "bool-off on removes the key: {on}");
    w.folder(&[], &["hook_profile"], false);
    assert_eq!(tree_snapshot(&w.cwd), before, "the last unset puts everything back");
}

#[test]
fn unset_only_removes_keys_toolport_owns_and_restores_the_user_value() {
    let w = World::new("cfg-owned");
    std::fs::write(w.settings(), "{\n  \"env\": {\n    \"ECC_HOOK_PROFILE\": \"strict\",\n    \"ECC_GATEGUARD\": \"off\"\n  }\n}\n").unwrap();
    let orig = std::fs::read_to_string(w.settings()).unwrap();
    let plan = w.folder(&[], &["gateguard"], true);
    assert!(warnings(&plan).contains("not set by Toolport"), "{}", warnings(&plan));
    w.folder(&[], &["gateguard"], false);
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), orig, "the user's own key is left alone");
    w.folder(&[("hook_profile", "minimal")], &[], false);
    assert_eq!(w.json()["env"]["ECC_HOOK_PROFILE"], "minimal");
    w.folder(&[], &["hook_profile"], false);
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), orig, "unset restores the replaced value");
}

#[test]
fn a_changed_owned_key_is_a_conflict_and_is_left_alone() {
    let w = World::new("cfg-conflict");
    w.folder(&[("gateguard", "off"), ("hook_profile", "minimal")], &[], false);
    let mut doc = w.json();
    doc["env"]["ECC_HOOK_PROFILE"] = json!("strict");
    std::fs::write(w.settings(), doc.to_string()).unwrap();
    let out = w.folder(&[], &["gateguard"], false);
    assert!(warnings(&out).contains("changed since the last apply"), "{}", warnings(&out));
    assert_eq!(out["conflicts"], json!(["env.ECC_HOOK_PROFILE"]));
    assert_eq!(w.json()["env"], json!({"ECC_HOOK_PROFILE": "strict"}));
}

#[test]
fn list_knobs_replace_a_user_level_value_and_the_plan_says_so() {
    let w = World::new("cfg-csv");
    w.fx.user_settings(json!({"env": {"GATEGUARD_EXEMPT_GLOBS": "docs/**"}}));
    let plan = w.folder(&[("gateguard_exempt_globs", " a/** , b/** ,")], &[], true);
    assert!(warnings(&plan).contains("replaces it, it is not appended"), "{}", warnings(&plan));
    assert!(warnings(&plan).contains("docs/**"));
    w.folder(&[("gateguard_exempt_globs", " a/** , b/** ,")], &[], false);
    assert_eq!(w.json()["env"]["GATEGUARD_EXEMPT_GLOBS"], "a/**,b/**");
}

#[test]
fn refusals_name_the_problem() {
    let w = World::new("cfg-refuse");
    let kind = |r: Result<Value, OpError>| {
        let e = r.unwrap_err();
        (e.kind, e.message)
    };
    let (k, m) = kind(w.config(&[("nope", "1")], &[], Some(&w.cwd), true));
    assert_eq!(k, ErrorKind::Usage);
    assert!(m.contains("no knob 'nope'") && m.contains("hook_profile"), "{m}");
    let (k, m) = kind(w.config(&[("gateguard", "off")], &[], None, true));
    assert_eq!(k, ErrorKind::Usage);
    assert!(m.contains("folder-only") && m.contains("--cwd"), "{m}");
    let (_, m) = kind(w.config(&[("hook_profile", "wild")], &[], Some(&w.cwd), true));
    assert!(m.contains("one of minimal, standard, strict"), "{m}");
    let (_, m) = kind(w.config(&[("gateguard", "maybe")], &[], Some(&w.cwd), true));
    assert!(m.contains("on or off"), "{m}");
    let (_, m) = kind(w.config(&[("disabled_hooks", " , ")], &[], Some(&w.cwd), true));
    assert!(m.contains("--unset"), "{m}");
    let (_, m) = kind(w.config(&[("disabled_hooks", "a\nb")], &[], Some(&w.cwd), true));
    assert!(m.contains("one line"), "{m}");
    let (_, m) = kind(w.config(&[], &[], Some(&w.cwd), true));
    assert!(m.contains("nothing to do"), "{m}");
    let (_, m) = kind(w.config(&[("gateguard", "off")], &["gateguard"], Some(&w.cwd), true));
    assert!(m.contains("more than once"), "{m}");
    let (k, _) = kind(config::config(&w.fx.env(), None, &ConfigArgs { id: "ghost@x", cwd: Some(&w.cwd), sets: vec![("a".into(), "b".into())], unsets: vec![], dry_run: true }));
    assert_eq!(k, ErrorKind::NotFound);
    assert!(!w.settings().exists());
}

fn stub(w: &World) -> SystemClaude {
    let dir = w.fx.path("stub");
    std::fs::create_dir_all(dir.join("stub-data")).unwrap();
    let bin = dir.join("claude");
    std::fs::write(&bin, STUB).unwrap();
    std::fs::write(dir.join("stub-data/configure.json"), CONFIGURE).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    SystemClaude::with_bin(bin.to_string_lossy().into_owned())
}

#[cfg(unix)]
#[test]
fn global_config_sends_values_on_stdin_and_never_in_argv() {
    let w = World::new("cfg-global");
    let claude = stub(&w);
    let args = |dry| ConfigArgs { id: "ecc@ecc", cwd: None, sets: vec![("hook_profile".into(), "strict".into())], unsets: vec![], dry_run: dry };
    let runner: &dyn ClaudeRunner = &claude;
    let plan = config::config(&w.fx.env(), Some(runner), &args(true)).unwrap();
    assert_eq!(plan["scope"], "global");
    assert_eq!(plan["plan"]["steps"][0]["op"], "exec");
    assert!(plan["plan"]["undo"].as_str().unwrap().contains("--set hook_profile=standard"), "{plan}");
    assert!(!w.fx.path("stub/stub-data/stdin.2.json").exists(), "a dry run configures nothing");

    let done = config::config(&w.fx.env(), Some(runner), &args(false)).unwrap();
    assert_eq!(done["result"]["applied"], true);
    let log = std::fs::read_to_string(w.fx.path("stub/stub-data/argv.log")).unwrap();
    assert!(log.lines().any(|l| l == "plugin configure ecc@ecc --values-stdin"), "{log}");
    assert!(!log.contains("strict") && !log.contains("hook_profile"), "values never reach argv: {log}");
    let sent: Vec<PathBuf> = std::fs::read_dir(w.fx.path("stub/stub-data")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().contains("stdin.")).collect();
    assert_eq!(sent.len(), 1);
    let body: Value = serde_json::from_str(&std::fs::read_to_string(&sent[0]).unwrap()).unwrap();
    assert_eq!(body, json!({"hook_profile": "strict"}));
    assert!(!w.settings().exists(), "the global path writes no folder file");

    let unset = ConfigArgs { id: "ecc@ecc", cwd: None, sets: vec![], unsets: vec!["hook_profile".into()], dry_run: true };
    let plan = config::config(&w.fx.env(), Some(runner), &unset).unwrap();
    assert!(warnings(&plan).contains("back to its default (standard)"));
}

#[test]
fn mcp_deny_lists_what_goes_away_and_allow_removes_only_its_own_entry() {
    let w = World::new("mcp");
    std::fs::write(w.settings(), "{\n  \"deniedMcpServers\": [\n    {\"serverName\": \"other\"}\n  ]\n}\n").unwrap();
    let orig = std::fs::read_to_string(w.settings()).unwrap();
    let plan = w.mcp(McpOp::Deny, "chrome-devtools", true).unwrap();
    assert_eq!(plan["serverName"], "plugin:ecc:chrome-devtools");
    assert_eq!(plan["toolPrefix"], "mcp__plugin_ecc_chrome-devtools__");
    let text = warnings(&plan);
    assert!(text.contains("mcp__plugin_ecc_chrome-devtools__*") && text.contains("skill ui-check"), "{text}");
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), orig);

    w.mcp(McpOp::Deny, "chrome-devtools", false).unwrap();
    assert_eq!(w.json()["deniedMcpServers"], json!([{"serverName": "other"}, {"serverName": "plugin:ecc:chrome-devtools"}]));
    let shown = super::report::show(&w.fx.env(), None, "ecc@ecc", &super::report::Opts { cwd: Some(&w.cwd), refresh: false }).unwrap();
    let server = shown["mcpServers"].as_array().unwrap().iter().find(|s| s["name"] == "chrome-devtools").unwrap();
    assert_eq!(server["denied"]["local"], true);
    w.mcp(McpOp::Allow, "chrome-devtools", false).unwrap();
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), orig);

    let again = w.mcp(McpOp::Allow, "chrome-devtools", true).unwrap();
    assert!(warnings(&again).contains("not denied here"));
    let user_made = w.mcp(McpOp::Allow, "other", true).unwrap_err();
    assert_eq!(user_made.kind, ErrorKind::NotFound);
    assert!(user_made.message.contains("chrome-devtools"), "{}", user_made.message);
}

#[test]
fn an_entry_the_user_wrote_is_not_removed_by_allow() {
    let w = World::new("mcp-user");
    std::fs::write(w.settings(), "{\"deniedMcpServers\":[{\"serverName\":\"plugin:ecc:chrome-devtools\"}]}").unwrap();
    let before = std::fs::read_to_string(w.settings()).unwrap();
    let plan = w.mcp(McpOp::Allow, "chrome-devtools", false).unwrap();
    assert!(warnings(&plan).contains("not write"), "{}", warnings(&plan));
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), before);
    let deny = w.mcp(McpOp::Deny, "chrome-devtools", false).unwrap();
    assert!(warnings(&deny).contains("already denied"));
    assert_eq!(std::fs::read_to_string(w.settings()).unwrap(), before);
}

fn word(rng: &mut Rng) -> String {
    rng.pick(&["alpha", "g\u{e9}", "\u{65e5}\u{672c}", "q\"uote", "back\\slash", "x y"]).to_string()
}

fn gen_foreign(rng: &mut Rng) -> String {
    let mut root = serde_json::Map::new();
    for i in 0..rng.range(0, 3) {
        root.insert(format!("other{i}"), json!(word(rng)));
    }
    if rng.chance(70) {
        let mut env = serde_json::Map::new();
        for name in ["FOO_ONE", "FOO_TWO", "ECC_HOOK_PROFILE", "GATEGUARD_EXEMPT_GLOBS"] {
            if rng.chance(50) {
                env.insert(name.into(), json!(word(rng)));
            }
        }
        root.insert("env".into(), json!(env));
    }
    if rng.chance(60) {
        let items: Vec<Value> = ["other", "plugin:ecc:chrome-devtools", "plugin:x:y"].iter().filter(|_| rng.chance(50)).map(|n| json!({"serverName": n})).collect();
        root.insert("deniedMcpServers".into(), Value::Array(items));
    }
    if rng.chance(60) {
        root.insert("permissions".into(), json!({"allow": ["Bash(ls:*)"], "deny": ["Read(./s/**)"]}));
    }
    if rng.chance(50) {
        root.insert("enabledPlugins".into(), json!({"mine@own": rng.chance(50)}));
    }
    let keys: Vec<&String> = root.keys().collect();
    let mut out = String::from("{\n");
    for (i, k) in keys.iter().enumerate() {
        out.push_str(&format!("  {}: {}{}\n", json!(k), serde_json::to_string(&root[*k]).unwrap(), if i + 1 < keys.len() { "," } else { "" }));
    }
    out.push_str("}\n");
    out
}

#[test]
fn config_and_mcp_apply_then_undo_leave_every_foreign_key_byte_identical() {
    run_cases("plugin-controls-property", 60, |_, rng| {
        let w = World::new("prop");
        let foreign = gen_foreign(rng);
        let had_file = rng.chance(85);
        if had_file {
            std::fs::write(w.settings(), &foreign).unwrap();
        }
        let before = tree_snapshot(&w.cwd);
        let original: Option<Value> = had_file.then(|| serde_json::from_str(&foreign).unwrap());

        let mut sets = vec![("gateguard", "off")];
        if rng.chance(60) {
            sets.push(("hook_profile", "strict"));
        }
        if rng.chance(50) {
            sets.push(("gateguard_exempt_globs", "a/**,b/**"));
        }
        w.folder(&sets, &[], false);
        let denied = rng.chance(70);
        if denied {
            w.mcp(McpOp::Deny, "chrome-devtools", false).unwrap();
        }
        if let Some(original) = &original {
            let now = w.json();
            for (key, value) in original.as_object().unwrap() {
                if !["env", "deniedMcpServers"].contains(&key.as_str()) {
                    assert_eq!(&now[key], value, "foreign key {key}");
                }
            }
            for (name, value) in original.get("env").and_then(Value::as_object).into_iter().flatten() {
                if !["ECC_HOOK_PROFILE", "GATEGUARD_EXEMPT_GLOBS"].contains(&name.as_str()) {
                    assert_eq!(&now["env"][name], value, "foreign env {name}");
                }
            }
            for item in original.get("deniedMcpServers").and_then(Value::as_array).into_iter().flatten() {
                assert!(now["deniedMcpServers"].as_array().unwrap().contains(item));
            }
        }
        let order: [&dyn Fn(&World); 2] = [
            &|w| {
                w.folder(&[], &["gateguard", "hook_profile", "gateguard_exempt_globs"], false);
            },
            &|w| {
                let _ = w.mcp(McpOp::Allow, "chrome-devtools", false);
            },
        ];
        let first = rng.below(2);
        order[first](&w);
        order[1 - first](&w);
        assert_eq!(tree_snapshot(&w.cwd), before, "sets {sets:?} denied {denied} had_file {had_file}");
    });
}
