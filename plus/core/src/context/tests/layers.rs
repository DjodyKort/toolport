use super::world::{world, World};
use crate::context::layer_deploy;
use crate::context::layer_list;
use serde_json::Value;
use std::fs;

const BEGIN: &str = "<!-- toolport:layer:begin ";

fn setup(w: &World) {
    w.clients_root("clients");
    fs::create_dir_all(w.home.join("clients/alpha/.git")).unwrap();
    fs::create_dir_all(w.home.join("clients/beta")).unwrap();
    w.layer(
        "client-alpha",
        "scope: folder\nfolders: [\"**/clients/alpha/**\"]",
        "## alpha\n\nUse the staging database.",
    );
}

fn actions(data: &Value) -> Vec<(String, String, String)> {
    data["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["layer"].as_str().unwrap().to_string(),
                r["folder"]
                    .as_str()
                    .unwrap()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_string(),
                r["action"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn row(name: &str, folder: &str, action: &str) -> (String, String, String) {
    (name.into(), folder.into(), action.into())
}

#[test]
fn test01_a_folder_layer_lands_only_in_the_folders_its_pattern_names() {
    let w = world("layers-add");
    setup(&w);
    let data = layer_deploy::deploy(&w.host, false, true).unwrap();
    assert_eq!(actions(&data), [row("client-alpha", "alpha", "add")]);
    let file = w.read("clients/alpha/CLAUDE.local.md");
    assert!(
        file.starts_with("<!-- toolport:layer:begin client-alpha -->\n"),
        "{file}"
    );
    assert!(file.contains("Use the staging database."));
    assert!(file.ends_with("<!-- toolport:layer:end client-alpha -->\n"));
    assert!(!w.exists("clients/beta/CLAUDE.local.md"));
    assert!(w
        .read("clients/alpha/.git/info/exclude")
        .contains("CLAUDE.local.md"));
    assert_eq!(data["undo"], "toolportctl context layers undo");
}

#[test]
fn test02_a_second_deploy_changes_nothing_and_a_plan_writes_nothing() {
    let w = world("layers-idem");
    setup(&w);
    let planned = layer_deploy::deploy(&w.host, true, true).unwrap();
    assert_eq!(actions(&planned), [row("client-alpha", "alpha", "add")]);
    assert!(!w.exists("clients/alpha/CLAUDE.local.md"));
    assert!(!w.data.join("plus/layer-ledger.json").exists());
    layer_deploy::deploy(&w.host, false, true).unwrap();
    let before = w.tree();
    let again = layer_deploy::deploy(&w.host, false, true).unwrap();
    assert_eq!(again["rows"].as_array().unwrap().len(), 0);
    assert_eq!(again["upToDate"], 1);
    assert_eq!(again["undo"], Value::Null);
    assert_eq!(w.tree(), before);
}

#[test]
fn test03_the_text_around_our_block_stays_byte_for_byte_and_undo_gives_the_file_back() {
    for original in ["my own notes\n", "my own notes", "", "a\r\nb\r\n"] {
        let w = world("layers-around");
        setup(&w);
        if !original.is_empty() {
            w.write("clients/alpha/CLAUDE.local.md", original);
        }
        layer_deploy::deploy(&w.host, false, true).unwrap();
        let deployed = w.read("clients/alpha/CLAUDE.local.md");
        assert!(deployed.starts_with(original), "{deployed:?}");
        let undone = layer_deploy::undo(&w.host, false).unwrap();
        assert_eq!(undone["rows"][0]["action"], "remove");
        if original.is_empty() {
            assert!(
                !w.exists("clients/alpha/CLAUDE.local.md"),
                "a file we created goes with the block"
            );
        } else {
            assert_eq!(w.read("clients/alpha/CLAUDE.local.md"), original);
        }
        assert!(!w
            .data
            .join("plus/layer-ledger.json")
            .read_to_string_lossy()
            .contains("client-alpha"));
    }
}

trait ReadLossy {
    fn read_to_string_lossy(&self) -> String;
}

impl ReadLossy for std::path::PathBuf {
    fn read_to_string_lossy(&self) -> String {
        fs::read_to_string(self).unwrap_or_default()
    }
}

#[test]
fn test04_a_bundle_block_in_the_same_file_is_carried_over_untouched() {
    let w = world("layers-bundle");
    setup(&w);
    let bundle =
        "<!-- toolport:bundle:begin lean -->\nbundle text\n<!-- toolport:bundle:end lean -->\n";
    w.write("clients/alpha/CLAUDE.local.md", bundle);
    layer_deploy::deploy(&w.host, false, true).unwrap();
    let file = w.read("clients/alpha/CLAUDE.local.md");
    assert!(file.starts_with(bundle), "{file}");
    assert!(file.contains(BEGIN));
    w.layer(
        "client-alpha",
        "scope: folder\nfolders: [\"**/clients/alpha/**\"]",
        "changed",
    );
    layer_deploy::deploy(&w.host, false, true).unwrap();
    assert!(w.read("clients/alpha/CLAUDE.local.md").starts_with(bundle));
}

#[test]
fn test05_a_changed_layer_updates_the_block_and_undo_restores_the_text_it_replaced() {
    let w = world("layers-update");
    setup(&w);
    layer_deploy::deploy(&w.host, false, true).unwrap();
    let first = w.read("clients/alpha/CLAUDE.local.md");
    w.layer(
        "client-alpha",
        "scope: folder\nfolders: [\"**/clients/alpha/**\"]",
        "## alpha\n\nUse production read-only.",
    );
    let data = layer_deploy::deploy(&w.host, false, true).unwrap();
    assert_eq!(actions(&data), [row("client-alpha", "alpha", "update")]);
    assert!(w
        .read("clients/alpha/CLAUDE.local.md")
        .contains("production read-only"));
    layer_deploy::undo(&w.host, false).unwrap();
    assert_eq!(w.read("clients/alpha/CLAUDE.local.md"), first);
}

#[test]
fn test06_a_block_changed_by_hand_is_held_by_an_unattended_run_and_replaced_with_a_copy_by_a_confirmed_one(
) {
    let w = world("layers-hand");
    setup(&w);
    layer_deploy::deploy(&w.host, false, true).unwrap();
    let edited = w
        .read("clients/alpha/CLAUDE.local.md")
        .replace("staging", "my own");
    w.write("clients/alpha/CLAUDE.local.md", &edited);
    w.layer(
        "client-alpha",
        "scope: folder\nfolders: [\"**/clients/alpha/**\"]",
        "## alpha\n\nnew text",
    );

    let held = layer_deploy::deploy(&w.host, false, false).unwrap();
    assert_eq!(actions(&held), [row("client-alpha", "alpha", "held")]);
    assert_eq!(
        w.read("clients/alpha/CLAUDE.local.md"),
        edited,
        "an unattended run leaves it"
    );

    let plan = layer_deploy::deploy(&w.host, true, true).unwrap();
    assert_eq!(actions(&plan), [row("client-alpha", "alpha", "replace")]);
    assert_eq!(
        w.read("clients/alpha/CLAUDE.local.md"),
        edited,
        "a plan writes nothing"
    );

    let done = layer_deploy::deploy(&w.host, false, true).unwrap();
    assert_eq!(actions(&done), [row("client-alpha", "alpha", "replace")]);
    assert!(w.read("clients/alpha/CLAUDE.local.md").contains("new text"));
    let backups = done["backups"].as_array().unwrap();
    assert_eq!(backups.len(), 1);
    assert!(fs::read_to_string(backups[0].as_str().unwrap())
        .unwrap()
        .contains("my own"));
}

#[test]
fn test07_a_block_no_layer_asks_for_is_removed_by_a_confirmed_run_only() {
    let w = world("layers-stale");
    setup(&w);
    layer_deploy::deploy(&w.host, false, true).unwrap();
    fs::remove_dir_all(w.home.join(".config/mcpm/skills_repo/rules/client-alpha")).unwrap();
    let held = layer_deploy::deploy(&w.host, false, false).unwrap();
    assert_eq!(actions(&held), [row("client-alpha", "alpha", "held")]);
    assert!(w.exists("clients/alpha/CLAUDE.local.md"));
    let done = layer_deploy::deploy(&w.host, false, true).unwrap();
    assert_eq!(actions(&done), [row("client-alpha", "alpha", "remove")]);
    assert!(!w.exists("clients/alpha/CLAUDE.local.md"));
}

#[test]
fn test08_a_folder_outside_the_home_folder_is_refused() {
    let w = world("layers-outside");
    let outside = w.home.parent().unwrap().join("outside");
    fs::create_dir_all(&outside).unwrap();
    w.layer(
        "client-x",
        &format!("scope: folder\nfolders: [\"{}\"]", outside.display()),
        "text",
    );
    let data = layer_deploy::deploy(&w.host, false, true).unwrap();
    assert_eq!(actions(&data), [row("client-x", "outside", "failed")]);
    assert!(data["rows"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("outside the home folder"));
    assert!(!outside.join("CLAUDE.local.md").exists());
}

#[test]
fn test09_an_import_is_copied_into_the_block_or_written_as_an_at_line() {
    let w = world("layers-import");
    w.write("notes/shared.md", "shared knowledge\n");
    fs::create_dir_all(w.home.join("proj")).unwrap();
    w.layer(
        "copy",
        "scope: folder\nfolders: [\"~/proj\"]\nimports: [\"~/notes/shared.md\"]",
        "own text",
    );
    layer_deploy::deploy(&w.host, false, true).unwrap();
    let file = w.read("proj/CLAUDE.local.md");
    assert!(
        file.contains("own text") && file.contains("shared knowledge"),
        "{file}"
    );
    assert!(!file.contains("@~/notes"));

    w.write("notes/shared.md", "shared knowledge, newer\n");
    let data = layer_deploy::deploy(&w.host, false, true).unwrap();
    assert_eq!(actions(&data), [row("copy", "proj", "update")]);
    assert!(w.read("proj/CLAUDE.local.md").contains("newer"));

    w.layer(
        "copy",
        "scope: folder\nfolders: [\"~/proj\"]\nimports: [\"~/notes/shared.md\"]\ndelivery: import",
        "own text",
    );
    layer_deploy::deploy(&w.host, false, true).unwrap();
    let file = w.read("proj/CLAUDE.local.md");
    assert!(
        file.contains("@~/notes/shared.md") && !file.contains("newer"),
        "{file}"
    );
}

#[test]
fn test10_an_import_must_be_a_text_file_inside_the_home_folder() {
    let w = world("layers-import-limits");
    fs::create_dir_all(w.home.join("proj")).unwrap();
    let outside = w.home.parent().unwrap().join("outside.md");
    fs::write(&outside, "secret-ish\n").unwrap();
    w.write("notes/key.json", "{}\n");
    std::os::unix::fs::symlink(&outside, w.home.join("notes/link.md")).unwrap();
    for (import, why) in [
        (outside.display().to_string(), "outside the home folder"),
        ("~/notes/key.json".to_string(), "not a text file"),
        ("~/notes/link.md".to_string(), "outside the home folder"),
        ("~/notes/gone.md".to_string(), "does not exist"),
    ] {
        w.layer(
            "x",
            &format!("scope: folder\nfolders: [\"~/proj\"]\nimports: [\"{import}\"]"),
            "t",
        );
        let data = layer_deploy::deploy(&w.host, false, true).unwrap();
        let reason = data["rows"][0]["reason"].as_str().unwrap().to_string();
        assert_eq!(data["rows"][0]["action"], "failed", "{import}");
        assert!(reason.contains(why), "{import}: {reason}");
        assert!(!w.exists("proj/CLAUDE.local.md"));
    }
}

#[test]
fn test11_a_block_without_a_ledger_is_adopted_when_it_matches_and_held_when_it_differs() {
    let w = world("layers-noledger");
    setup(&w);
    layer_deploy::deploy(&w.host, false, true).unwrap();
    fs::remove_file(w.data.join("plus/layer-ledger.json")).unwrap();
    let again = layer_deploy::deploy(&w.host, false, false).unwrap();
    assert_eq!(again["rows"].as_array().unwrap().len(), 0);
    fs::remove_file(w.data.join("plus/layer-ledger.json")).unwrap();
    w.layer(
        "client-alpha",
        "scope: folder\nfolders: [\"**/clients/alpha/**\"]",
        "different",
    );
    let held = layer_deploy::deploy(&w.host, false, false).unwrap();
    assert_eq!(actions(&held), [row("client-alpha", "alpha", "held")]);
}

#[test]
fn test12_a_star_pattern_names_folders_the_way_a_bundle_binds_them() {
    let w = world("layers-star");
    fs::create_dir_all(w.home.join("work/a/app")).unwrap();
    fs::create_dir_all(w.home.join("work/b/app")).unwrap();
    fs::create_dir_all(w.home.join("work/c")).unwrap();
    w.layer("apps", "scope: folder\nfolders: [\"~/work/*/app\"]", "t");
    let data = layer_deploy::deploy(&w.host, false, true).unwrap();
    assert_eq!(
        actions(&data),
        [row("apps", "app", "add"), row("apps", "app", "add")]
    );
    assert!(w.exists("work/a/app/CLAUDE.local.md") && w.exists("work/b/app/CLAUDE.local.md"));
    assert!(!w.exists("work/c/CLAUDE.local.md"));
}

fn state_of(data: &Value, name: &str) -> String {
    data["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == name)
        .unwrap()["state"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn test13_layers_ls_names_where_each_layer_lands_and_the_state_of_its_blocks() {
    let w = world("layers-ls");
    setup(&w);
    w.layer(
        "client-gamma",
        "scope: folder\nfolders: [\"**/clients/gamma/**\"]",
        "t",
    );
    let ls = layer_list::ls(&w.host).unwrap();
    assert_eq!(state_of(&ls, "client-alpha"), "not-deployed");
    assert_eq!(state_of(&ls, "client-gamma"), "nowhere");
    layer_deploy::deploy(&w.host, false, true).unwrap();
    let ls = layer_list::ls(&w.host).unwrap();
    assert_eq!(state_of(&ls, "client-alpha"), "deployed");
    let alpha = ls["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "client-alpha")
        .unwrap();
    assert_eq!(alpha["target"]["kind"], "folders");
    assert_eq!(alpha["target"]["deployed"], 1);
    assert_eq!(alpha["tokens"]["basis"], "estimate");
    let edited = w
        .read("clients/alpha/CLAUDE.local.md")
        .replace("staging", "mine");
    w.write("clients/alpha/CLAUDE.local.md", &edited);
    assert_eq!(
        state_of(&layer_list::ls(&w.host).unwrap(), "client-alpha"),
        "edited"
    );
}

#[test]
fn test14_a_global_layer_is_a_rule_that_skills_sync_delivers_and_its_state_is_that_plan() {
    let w = world("layers-global");
    w.layer("personal", "", "## Personal preferences\n\nShort answers.");
    let ls = layer_list::ls(&w.host).unwrap();
    assert_eq!(state_of(&ls, "personal"), "not-deployed");
    let plan = layer_list::deploy_rules(&w.host, true).unwrap();
    assert_eq!(plan["rows"][0]["op"], "add");
    assert!(
        !w.exists(".claude/rules/personal.md"),
        "a plan writes nothing"
    );
    layer_list::deploy_rules(&w.host, false).unwrap();
    assert!(w
        .read(".claude/rules/personal.md")
        .contains("Short answers."));
    assert_eq!(
        state_of(&layer_list::ls(&w.host).unwrap(), "personal"),
        "deployed"
    );

    let ours = w.read(".claude/rules/personal.md");
    w.write(".claude/rules/personal.md", &ours.replace("Short", "Long"));
    assert_eq!(
        state_of(&layer_list::ls(&w.host).unwrap(), "personal"),
        "edited"
    );
    let held = layer_list::deploy_rules(&w.host, false).unwrap();
    assert_eq!(
        held["held"].as_array().unwrap().len(),
        1,
        "a hand edit waits for a decision"
    );
    assert!(w
        .read(".claude/rules/personal.md")
        .contains("Long answers."));
}

#[test]
fn test15_a_hand_written_rule_file_shadows_the_layer_until_someone_decides() {
    let w = world("layers-shadow");
    w.layer("personal", "", "## Personal\n\nShort answers.");
    w.write(".claude/rules/personal.md", "my own rules\n");
    assert_eq!(
        state_of(&layer_list::ls(&w.host).unwrap(), "personal"),
        "shadowed"
    );
    layer_list::deploy_rules(&w.host, false).unwrap();
    assert_eq!(w.read(".claude/rules/personal.md"), "my own rules\n");
}
