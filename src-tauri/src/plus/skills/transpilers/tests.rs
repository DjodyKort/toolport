use super::agents_md::{self, html_escape};
use super::*;
use crate::plus::skills::json::{self, J};
use crate::plus::skills::parser::{parse_skill_file, Skill, SkillType};
use crate::plus::skills::transpiler::Transpiler;
use std::fs;
use std::path::{Path, PathBuf};

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("skills-wave1-{tag}-{}", std::process::id()));
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

fn skill(root: &Path, dir: &str, front: &str, body: &str) -> Skill {
    let p = root.join(dir).join("SKILL.md");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    if front.contains("hooks:") {
        let scripts = p.parent().unwrap().join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        for f in ["a.sh", "b.sh"] {
            fs::write(scripts.join(f), "#!/bin/sh\n").unwrap();
        }
    }
    fs::write(&p, format!("---\n{front}---\n{body}\n")).unwrap();
    parse_skill_file(&p).unwrap()
}

#[test]
fn registry_order_follows_mcpm_import_order() {
    let mut reg = TranspilerRegistry::new();
    register_wave1(&mut reg);
    let keys: Vec<&str> = reg.all().map(|t| t.client_key()).collect();
    assert_eq!(
        keys,
        ["agents-md", "claude-code", "cline", "cursor", "windsurf"]
    );
}

#[test]
fn html_escape_matches_python() {
    assert_eq!(
        html_escape(r#"a & b < c > "d" 'e'"#),
        "a &amp; b &lt; c &gt; &quot;d&quot; &#x27;e&#x27;"
    );
}

#[test]
fn claude_rule_paths_is_a_flow_list() {
    let t = Tmp::new("rule");
    let mut s = skill(
        &t.0,
        "python-style",
        "name: python-style\ndescription: d\nglobs: \"**/*.py, src/**,, \"\n",
        "Body",
    );
    s.skill_type = SkillType::Rule;
    let out = claude_code::ClaudeCode.transpile(&s, &t.0).unwrap();
    assert_eq!(
        out.content,
        "---\npaths: [\"**/*.py\", \"src/**\"]\n---\n\nBody\n"
    );
    assert!(out.output_path.ends_with(".claude/rules/python-style.md"));
}

#[test]
fn claude_rule_without_globs_has_no_frontmatter() {
    let t = Tmp::new("rule-plain");
    let mut s = skill(&t.0, "r", "name: r\ndescription: d\n", "Body");
    s.skill_type = SkillType::Rule;
    let out = claude_code::ClaudeCode.transpile(&s, &t.0).unwrap();
    assert_eq!(out.content, "Body\n");
}

#[test]
fn claude_manual_skill_disables_model_invocation() {
    let t = Tmp::new("manual");
    let s = skill(
        &t.0,
        "m",
        "name: m\ndescription: d\nactivation: manual\nallowed-tools: Read\n",
        "Body",
    );
    let out = claude_code::ClaudeCode.transpile(&s, &t.0).unwrap();
    assert_eq!(
        out.content,
        "---\nname: m\ndescription: \"d\"\nallowed-tools: Read\ndisable-model-invocation: true\n---\n\nBody\n"
    );
}

#[test]
fn cursor_activations() {
    let t = Tmp::new("cursor");
    let cases = [
        (
            "always",
            "description: \"d\"\nglobs: *.rs\nalwaysApply: true\n",
        ),
        ("auto", "description: \"d\"\nglobs: *.rs\n"),
        ("agent", "description: \"d\"\n"),
        ("manual", "description: \"d\"\n"),
    ];
    for (act, expect) in cases {
        let s = skill(
            &t.0,
            act,
            &format!("name: {act}\ndescription: d\nactivation: {act}\nglobs: \"*.rs\"\n"),
            "B",
        );
        let out = cursor::Cursor.transpile(&s, &t.0).unwrap();
        assert_eq!(out.content, format!("---\n{expect}---\n\nB\n"), "{act}");
    }
}

#[test]
fn windsurf_truncates_at_twelve_thousand_chars() {
    let t = Tmp::new("ws");
    let body = "é".repeat(13_000);
    let s = skill(&t.0, "big", "name: big\ndescription: d\n", &body);
    let out = windsurf::Windsurf.transpile(&s, &t.0).unwrap();
    assert!(out
        .content
        .ends_with("\n\n[truncated -- see full skill at source]\n"));
    assert_eq!(
        out.warnings,
        vec![format!(
            "windsurf: body truncated from {} to fit 12000 char limit",
            s.body.chars().count()
        )]
    );
    assert!(out.content.chars().count() < 12_000);
}

#[test]
fn windsurf_trigger_mapping() {
    let t = Tmp::new("ws-trigger");
    for (front, trigger) in [
        ("activation: always\n", "always_on"),
        ("activation: auto\nglobs: \"*.md\"\n", "glob"),
        ("activation: auto\n", "model_decision"),
        ("activation: agent\n", "model_decision"),
        ("activation: manual\n", "manual"),
    ] {
        let s = skill(&t.0, "w", &format!("name: w\ndescription: d\n{front}"), "B");
        let out = windsurf::Windsurf.transpile(&s, &t.0).unwrap();
        assert!(
            out.content.contains(&format!("trigger: {trigger}\n")),
            "{front}"
        );
    }
}

#[test]
fn cline_downgrade_warnings() {
    let t = Tmp::new("cline");
    let s = skill(
        &t.0,
        "a",
        "name: a\ndescription: d\nactivation: agent\nglobs: \"*.py\"\n",
        "B",
    );
    let out = cline::Cline.transpile(&s, &t.0).unwrap();
    assert_eq!(
        out.warnings,
        ["cline: activation 'agent' downgraded to 'auto' (paths-based)"]
    );
    let s = skill(
        &t.0,
        "b",
        "name: b\ndescription: d\nactivation: manual\n",
        "B",
    );
    let out = cline::Cline.transpile(&s, &t.0).unwrap();
    assert_eq!(
        out.warnings,
        ["cline: activation 'manual' downgraded to 'always'"]
    );
    assert_eq!(out.content, "B\n");
}

const HOOKED: &str = "name: h\ndescription: d\nhooks:\n  SessionStart:\n    command: scripts/a.sh\n  PreCompact:\n    command: scripts/b.sh\n    matcher: auto\n";

fn settings(root: &Path) -> J {
    json::parse(&fs::read_to_string(root.join(".claude/settings.json")).unwrap()).unwrap()
}

#[test]
fn hooks_install_is_idempotent_and_revocable() {
    let t = Tmp::new("hooks");
    let s = skill(&t.0, "h", HOOKED, "B");
    let dst = t.0.join(".claude/skills/h/scripts");
    fs::create_dir_all(&dst).unwrap();
    fs::write(dst.join("a.sh"), "#!/bin/sh\n").unwrap();
    fs::write(
        t.0.join(".claude/settings.json"),
        "{\"theme\": \"dark\", \"hooks\": {\"PreCompact\": [{\"matcher\": \"*\", \"hooks\": [{\"type\": \"command\", \"command\": \"/u/keep.sh\"}]}]}}",
    )
    .unwrap();
    let ct = claude_code::ClaudeCode;
    let ids = ct.install_hooks(&s, &t.0).unwrap();
    assert_eq!(ids.len(), 2);
    let again = ct.install_hooks(&s, &t.0).unwrap();
    assert_eq!(ids, again);
    let J::Arr(pre) = settings(&t.0)
        .get("hooks")
        .unwrap()
        .get("PreCompact")
        .unwrap()
        .clone()
    else {
        panic!()
    };
    assert_eq!(pre.len(), 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dst.join("a.sh")).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0);
    }
    let removed = ct.uninstall_hooks(&t.0, &ids).unwrap();
    assert_eq!(removed.len(), 2);
    let after = settings(&t.0);
    assert_eq!(after.get("theme").and_then(J::as_str), Some("dark"));
    let J::Arr(pre) = after
        .get("hooks")
        .unwrap()
        .get("PreCompact")
        .unwrap()
        .clone()
    else {
        panic!()
    };
    assert_eq!(pre.len(), 1);
    assert!(after.get("hooks").unwrap().get("SessionStart").is_none());
}

