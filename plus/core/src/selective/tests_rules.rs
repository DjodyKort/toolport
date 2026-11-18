use super::ops::{self, Choices};
use super::rules::library_text;
use super::store::{Kind, State};
use super::tests::{put, Fx, TAP};
use crate::skills::origin::read_origins;
use crate::skills::parser::{parse_skill_file, SkillType};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

const STYLE: &str = "rules/style/SKILL.md";

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn force(ids: &[&str]) -> Choices {
    Choices {
        force: ids.iter().map(|s| s.to_string()).collect(),
        skip: Vec::new(),
    }
}

fn skip(ids: &[&str]) -> Choices {
    Choices {
        force: Vec::new(),
        skip: ids.iter().map(|s| s.to_string()).collect(),
    }
}

fn library_with_a_rule(fx: &Fx) {
    put(&fx.library.join("rules/mine/SKILL.md"), "my own rule\n");
    put(
        &fx.library.join("mcpm-skills-origins.json"),
        "{\n  \"mine\": \"local\"\n}",
    );
}

#[test]
fn test01_apply_installs_a_chosen_rule_into_the_library_as_a_skill_folder() {
    let fx = Fx::new();
    library_with_a_rule(&fx);
    fx.set(&["style"], &[], &[]).unwrap();
    let applied = fx.apply().unwrap();
    let skill = parse_skill_file(&fx.library.join(STYLE)).unwrap();
    assert_eq!(skill.name(), "style");
    assert_eq!(skill.skill_type, SkillType::Rule);
    assert!(std::fs::read_to_string(fx.library.join(STYLE))
        .unwrap()
        .contains("Keep functions short."));
    assert_eq!(read_origins(&fx.library)["style"], "tap:example-tap");
    assert_eq!(read_origins(&fx.library)["mine"], "local");
    assert!(applied["result"]["changed"]
        .to_string()
        .contains("rules/style written"));
    let linked = &fx.selection().sources[TAP].linked["style"];
    assert_eq!(linked.name, "style");
}

#[test]
fn test02_undo_puts_every_library_file_back_byte_for_byte() {
    let fx = Fx::new();
    library_with_a_rule(&fx);
    let before = snapshot(&fx.library);
    fx.set(&["style", "code-review"], &[], &[]).unwrap();
    fx.apply().unwrap();
    assert_ne!(snapshot(&fx.library), before);
    ops::undo(&fx.env(), TAP).unwrap();
    assert_eq!(snapshot(&fx.library), before);
    assert!(fx.selection().sources[TAP].linked.is_empty());
}

#[test]
fn test03_undo_of_a_first_install_leaves_no_rules_folder_in_a_new_library() {
    let fx = Fx::new();
    std::fs::create_dir_all(&fx.library).unwrap();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    ops::undo(&fx.env(), TAP).unwrap();
    assert!(snapshot(&fx.library).is_empty());
    assert!(!fx.library.join("rules").exists());
    assert!(!fx.library.join("mcpm-skills-origins.json").exists());
}

#[test]
fn test04_a_folder_toolport_did_not_write_is_a_conflict_that_needs_a_choice() {
    let fx = Fx::new();
    put(
        &fx.library.join(STYLE),
        "---\nname: style\ndescription: mine\n---\nMine\n",
    );
    let before = snapshot(&fx.library);
    fx.set(&["style"], &[], &[]).unwrap();
    let plan = fx.plan();
    assert_eq!(plan["conflicts"][0]["conflict"]["code"], "foreign");
    assert_eq!(
        plan["conflicts"][0]["conflict"]["choices"],
        serde_json::json!(["force", "skip"])
    );
    let refused = fx.apply().unwrap_err();
    assert!(refused.contains("--force <rule>"), "{refused}");
    assert_eq!(snapshot(&fx.library), before);
    assert!(fx.selection().sources[TAP].pinned.is_none());
}

#[test]
fn test05_skip_leaves_the_foreign_folder_alone_and_keeps_the_rule_on() {
    let fx = Fx::new();
    put(&fx.library.join(STYLE), "mine\n");
    fx.set(&["style", "code-review"], &[], &[]).unwrap();
    fx.apply_with(&skip(&["style"])).unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.library.join(STYLE)).unwrap(),
        "mine\n"
    );
    assert!(fx.selection().sources[TAP].linked.is_empty());
    assert_eq!(fx.state_of(Kind::Rule, "style"), State::On);
    assert!(fx.slice().join("skills/code-review").is_dir());
}

