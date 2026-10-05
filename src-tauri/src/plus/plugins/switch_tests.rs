use super::switch::{folder_switch, off_on, user_switch, Switch, UserOp};
use super::tests::Fx;
#[cfg(unix)]
use super::SystemClaude;
use crate::plus::context::bundle_apply::{self, World};
use crate::plus::context::bundle_ledger as ledger;
use crate::plus::context::Roots;
use crate::plus::op::{ErrorKind, OpError};
use crate::plus::randutil::{run_cases, Rng};
use crate::plus::testutil::tree_snapshot;
use serde_json::{json, Value};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(unix)]
const STUB: &str = include_str!("../../../tests/fixtures/plugins/claude-stub.sh");

struct Case {
    fx: Fx,
    cwd: PathBuf,
}

impl Case {
    fn new(tag: &str) -> Self {
        let fx = Fx::new(tag);
        fx.plugin("ecc", "ecc", "/bin/true");
        fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": true}}));
        let cwd = fx.project("work/app");
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        std::fs::write(cwd.join(".git/info/exclude"), "# local\n").unwrap();
        Self { fx, cwd }
    }

    fn settings(&self) -> PathBuf {
        self.cwd.join(".claude/settings.local.json")
    }

    fn text(&self) -> String {
        std::fs::read_to_string(self.settings()).unwrap()
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.text()).unwrap()
    }

    fn switch(&self, op: Switch, dry: bool) -> Result<Value, OpError> {
        folder_switch(&self.fx.env(), op, "ecc@ecc", &self.cwd, dry)
    }

    fn ok(&self, op: Switch, dry: bool) -> Value {
        self.switch(op, dry).unwrap()
    }

    fn ledger(&self) -> ledger::Ledger {
        ledger::load(&self.fx.path("data"))
    }
}