#[test]
fn uninstalling_the_last_hook_drops_the_hooks_key() {
    let t = Tmp::new("hooks-drop");
    let s = skill(&t.0, "h", HOOKED, "B");
    let ct = claude_code::ClaudeCode;
    let ids = ct.install_hooks(&s, &t.0).unwrap();
    ct.uninstall_hooks(&t.0, &ids).unwrap();
    assert!(settings(&t.0).get("hooks").is_none());
}

#[test]
fn malformed_settings_are_treated_as_empty() {
    let t = Tmp::new("hooks-bad");
    fs::create_dir_all(t.0.join(".claude")).unwrap();
    fs::write(t.0.join(".claude/settings.json"), "{nope").unwrap();
    let s = skill(&t.0, "h", HOOKED, "B");
    assert_eq!(
        claude_code::ClaudeCode
            .install_hooks(&s, &t.0)
            .unwrap()
            .len(),
        2
    );
    assert!(settings(&t.0).get("hooks").is_some());
}

#[test]
fn agents_md_block_roundtrip_and_clean() {
    let t = Tmp::new("agents");
    fs::write(t.0.join("AGENTS.md"), "# Mine\n\nKeep this.\n").unwrap();
    let s = skill(&t.0, "q", "name: q\ndescription: a \"b\" & c\n", "B");
    let out = agents_md::AgentsMd
        .transpile_all(std::slice::from_ref(&s), &t.0)
        .unwrap()
        .unwrap();
    assert!(out
        .content
        .contains("<skill name=\"q\" description=\"a &quot;b&quot; &amp; c\" />"));
    fs::write(&out.output_path, &out.content).unwrap();
    assert_eq!(agents_md::clean(&t.0).unwrap().len(), 1);
    assert_eq!(
        fs::read_to_string(t.0.join("AGENTS.md")).unwrap(),
        "# Mine\n\nKeep this.\n"
    );
}
