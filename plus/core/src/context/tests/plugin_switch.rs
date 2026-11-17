use crate::context::bundle_apply::{self, World as BundleWorld};
use crate::context::bundle_ledger as ledger;
use crate::context::fsx;
use crate::context::loads::NoLoads;
use crate::context::plugin_switch::{self, Args, Switch, World};
use crate::context::testkit::{run_cases, tree_snapshot, Rng, ScratchDir};
use crate::context::Roots;
use crate::op::{ErrorKind, OpError};
use serde_json::{json, Value};
use std::path::PathBuf;

const ID: &str = "ecc@ecc";

struct Case {
    _scratch: ScratchDir,
    roots: Roots,
    home: PathBuf,
    data: PathBuf,
    cwd: PathBuf,
}

fn put(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

impl Case {
    fn new(tag: &str) -> Self {
        let scratch = ScratchDir::new(tag);
        let base = fsx::canonical(scratch.path());
        let home = base.join("home");
        put(
            &home.join(".claude/plugins/installed_plugins.json"),
            &json!({"version": 2, "plugins": {ID: [{"scope": "user"}], "mine@own": [{}]}})
                .to_string(),
        );
        put(
            &home.join(".claude/settings.json"),
            &json!({"enabledPlugins": {ID: true}}).to_string(),
        );
        let cwd = base.join("work/app");
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        put(&cwd.join(".git/info/exclude"), "# local\n");
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        Self {
            roots: Roots::from_home(&home),
            _scratch: scratch,
            home,
            data,
            cwd,
        }
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

    fn switch(&self, op: Switch, dry_run: bool) -> Result<Value, OpError> {
        self.switch_id(op, ID, dry_run)
    }

    fn switch_id(&self, op: Switch, id: &str, dry_run: bool) -> Result<Value, OpError> {
        plugin_switch::run(
            &World {
                claude_home: &self.home.join(".claude"),
                data_dir: &self.data,
            },
            &Args {
                op,
                id,
                cwd: &self.cwd,
                dry_run,
            },
        )
    }

    fn ok(&self, op: Switch, dry_run: bool) -> Value {
        self.switch(op, dry_run).unwrap()
    }

    fn ledger(&self) -> ledger::Ledger {
        ledger::load(&self.data)
    }
}

fn warnings(data: &Value) -> String {
    data["plan"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn off_previews_and_on_puts_the_folder_back() {
    let w = Case::new("sw-roundtrip");
    let before = tree_snapshot(&w.cwd);
    let plan = w.ok(Switch::Off, true);
    assert_eq!(plan["scope"], "folder");
    assert_eq!(plan["dryRun"], true);
    assert_eq!(plan["result"], Value::Null);
    assert_eq!(plan["id"], ID);
    assert_eq!(plan["plan"]["steps"][0]["op"], "create");
    assert_eq!(plan["plan"]["steps"][0]["keys"], json!(["enabledPlugins"]));
    assert_eq!(
        plan["changes"],
        json!([{"key": "enabledPlugins.ecc@ecc", "action": "set", "value": false}])
    );
    assert!(plan["plan"]["undo"]
        .as_str()
        .unwrap()
        .starts_with("toolportctl plugins on ecc@ecc --cwd "));
    assert_eq!(tree_snapshot(&w.cwd), before, "a dry run writes nothing");
    assert_eq!(w.ledger(), ledger::Ledger::default());

    let done = w.ok(Switch::Off, false);
    assert_eq!(done["result"]["applied"], true);
    assert_eq!(w.json(), json!({"enabledPlugins": {ID: false}}));
    assert!(std::fs::read_to_string(w.cwd.join(".git/info/exclude"))
        .unwrap()
        .contains(".claude/settings.local.json"));
    assert!(w
        .ledger()
        .controls
        .values()
        .any(|m| m.contains_key("off:ecc@ecc")));

    let again = w.ok(Switch::Off, false);
    assert!(warnings(&again).contains("already turned off"));
    assert_eq!(again["changes"][0]["action"], "none");
    assert_eq!(again["result"]["changed"], json!([]));

    let preview = w.ok(Switch::On, true);
    assert_eq!(
        preview["changes"],
        json!([{"key": "enabledPlugins.ecc@ecc", "action": "remove", "value": null}])
    );
    let back = w.ok(Switch::On, false);
    assert_eq!(back["result"]["applied"], true);
    assert_eq!(
        tree_snapshot(&w.cwd),
        before,
        "on puts everything back, the file Toolport created included"
    );
    assert_eq!(w.ledger(), ledger::Ledger::default());
}

#[test]
fn on_restores_a_value_the_user_had_and_keeps_other_plugins_alone() {
    let w = Case::new("sw-restore");
    let original = "{\n  \"enabledPlugins\": {\n    \"mine@own\": true,\n    \"ecc@ecc\": true\n  },\n  \"model\": \"opus\"\n}\n";
    put(&w.settings(), original);
    w.ok(Switch::Off, false);
    assert_eq!(
        w.json(),
        json!({"enabledPlugins": {"mine@own": true, ID: false}, "model": "opus"})
    );
    let preview = w.ok(Switch::On, true);
    assert_eq!(
        preview["changes"],
        json!([{"key": "enabledPlugins.ecc@ecc", "action": "restore", "value": true}])
    );
    w.ok(Switch::On, false);
    assert_eq!(w.text(), original);
}

#[test]
fn a_changed_owned_key_is_a_conflict_and_nothing_is_written() {
    let w = Case::new("sw-conflict");
    w.ok(Switch::Off, false);
    put(
        &w.settings(),
        "{\"enabledPlugins\": {\"ecc@ecc\": true}, \"model\": \"x\"}",
    );
    let changed = w.text();

    let off = w.ok(Switch::Off, false);
    assert_eq!(off["conflicts"], json!(["enabledPlugins.ecc@ecc"]));
    assert!(warnings(&off).contains("was changed since Toolport turned the plugin off here"));
    assert_eq!(off["changes"][0]["action"], "none");
    assert_eq!(w.text(), changed);

    let on = w.ok(Switch::On, false);
    assert_eq!(on["conflicts"], json!(["enabledPlugins.ecc@ecc"]));
    assert_eq!(on["changes"][0]["action"], "none");
    assert_eq!(w.text(), changed, "the user's value stays");
    assert_eq!(
        w.ledger(),
        ledger::Ledger::default(),
        "the record is dropped"
    );
}

#[test]
fn an_entry_toolport_did_not_write_is_left_alone() {
    let w = Case::new("sw-foreign");
    put(&w.settings(), "{\"enabledPlugins\":{\"ecc@ecc\":false}}");
    let original = w.text();
    let off = w.ok(Switch::Off, false);
    assert!(warnings(&off).contains("already false") && warnings(&off).contains("did not write"));
    assert_eq!(off["ledger"], Value::Null);
    let on = w.ok(Switch::On, false);
    assert!(warnings(&on).contains("did not write"));
    assert_eq!(w.text(), original);
    assert_eq!(w.ledger(), ledger::Ledger::default());
    let none = Case::new("sw-none");
    let on = none.ok(Switch::On, true);
    assert!(warnings(&on).contains("nothing to remove"));
}

#[test]
fn a_plugin_that_is_off_in_the_user_settings_is_said_so_and_not_changed() {
    let w = Case::new("sw-user-off");
    let user_file = w.home.join(".claude/settings.json");
    put(
        &user_file,
        &json!({"enabledPlugins": {ID: false}}).to_string(),
    );
    let user_before = std::fs::read_to_string(&user_file).unwrap();
    let plan = w.ok(Switch::Off, true);
    assert!(warnings(&plan).contains("not on in your user or project settings"));
    w.ok(Switch::Off, false);
    let on = w.ok(Switch::On, true);
    assert!(warnings(&on).contains("stays off here after this"));
    assert_eq!(std::fs::read_to_string(&user_file).unwrap(), user_before);
}

#[test]
fn off_and_on_refuse_what_they_cannot_do() {
    let w = Case::new("sw-refuse");
    let err = w.switch_id(Switch::Off, "ghost@x", true).unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);
    let err = w.switch_id(Switch::Off, "bad id", true).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Usage);
    assert!(err.message.contains("invalid plugin id"));
    let moved = Case {
        cwd: w.cwd.join("nope"),
        ..Case::new("sw-refuse-folder")
    };
    let err = moved.switch(Switch::On, true).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Usage);
    assert!(err.message.contains("not a folder"));
    assert!(!w.settings().exists());
    assert_eq!(w.ledger(), ledger::Ledger::default());
}

#[test]
fn an_unreadable_install_list_only_warns() {
    let w = Case::new("sw-no-list");
    std::fs::remove_file(w.home.join(".claude/plugins/installed_plugins.json")).unwrap();
    let plan = w.ok(Switch::Off, true);
    assert!(warnings(&plan).contains("could not be read"));
}

const HAND: &str =
    "format: 1\nname: hand\nskills:\n  off: [scratch-1]\nplugins:\n  off: [\"ecc@ecc\"]\n";

#[test]
fn a_bundle_and_an_off_control_own_the_same_key_and_leave_in_either_order() {
    for had_file in [false, true] {
        for control_first in [true, false] {
            for bundle_leaves_first in [true, false] {
                let w = Case::new("sw-handover");
                put(&w.roots.skills_repo_path().join("profiles/hand.yaml"), HAND);
                if had_file {
                    put(
                        &w.settings(),
                        "{\n  \"model\": \"opus\",\n  \"enabledPlugins\": {\n    \"mine@own\": true\n  }\n}\n",
                    );
                }
                let before = tree_snapshot(&w.cwd);
                let world = BundleWorld {
                    roots: &w.roots,
                    data_dir: &w.data,
                    loads: &NoLoads,
                };
                let apply_bundle = || bundle_apply::apply(&world, "hand", &w.cwd, false).unwrap();
                if control_first {
                    w.ok(Switch::Off, false);
                    apply_bundle();
                } else {
                    apply_bundle();
                    w.ok(Switch::Off, false);
                }
                let key = |w: &Case| w.json()["enabledPlugins"][ID].clone();
                assert_eq!(key(&w), json!(false));
                let tag = format!(
                    "had_file {had_file} control_first {control_first} bundle_leaves_first {bundle_leaves_first}"
                );
                if bundle_leaves_first {
                    bundle_apply::undo(&world, &w.cwd, false).unwrap();
                    if control_first {
                        assert_eq!(
                            key(&w),
                            json!(false),
                            "the control still owns the key: {tag}"
                        );
                    }
                    w.ok(Switch::On, false);
                } else {
                    w.ok(Switch::On, false);
                    if !control_first {
                        assert_eq!(
                            key(&w),
                            json!(false),
                            "the bundle still owns the key: {tag}"
                        );
                    }
                    bundle_apply::undo(&world, &w.cwd, false).unwrap();
                }
                assert_eq!(tree_snapshot(&w.cwd), before, "{tag}");
                assert_eq!(w.ledger(), ledger::Ledger::default(), "{tag}");
            }
        }
    }
}

fn gen_foreign(rng: &mut Rng) -> String {
    let mut parts = vec![format!("\"model\": \"{}\"", rng.slug(8))];
    if rng.chance(50) {
        parts.push(format!(
            "\"permissions\": {{\"allow\": [\"Bash({}:*)\"]}}",
            rng.slug(6)
        ));
    }
    if rng.chance(50) {
        parts.push(format!(
            "\"enabledPlugins\": {{\"{}@own\": true}}",
            rng.slug(6)
        ));
    }
    let indent = *rng.pick(&["", "  ", "\t"]);
    format!("{{\n{indent}{}\n}}\n", parts.join(&format!(",\n{indent}")))
}

#[test]
fn off_then_on_leave_every_foreign_key_byte_identical() {
    run_cases("plugin-switch-foreign", 24, |_seed, rng| {
        let w = Case::new("sw-prop");
        let text = gen_foreign(rng);
        put(&w.settings(), &text);
        let before = tree_snapshot(&w.cwd);
        w.ok(Switch::Off, false);
        let after_off = w.json();
        assert_eq!(after_off["enabledPlugins"][ID], json!(false));
        w.ok(Switch::On, false);
        assert_eq!(tree_snapshot(&w.cwd), before);
        assert_eq!(w.text(), text);
    });
}
