use super::*;
use crate::plus::skills::parser::{parse_skill_file, Skill};
use crate::plus::skills::transpiler::{Capabilities, Transpiler};
use std::fs;
use std::path::{Path, PathBuf};

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("skills-wave2-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn skill(root: &Path, front: &str, body: &str) -> Skill {
    let p = root.join("s").join("SKILL.md");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, format!("---\n{front}---\n{body}\n")).unwrap();
    parse_skill_file(&p).unwrap()
}

#[test]
fn full_registry_has_fifteen_clients_without_vscode() {
    let mut reg = TranspilerRegistry::new();
    register_all_with_home(&mut reg, None);
    assert_eq!(reg.len(), 15);
    assert!(reg.get("vscode").is_none());
    register_vscode_copilot(&mut reg);
    assert_eq!(reg.all().last().unwrap().client_key(), "vscode");
}

#[test]
fn only_zed_and_agents_md_are_project_only_and_append_mode() {
    let mut reg = TranspilerRegistry::new();
    register_all_with_home(&mut reg, None);
    register_vscode_copilot(&mut reg);
    for t in reg.all() {
        let aggregates = matches!(t.client_key(), "zed" | "agents-md");
        let expected = if aggregates {
            Capabilities::PROJECT_APPEND
        } else {
            Capabilities::PER_FILE
        };
        assert_eq!(t.capabilities(), expected, "{}", t.client_key());
    }
}

#[test]
fn codex_uses_dot_codex_only_for_the_home_root() {
    let t = Tmp::new("codex");
    let s = skill(&t.0, "name: s\ndescription: d\n", "B");
    let home = t.0.join("home");
    let codex = codex_cli::CodexCli::new(Some(home.clone()));
    assert_eq!(
        codex.get_output_path(&s, &home),
        home.join(".codex/skills/s/SKILL.md")
    );
    let proj = t.0.join("proj");
    assert_eq!(
        codex.get_output_path(&s, &proj),
        proj.join(".agents/skills/s/SKILL.md")
    );
}

#[test]
fn continue_maps_priority_and_downgrades_agent() {
    let t = Tmp::new("continue");
    let s = skill(
        &t.0,
        "name: s\ndescription: d\nactivation: agent\npriority: 3\nglobs: \"*.py\"\n",
        "B",
    );
    let out = continue_dev::ContinueDev.transpile(&s, &t.0).unwrap();
    assert_eq!(
        out.content,
        "---\ndescription: \"d\"\nglobs: *.py\npriority: 3\n---\n\nB\n"
    );
    assert_eq!(
        out.warnings,
        ["continue: activation 'agent' downgraded to 'auto'"]
    );
}

#[test]
fn jetbrains_header_lists_globs_and_description() {
    let t = Tmp::new("jb");
    let s = skill(&t.0, "name: s\ndescription: d\nglobs: \"*.kt\"\n", "B");
    let out = jetbrains::JetBrains.transpile(&s, &t.0).unwrap();
    assert_eq!(
        out.content,
        "<!-- mcpm: name=s, activation=auto, globs=*.kt -->\n<!-- mcpm: description=\"d\" -->\n\nB\n"
    );
}

#[test]
fn vscode_rule_without_globs_has_no_frontmatter() {
    let t = Tmp::new("vsc");
    let s = skill(&t.0, "name: s\ndescription: d\nactivation: always\n", "B");
    let out = vscode_copilot::VsCodeCopilot.transpile(&s, &t.0).unwrap();
    assert_eq!(out.content, "B\n");
    assert!(out
        .output_path
        .ends_with(".github/instructions/s.instructions.md"));
}

#[test]
fn default_clean_removes_output_and_the_empty_parent() {
    let t = Tmp::new("clean");
    let s = skill(&t.0, "name: s\ndescription: d\n", "B");
    let tr = gemini_cli::GeminiCli;
    let out = tr.transpile(&s, &t.0).unwrap();
    fs::create_dir_all(out.output_path.parent().unwrap()).unwrap();
    fs::write(&out.output_path, out.content).unwrap();
    let removed = tr.clean(&t.0, &["s".to_string()]).unwrap();
    assert_eq!(removed, vec![out.output_path.clone()]);
    assert!(!out.output_path.parent().unwrap().exists());
    assert!(tr.clean(&t.0, &["s".to_string()]).unwrap().is_empty());
}

#[test]
fn zed_clean_keeps_user_text_outside_the_block() {
    let t = Tmp::new("zed");
    let s = skill(
        &t.0,
        "name: s\ndescription: d\nactivation: always\n",
        "Body",
    );
    let all = zed::Zed
        .transpile_all(std::slice::from_ref(&s), &t.0)
        .unwrap()
        .unwrap();
    fs::write(&all.output_path, format!("mine\n\n{}", all.content)).unwrap();
    assert_eq!(zed::clean(&t.0).unwrap().len(), 1);
    assert_eq!(fs::read_to_string(t.0.join(".rules")).unwrap(), "mine\n");
}