#[test]
fn test06_force_replaces_the_foreign_folder_and_undo_brings_it_back() {
    let fx = Fx::new();
    put(&fx.library.join(STYLE), "mine\n");
    let before = snapshot(&fx.library);
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply_with(&force(&["style"])).unwrap();
    assert!(std::fs::read_to_string(fx.library.join(STYLE))
        .unwrap()
        .contains("Keep functions short."));
    ops::undo(&fx.env(), TAP).unwrap();
    assert_eq!(snapshot(&fx.library), before);
}

#[test]
fn test07_a_choice_for_a_rule_without_a_conflict_is_refused() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    let refused = fx.apply_with(&force(&["style"])).unwrap_err();
    assert!(refused.contains("rule:style has no conflict"), "{refused}");
    assert!(!fx.library.exists());
}

#[test]
fn test08_turning_a_rule_off_removes_the_folder_this_selection_wrote() {
    let fx = Fx::new();
    library_with_a_rule(&fx);
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    fx.set(&[], &[], &["style"]).unwrap();
    let plan = fx.plan();
    assert_eq!(plan["rules"][0]["action"], "remove");
    fx.apply().unwrap();
    assert!(!fx.library.join("rules/style").exists());
    assert!(!read_origins(&fx.library).contains_key("style"));
    assert!(fx.library.join("rules/mine/SKILL.md").exists());
    assert!(fx.selection().sources[TAP].linked.is_empty());
    ops::undo(&fx.env(), TAP).unwrap();
    assert!(fx.library.join(STYLE).exists());
    assert!(fx.selection().sources[TAP].linked.contains_key("style"));
}

#[test]
fn test09_a_folder_edited_after_the_install_is_not_removed_without_force() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    put(&fx.library.join(STYLE), "edited by me\n");
    fx.set(&[], &[], &["style"]).unwrap();
    let plan = fx.plan();
    assert_eq!(plan["conflicts"][0]["conflict"]["code"], "edited");
    fx.apply().unwrap_err();
    assert!(fx.library.join(STYLE).exists());
    fx.apply_with(&skip(&["style"])).unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.library.join(STYLE)).unwrap(),
        "edited by me\n"
    );
    assert!(fx.selection().sources[TAP].linked.is_empty());
}

#[test]
fn test10_force_removes_an_edited_folder_and_undo_restores_the_edit() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    put(&fx.library.join(STYLE), "edited by me\n");
    let before = snapshot(&fx.library);
    fx.set(&[], &[], &["style"]).unwrap();
    fx.apply_with(&force(&["style"])).unwrap();
    assert!(!fx.library.join("rules/style").exists());
    ops::undo(&fx.env(), TAP).unwrap();
    assert_eq!(snapshot(&fx.library), before);
}

#[test]
fn test11_a_rule_changed_upstream_replaces_the_library_copy_it_wrote() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    put(
        &fx.upstream.join("rules/style.md"),
        "Keep functions tiny.\n",
    );
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let plan = fx.plan();
    assert_eq!(plan["rules"][0]["action"], "update");
    assert_eq!(plan["waiting"], true);
    fx.apply().unwrap();
    assert!(std::fs::read_to_string(fx.library.join(STYLE))
        .unwrap()
        .contains("Keep functions tiny."));
    assert_eq!(fx.plan()["rules"], Value::Array(Vec::new()));
}

#[test]
fn test12_an_edited_copy_is_not_replaced_by_an_upstream_change_without_force() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    put(&fx.library.join(STYLE), "edited by me\n");
    put(
        &fx.upstream.join("rules/style.md"),
        "Keep functions tiny.\n",
    );
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    assert_eq!(fx.plan()["conflicts"][0]["conflict"]["code"], "edited");
    fx.apply().unwrap_err();
    fx.apply_with(&skip(&["style"])).unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.library.join(STYLE)).unwrap(),
        "edited by me\n"
    );
}

#[test]
fn test13_an_edit_alone_is_left_in_place_when_the_source_did_not_change() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    put(&fx.library.join(STYLE), "edited by me\n");
    assert_eq!(fx.plan()["rules"], Value::Array(Vec::new()));
}

#[test]
fn test14_undo_refuses_when_the_library_changed_since_the_apply() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    put(&fx.library.join(STYLE), "edited by me\n");
    let refused = ops::undo(&fx.env(), TAP).unwrap_err().message;
    assert!(
        refused.contains("rules/style changed in the Library"),
        "{refused}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.library.join(STYLE)).unwrap(),
        "edited by me\n"
    );
    assert!(fx.selection().sources[TAP].linked.contains_key("style"));
}