fn warnings(data: &Value) -> String {
    data["plan"]["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" | ")
}

fn notes(data: &Value) -> String {
    data["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["op"] == "note")
        .filter_map(|s| s["detail"].as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn off_previews_what_goes_away_and_on_puts_the_folder_back() {
    let w = Case::new("sw-roundtrip");
    let before = tree_snapshot(&w.cwd);
    let plan = w.ok(Switch::Off, true);
    assert_eq!((plan["scope"].as_str(), plan["dryRun"].clone(), plan["result"].clone()), (Some("folder"), json!(true), Value::Null));
    assert_eq!(plan["id"], "ecc@ecc");
    assert_eq!(plan["plan"]["steps"][0]["op"], "create");
    assert_eq!(plan["plan"]["steps"][0]["keys"], json!(["enabledPlugins"]));
    assert_eq!(plan["changes"], json!([{"key": "enabledPlugins.ecc@ecc", "action": "set", "value": false}]));
    let text = notes(&plan);
    assert!(text.contains("goes away in this folder: 2 skills (alpha, beta), 1 agent (reviewer), 2 commands"), "{text}");
    assert!(text.contains("3 hook handlers") && text.contains("2 MCP servers (plugin:ecc:chrome-devtools, plugin:ecc:remote)"), "{text}");
    assert!(text.contains("stays: your own skills") && text.contains("user settings (on) is not changed"), "{text}");
    assert!(plan["plan"]["undo"].as_str().unwrap().starts_with("toolportctl plugins on ecc@ecc --cwd "));
    assert_eq!(tree_snapshot(&w.cwd), before, "a dry run writes nothing");
    assert_eq!(w.ledger(), ledger::Ledger::default());

    let done = w.ok(Switch::Off, false);
    assert_eq!(done["result"]["applied"], true);
    assert_eq!(w.json(), json!({"enabledPlugins": {"ecc@ecc": false}}));
    assert!(std::fs::read_to_string(w.cwd.join(".git/info/exclude")).unwrap().contains(".claude/settings.local.json"));
    assert!(done["ledger"].as_str().unwrap().ends_with(".json"));
    let shown = super::report::show(&w.fx.env(), None, "ecc@ecc", &super::report::Opts { cwd: Some(&w.cwd), refresh: false }).unwrap();
    assert_eq!(shown["enabled"]["local"], false);
    assert_eq!(shown["enabled"]["effective"], false);

    let again = w.ok(Switch::Off, false);
    assert!(warnings(&again).contains("already turned off"), "{}", warnings(&again));
    assert_eq!(again["changes"][0]["action"], "none");
    assert_eq!(again["result"]["changed"], json!([]));

    let preview = w.ok(Switch::On, true);
    assert!(notes(&preview).contains("comes back in this folder: 2 skills"), "{}", notes(&preview));
    assert!(preview["plan"]["undo"].as_str().unwrap().starts_with("toolportctl plugins off ecc@ecc --cwd "));
    assert_eq!(preview["changes"], json!([{"key": "enabledPlugins.ecc@ecc", "action": "remove", "value": null}]));
    let back = w.ok(Switch::On, false);
    assert_eq!(back["result"]["applied"], true);
    assert_eq!(tree_snapshot(&w.cwd), before, "on puts everything back, the file Toolport created included");
    assert_eq!(w.ledger(), ledger::Ledger::default());
}

#[test]
fn on_restores_a_value_the_user_had_and_keeps_other_plugins_alone() {
    let w = Case::new("sw-restore");
    let original = "{\n  \"enabledPlugins\": {\n    \"mine@own\": true,\n    \"ecc@ecc\": true\n  },\n  \"model\": \"opus\"\n}\n";
    std::fs::write(w.settings(), original).unwrap();
    w.ok(Switch::Off, false);
    assert_eq!(w.json(), json!({"enabledPlugins": {"mine@own": true, "ecc@ecc": false}, "model": "opus"}));
    let preview = w.ok(Switch::On, true);
    assert_eq!(preview["changes"], json!([{"key": "enabledPlugins.ecc@ecc", "action": "restore", "value": true}]));
    w.ok(Switch::On, false);
    assert_eq!(w.text(), original);
}

#[test]
fn a_changed_owned_key_is_a_conflict_and_nothing_is_written() {
    let w = Case::new("sw-conflict");
    w.ok(Switch::Off, false);
    std::fs::write(w.settings(), "{\"enabledPlugins\": {\"ecc@ecc\": true}, \"model\": \"x\"}").unwrap();
    let changed = w.text();

    let off = w.ok(Switch::Off, false);
    assert_eq!(off["conflicts"], json!(["enabledPlugins.ecc@ecc"]));
    assert!(warnings(&off).contains("was changed since Toolport turned the plugin off here"), "{}", warnings(&off));
    assert_eq!(off["changes"][0]["action"], "none");
    assert_eq!(w.text(), changed);

    let on = w.ok(Switch::On, false);
    assert_eq!(on["conflicts"], json!(["enabledPlugins.ecc@ecc"]));
    assert_eq!(on["changes"][0]["action"], "none");
    assert!(!on["result"]["changed"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().ends_with("settings.local.json")));
    assert_eq!(w.text(), changed, "the user's value stays");
    assert_eq!(w.ledger(), ledger::Ledger::default(), "the record is dropped");
}

#[test]
fn an_entry_toolport_did_not_write_is_left_alone() {
    let w = Case::new("sw-foreign");
    std::fs::write(w.settings(), "{\"enabledPlugins\":{\"ecc@ecc\":false}}").unwrap();
    let orig = w.text();
    let off = w.ok(Switch::Off, false);
    assert!(warnings(&off).contains("already false") && warnings(&off).contains("did not write"), "{}", warnings(&off));
    assert_eq!(off["ledger"], Value::Null);
    let on = w.ok(Switch::On, false);
    assert!(warnings(&on).contains("did not write"), "{}", warnings(&on));
    assert_eq!(w.text(), orig);
    assert_eq!(w.ledger(), ledger::Ledger::default());
    let none = Case::new("sw-none");
    let on = none.ok(Switch::On, true);
    assert!(warnings(&on).contains("nothing to remove"), "{}", warnings(&on));
}

#[test]
fn a_plugin_that_is_off_in_the_user_settings_is_said_so_and_not_changed() {
    let w = Case::new("sw-user-off");
    w.fx.user_settings(json!({"enabledPlugins": {"ecc@ecc": false}}));
    let user_before = std::fs::read_to_string(w.fx.path("home/.claude/settings.json")).unwrap();
    let plan = w.ok(Switch::Off, true);
    assert!(warnings(&plan).contains("not on in your user or project settings"), "{}", warnings(&plan));
    assert!(notes(&plan).contains("user settings (off) is not changed"), "{}", notes(&plan));
    w.ok(Switch::Off, false);
    let on = w.ok(Switch::On, true);
    assert!(warnings(&on).contains("stays off here after this"), "{}", warnings(&on));
    assert_eq!(std::fs::read_to_string(w.fx.path("home/.claude/settings.json")).unwrap(), user_before);
}

#[test]
fn off_and_on_refuse_what_they_cannot_do() {
    let w = Case::new("sw-refuse");
    let kind = |r: Result<Value, OpError>| {
        let e = r.unwrap_err();
        (e.kind, e.message)
    };
    let env = w.fx.env();
    let (k, _) = kind(folder_switch(&env, Switch::Off, "ghost@x", &w.cwd, true));
    assert_eq!(k, ErrorKind::NotFound);
    let (k, m) = kind(folder_switch(&env, Switch::Off, "bad id", &w.cwd, true));
    assert_eq!(k, ErrorKind::Usage);
    assert!(m.contains("invalid plugin id"), "{m}");
    let (k, m) = kind(folder_switch(&env, Switch::On, "ecc@ecc", &w.fx.path("nope"), true));
    assert_eq!(k, ErrorKind::Usage);
    assert!(m.contains("not a folder"), "{m}");
    let (k, m) = kind(off_on(Switch::Off, "ecc@ecc", None, true));
    assert_eq!(k, ErrorKind::Usage);
    assert!(m.starts_with("cwd is required"), "{m}");
    assert!(!w.settings().exists());
    assert_eq!(w.ledger(), ledger::Ledger::default());
}

const HAND: &str = "format: 1\nname: hand\nskills:\n  off: [scratch-1]\nplugins:\n  off: [\"ecc@ecc\"]\n";

#[test]
fn a_bundle_and_an_off_control_own_the_same_key_and_leave_in_either_order() {
    for had_file in [false, true] {
        for control_first in [true, false] {
            for bundle_leaves_first in [true, false] {
                let w = Case::new("sw-handover");
                let roots = Roots::from_home(&w.fx.path("home"));
                let profile = roots.skills_repo_path().join("profiles/hand.yaml");
                std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
                std::fs::write(&profile, HAND).unwrap();
                if had_file {
                    std::fs::write(w.settings(), "{\n  \"model\": \"opus\",\n  \"enabledPlugins\": {\n    \"mine@own\": true\n  }\n}\n").unwrap();
                }
                let before = tree_snapshot(&w.cwd);
                let data = w.fx.path("data");
                let world = World { roots: &roots, data_dir: &data };
                let apply_bundle = || bundle_apply::apply(&world, "hand", &w.cwd, false).unwrap();
                if control_first {
                    w.ok(Switch::Off, false);
                    apply_bundle();
                } else {
                    apply_bundle();
                    w.ok(Switch::Off, false);
                }
                let key = |w: &Case| w.json()["enabledPlugins"]["ecc@ecc"].clone();
                assert_eq!(key(&w), json!(false));
                let tag = format!("had_file {had_file} control_first {control_first} bundle_leaves_first {bundle_leaves_first}");
                if bundle_leaves_first {
                    bundle_apply::undo(&world, &w.cwd, false).unwrap();
                    if control_first {
                        assert_eq!(key(&w), json!(false), "the control still owns the key: {tag}");
                    }
                    w.ok(Switch::On, false);
                } else {
                    w.ok(Switch::On, false);
                    if !control_first {
                        assert_eq!(key(&w), json!(false), "the bundle still owns the key: {tag}");
                    }
                    bundle_apply::undo(&world, &w.cwd, false).unwrap();
                }
                assert_eq!(tree_snapshot(&w.cwd), before, "{tag}");
                assert_eq!(w.ledger(), ledger::Ledger::default(), "{tag}");
            }
        }
    }
}

#[cfg(unix)]
fn stub(w: &Case) -> (SystemClaude, PathBuf) {
    let dir = w.fx.path("stub");
    std::fs::create_dir_all(dir.join("stub-data")).unwrap();
    let bin = dir.join("claude");
    std::fs::write(&bin, STUB).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    (SystemClaude::with_bin(bin.to_string_lossy().into_owned()), dir.join("stub-data"))
}

#[cfg(unix)]
fn calls(data: &Path) -> Vec<String> {
    std::fs::read_to_string(data.join("argv.log")).unwrap_or_default().lines().map(String::from).collect()
}

#[cfg(unix)]
#[test]
fn disable_and_enable_run_exactly_the_user_scope_command_and_nothing_else() {
    let w = Case::new("sw-user");
    let (claude, data) = stub(&w);
    let env = w.fx.env();
    let user_before = std::fs::read_to_string(w.fx.path("home/.claude/settings.json")).unwrap();

    let plan = user_switch(&env, &claude, UserOp::Disable, "ecc@ecc", true).unwrap();
    assert_eq!((plan["scope"].as_str(), plan["cwd"].clone(), plan["dryRun"].clone()), (Some("user"), Value::Null, json!(true)));
    assert_eq!(plan["plan"]["steps"][0]["op"], "exec");
    assert_eq!(plan["plan"]["steps"][0]["detail"], "claude plugin disable ecc@ecc --scope user");
    assert!(warnings(&plan).contains("every project") && warnings(&plan).contains("no setting of its own"), "{}", warnings(&plan));
    assert_eq!(plan["plan"]["undo"], "toolportctl plugins enable ecc@ecc");
    assert_eq!(plan["changes"][0]["command"], json!(["claude", "plugin", "disable", "ecc@ecc", "--scope", "user"]));
    assert!(calls(&data).is_empty(), "a dry run asks claude nothing");

    let done = user_switch(&env, &claude, UserOp::Disable, "ecc@ecc", false).unwrap();
    assert_eq!(done["result"]["applied"], true);
    assert_eq!(done["result"]["changed"], json!(["claude plugin disable ecc@ecc --scope user"]));
    assert_eq!(calls(&data), ["plugin disable ecc@ecc --scope user"]);

    let enabled = user_switch(&env, &claude, UserOp::Enable, "ecc@ecc", false).unwrap();
    assert_eq!(enabled["plan"]["undo"], "toolportctl plugins disable ecc@ecc");
    assert_eq!(calls(&data), ["plugin disable ecc@ecc --scope user", "plugin enable ecc@ecc --scope user"]);
    assert_eq!(std::fs::read_to_string(data.join("count")).unwrap().trim(), "2");
    assert_eq!(std::fs::read_to_string(w.fx.path("home/.claude/settings.json")).unwrap(), user_before, "Toolport itself writes no settings file");
    assert!(!w.settings().exists());
}

#[cfg(unix)]
#[test]
fn a_failing_or_missing_claude_and_an_unknown_plugin_are_refused_before_anything_runs() {
    let w = Case::new("sw-user-fail");
    let (claude, data) = stub(&w);
    let env = w.fx.env();
    let e = user_switch(&env, &claude, UserOp::Disable, "ghost@x", false).unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
    let e = user_switch(&env, &claude, UserOp::Enable, "bad id", false).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage);
    assert!(calls(&data).is_empty());

    std::fs::write(data.join("fail"), "").unwrap();
    let e = user_switch(&env, &claude, UserOp::Disable, "ecc@ecc", false).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Conflict);
    assert_eq!(e.message, "plugin is managed by policy");
    assert_eq!(calls(&data), ["plugin disable ecc@ecc --scope user"]);

    let missing = SystemClaude::with_bin(w.fx.path("nowhere/claude").to_string_lossy().into_owned());
    for dry in [true, false] {
        let e = user_switch(&env, &missing, UserOp::Enable, "ecc@ecc", dry).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Conflict);
        assert!(e.message.starts_with("claude not found on PATH"), "{}", e.message);
    }
    assert_eq!(calls(&data).len(), 1);
}

fn gen_foreign(rng: &mut Rng) -> String {
    let word = |rng: &mut Rng| rng.pick(&["alpha", "g\u{e9}", "\u{65e5}\u{672c}", "q\"uote", "back\\slash", "x y"]).to_string();
    let mut root = serde_json::Map::new();
    for i in 0..rng.range(0, 3) {
        root.insert(format!("other{i}"), json!(word(rng)));
    }
    if rng.chance(60) {
        root.insert("env".into(), json!({"FOO_ONE": word(rng), "ECC_HOOK_PROFILE": word(rng)}));
    }
    if rng.chance(50) {
        root.insert("deniedMcpServers".into(), json!([{"serverName": "other"}]));
    }
    if rng.chance(60) {
        root.insert("permissions".into(), json!({"allow": ["Bash(ls:*)"], "deny": ["Read(./s/**)"]}));
    }
    if rng.chance(70) {
        let mut plugins = serde_json::Map::new();
        for name in ["mine@own", "other@market", "ecc@ecc"] {
            if rng.chance(45) {
                plugins.insert(name.into(), json!(rng.chance(50)));
            }
        }
        root.insert("enabledPlugins".into(), Value::Object(plugins));
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
fn off_then_on_leave_every_foreign_key_byte_identical_and_a_changed_key_is_a_conflict() {
    run_cases("plugin-switch-property", 80, |_, rng| {
        let w = Case::new("sw-prop");
        let foreign = gen_foreign(rng);
        let had_file = rng.chance(85);
        if had_file {
            std::fs::write(w.settings(), &foreign).unwrap();
        }
        let original: Option<Value> = had_file.then(|| serde_json::from_str(&foreign).unwrap());
        let before = tree_snapshot(&w.cwd);
        let already_off = original.as_ref().is_some_and(|o| o["enabledPlugins"]["ecc@ecc"] == json!(false));

        w.ok(Switch::Off, rng.chance(30));
        w.ok(Switch::Off, false);
        if rng.chance(30) {
            w.ok(Switch::Off, false);
        }
        let now = w.json();
        assert_eq!(now["enabledPlugins"]["ecc@ecc"], json!(false));
        if let Some(original) = &original {
            for (key, value) in original.as_object().unwrap() {
                if key != "enabledPlugins" {
                    assert_eq!(&now[key], value, "foreign key {key}");
                }
            }
            for (name, value) in original.get("enabledPlugins").and_then(Value::as_object).into_iter().flatten() {
                if name != "ecc@ecc" {
                    assert_eq!(&now["enabledPlugins"][name], value, "foreign plugin {name}");
                }
            }
        }

        let flipped = !already_off && rng.chance(25);
        if flipped {
            let mut doc = w.json();
            doc["enabledPlugins"]["ecc@ecc"] = json!(true);
            std::fs::write(w.settings(), doc.to_string()).unwrap();
            let changed = w.text();
            let preview = w.ok(Switch::On, true);
            assert_eq!(preview["conflicts"], json!(["enabledPlugins.ecc@ecc"]));
            let out = w.ok(Switch::On, false);
            assert_eq!(out["conflicts"], json!(["enabledPlugins.ecc@ecc"]));
            assert_eq!(w.text(), changed, "a changed owned key is left alone, nothing is written");
        } else {
            let out = w.ok(Switch::On, false);
            assert_eq!(out["conflicts"], json!([]));
            assert_eq!(tree_snapshot(&w.cwd), before, "foreign {foreign:?} had_file {had_file}");
        }
        assert_eq!(w.ledger(), ledger::Ledger::default());
    });
}
