use std::path::{Path, PathBuf};

use toolport_plus::skills::parser::parse_skill_file;
use toolport_plus::skills::transpilers::claude_code::ClaudeCode;
use toolport_plus::skills::{Skill, Transpiler};
use toolport_plus_api::SettingsStore;

use crate::plus_adapter::HostAdapter;
use crate::registry::DataDirTestEnv;

const SETTINGS: &str = "{\n  // keep this comment\n  \"theme\": \"dark\",\n  \"hooks\": {\n    \"PreCompact\": [\n      {\"matcher\": \"*\", \"hooks\": [{\"type\": \"command\", \"command\": \"/u/keep.sh\"}]},\n    ],\n  },\n}\n";

fn skill_in(root: &Path) -> Skill {
    let dir = root.join("library/h");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("scripts/a.sh"), "#!/bin/sh\n").unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: h\ndescription: d\nhooks:\n  PreCompact:\n    command: scripts/a.sh\n---\nB\n",
    )
    .unwrap();
    parse_skill_file(&dir.join("SKILL.md")).unwrap()
}

fn project(env: &DataDirTestEnv) -> (PathBuf, PathBuf) {
    let root = env.dir.join("project");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    let settings = root.join(".claude/settings.json");
    std::fs::write(&settings, SETTINGS).unwrap();
    (root, settings)
}

#[test]
fn hook_install_and_removal_keep_the_comments_and_other_keys_of_settings() {
    let env = DataDirTestEnv::new("plus-hooks-cst");
    let (root, settings) = project(&env);
    let skill = skill_in(&env.dir);

    let ids = ClaudeCode
        .install_hooks(&skill, &root, &HostAdapter)
        .unwrap();
    assert_eq!(ids.len(), 1);
    let installed = std::fs::read_to_string(&settings).unwrap();
    assert!(installed.contains("// keep this comment"), "{installed}");
    assert!(installed.contains("\"theme\": \"dark\""), "{installed}");
    assert!(installed.contains(&ids[0]), "{installed}");
    assert!(installed.contains("/u/keep.sh"), "{installed}");

    ClaudeCode
        .uninstall_hooks(&root, &ids, &HostAdapter)
        .unwrap();
    let removed = std::fs::read_to_string(&settings).unwrap();
    assert!(removed.contains("// keep this comment"), "{removed}");
    assert!(removed.contains("\"theme\": \"dark\""), "{removed}");
    assert!(removed.contains("/u/keep.sh"), "{removed}");
    assert!(!removed.contains(&ids[0]), "{removed}");
}

#[test]
fn a_write_after_the_hooks_changed_is_refused() {
    let env = DataDirTestEnv::new("plus-hooks-conflict");
    let (_, settings) = project(&env);
    let (mut root, text) = HostAdapter.read_settings(&settings).unwrap();
    std::fs::write(
        &settings,
        SETTINGS.replace("/u/keep.sh", "/u/changed-meanwhile.sh"),
    )
    .unwrap();
    root["hooks"] = serde_json::json!({});
    let err = HostAdapter
        .write_hooks(&settings, text.as_deref(), &root)
        .unwrap_err();
    assert!(err.contains("conflict"), "{err}");
    assert!(std::fs::read_to_string(&settings)
        .unwrap()
        .contains("changed-meanwhile"));
}

#[test]
fn settings_that_do_not_parse_are_refused_and_left_alone() {
    let env = DataDirTestEnv::new("plus-hooks-malformed");
    let root = env.dir.join("project");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    let settings = root.join(".claude/settings.json");
    std::fs::write(&settings, "{ \"theme\": ").unwrap();
    let skill = skill_in(&env.dir);
    ClaudeCode
        .install_hooks(&skill, &root, &HostAdapter)
        .unwrap_err();
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{ \"theme\": ");
}