#[test]
fn test15_without_a_library_the_rule_is_a_conflict_that_only_skip_can_settle() {
    let fx = Fx::new();
    let mut env = fx.env();
    env.library = None;
    fx.set(&["style", "code-review"], &[], &[]).unwrap();
    let opts = ops::Options {
        fetch: true,
        ..Default::default()
    };
    let plan = ops::plan(&env, TAP, opts).unwrap();
    assert_eq!(plan["conflicts"][0]["conflict"]["code"], "no-library");
    assert_eq!(
        plan["conflicts"][0]["conflict"]["choices"],
        serde_json::json!(["skip"])
    );
    let hash = plan["planHash"].as_str().unwrap();
    ops::apply(&env, TAP, hash, opts, &Choices::default()).unwrap_err();
    ops::apply(&env, TAP, hash, opts, &force(&["style"])).unwrap_err();
    ops::apply(&env, TAP, hash, opts, &skip(&["style"])).unwrap();
    assert!(fx.slice().join("skills/code-review").is_dir());
}

#[test]
fn test16_a_rule_that_vanished_upstream_keeps_its_library_copy_until_turned_off() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    std::fs::remove_file(fx.upstream.join("rules/style.md")).unwrap();
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let plan = fx.plan();
    assert_eq!(plan["diffs"]["gone"][0]["id"], "style");
    assert_eq!(plan["rules"], Value::Array(Vec::new()));
    fx.apply().unwrap();
    assert!(fx.library.join(STYLE).exists());
    assert_eq!(fx.state_of(Kind::Rule, "style"), State::Removed);
    fx.set(&[], &[], &["style"]).unwrap();
    fx.apply().unwrap();
    assert!(!fx.library.join("rules/style").exists());
}

#[test]
fn test17_a_rule_deleted_from_the_library_by_hand_is_installed_again_when_still_on() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    std::fs::remove_dir_all(fx.library.join("rules/style")).unwrap();
    assert_eq!(fx.plan()["rules"][0]["action"], "install");
}

#[test]
fn test18_a_rule_deleted_by_hand_and_turned_off_only_drops_the_record() {
    let fx = Fx::new();
    fx.set(&["style"], &[], &[]).unwrap();
    fx.apply().unwrap();
    std::fs::remove_dir_all(fx.library.join("rules/style")).unwrap();
    fx.set(&[], &[], &["style"]).unwrap();
    assert_eq!(fx.plan()["rules"][0]["action"], "forget");
    fx.apply().unwrap();
    assert!(fx.selection().sources[TAP].linked.is_empty());
}

#[test]
fn test19_the_library_text_keeps_the_rule_body_and_names_it_for_the_library() {
    let text = library_text("style-guide", "# Style\n\nKeep it short.\n");
    assert!(
        text.starts_with("---\nname: style-guide\ndescription: Style\nactivation: always\n---\n")
    );
    assert!(text.ends_with("Keep it short.\n"));
    let own = library_text(
        "py",
        "---\ndescription: Python style\npaths: \"**/*.py\"\n---\nUse type hints.\n",
    );
    assert!(own.contains("description: Python style"));
    assert!(own.contains("Use type hints."));
    assert!(!own.contains("paths:"));
}

#[test]
fn test20_names_are_made_library_safe_and_a_clash_is_a_conflict() {
    let fx = Fx::new();
    let clone = fx.plugins.data_dir.join("taps").join(TAP);
    for root in [&fx.upstream, &clone] {
        put(&root.join("rules/Style_Guide.md"), "One.\n");
        put(&root.join("rules/style-guide.md"), "Two.\n");
        put(&root.join("rules/lang/py.md"), "Three.\n");
    }
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    fx.set(&["Style_Guide", "style-guide", "lang/py"], &[], &[])
        .unwrap();
    let plan = fx.plan();
    let names: Vec<&str> = plan["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["style-guide", "lang-py", "style-guide"]);
    let clash = plan["conflicts"].as_array().unwrap();
    assert_eq!(clash.len(), 1);
    assert_eq!(clash[0]["conflict"]["code"], "name");
}

#[test]
fn test21_a_marketplace_the_validator_refuses_leaves_the_library_as_it_was() {
    let fx = Fx::new();
    library_with_a_rule(&fx);
    let before = snapshot(&fx.library);
    fx.set(&["style", "code-review"], &[], &[]).unwrap();
    *fx.claude.fail.lock().unwrap() = Some("bad manifest".into());
    fx.apply().unwrap_err();
    assert_eq!(snapshot(&fx.library), before);
    assert!(fx.selection().sources[TAP].linked.is_empty());
    assert!(!fx.slice().exists());
}
