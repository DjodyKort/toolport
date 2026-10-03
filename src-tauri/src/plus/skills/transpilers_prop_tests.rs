//! Seeded randomized and edge-case tests for the per-client skill, agent and style transpilers
//! and the managed-block helpers: no panics, deterministic output, outputs stay under the root,
//! managed blocks are stable and removable, and the structured formats re-parse.

use super::agents::{all_agent_transpilers, Agent, PermissionMode};
use super::json::{self, J};
use super::parser::{parse_frontmatter, Activation, Skill, SkillType};
use super::pyfs::universal_newlines;
use super::styles::{all_style_transpilers, Style, StyleTranspiler};
use super::transpiler::{
    inject_managed_block, Transpiler, TranspilerRegistry, MCPM_BLOCK_END, MCPM_BLOCK_START,
};
use super::transpilers::windsurf::{py_prefix, WINDSURF_WORKSPACE_CHAR_LIMIT};
use super::transpilers::{register_all_with_home, register_vscode_copilot};
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use serde_yaml::Value;
use std::fs;
use std::path::{Component, Path};

const SAFE_TEXT: &str = "abcXYZ 019.,;:!?()#@/+_'é日🙂";

fn registry(home: &Path) -> TranspilerRegistry {
    let mut reg = TranspilerRegistry::new();
    register_all_with_home(&mut reg, Some(home.to_path_buf()));
    register_vscode_copilot(&mut reg);
    reg
}

fn nonempty(rng: &mut Rng, max: usize) -> String {
    let mut text = rng.garbage(max);
    text.push('d');
    text
}

fn safe_text(rng: &mut Rng, max: usize) -> String {
    let mut text = rng.string(SAFE_TEXT, max).replace("---", "-_-");
    text.push('d');
    text
}

fn body_text(rng: &mut Rng) -> String {
    match rng.below(10) {
        0 => String::new(),
        1 => "x".repeat(rng.range(11_000, 14_000)),
        _ => rng.garbage(160),
    }
}

fn skill(rng: &mut Rng, safe: bool) -> Skill {
    let name = format!("n-{}", rng.slug(28));
    let mut s = Skill::placeholder(&name, *rng.pick(&[SkillType::Skill, SkillType::Rule]));
    let fm = &mut s.frontmatter;
    fm.description = if safe {
        safe_text(rng, 40)
    } else {
        nonempty(rng, 60)
    };
    fm.activation = *rng.pick(&[
        Activation::Always,
        Activation::Auto,
        Activation::Agent,
        Activation::Manual,
    ]);
    fm.globs = match rng.below(4) {
        0 => None,
        1 => Some(String::new()),
        2 => Some(rng.string("ab*/._-, ", 24)),
        _ => Some("**/*.py, src/**".into()),
    };
    fm.allowed_tools = rng.chance(40).then(|| rng.string("ab(): *,", 12));
    fm.priority = rng.below(20) as i64 - 5;
    s.body = body_text(rng);
    s
}

fn agent(rng: &mut Rng, safe: bool) -> Agent {
    let mut a = Agent::placeholder(&format!("n-{}", rng.slug(28)));
    let fm = &mut a.frontmatter;
    fm.description = if safe {
        safe_text(rng, 40)
    } else {
        nonempty(rng, 60)
    };
    let word = |rng: &mut Rng| rng.slug(8);
    fm.model = rng.chance(50).then(|| word(rng));
    fm.effort = rng.chance(30).then(|| word(rng));
    fm.color = rng.chance(30).then(|| word(rng));
    fm.tools = (0..rng.below(4))
        .map(|_| {
            rng.pick(&["Read", "Write", "Edit", "Bash", "Glob", "Browser", "Other"])
                .to_string()
        })
        .collect();
    fm.disallowed_tools = (0..rng.below(3)).map(|_| word(rng)).collect();
    fm.mcp_servers = (0..rng.below(3)).map(|i| format!("srv{i}")).collect();
    fm.skills = (0..rng.below(3)).map(|_| word(rng)).collect();
    fm.max_turns = rng.chance(50).then(|| rng.below(30) as i64 - 3);
    fm.readonly = rng.chance(30);
    fm.permission_mode = match rng.below(4) {
        0 => Some(PermissionMode::Default),
        1 => Some(PermissionMode::Plan),
        2 => Some(PermissionMode::FullAuto),
        _ => None,
    };
    a.body = body_text(rng);
    a
}

fn style(rng: &mut Rng, safe: bool) -> Style {
    let mut s = Style::placeholder(&format!("n-{}", rng.slug(28)));
    s.frontmatter.description = if safe {
        safe_text(rng, 40)
    } else {
        nonempty(rng, 60)
    };
    s.frontmatter.keep_coding_instructions = rng.chance(50);
    s.body = body_text(rng);
    s
}

fn stays_under(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
        && path
            .components()
            .all(|c| !matches!(c, Component::ParentDir | Component::CurDir))
}

#[test]
fn every_skill_transpiler_is_total_deterministic_and_stays_under_the_root() {
    let tmp = ScratchDir::new("skill-transpilers");
    let reg = registry(tmp.path());
    assert!(reg.len() >= 15);
    run_cases("skills-transpile-all-clients", 600, |_, rng| {
        let s = skill(rng, false);
        for t in reg.all() {
            let first = t.transpile(&s, tmp.path());
            assert_eq!(first, t.transpile(&s, tmp.path()), "{}", t.client_key());
            let out = first.unwrap_or_else(|e| panic!("{}: {e}", t.client_key()));
            assert!(
                stays_under(tmp.path(), &out.output_path),
                "{}",
                t.client_key()
            );
            assert_eq!(
                out.output_path,
                t.get_output_path(&s, tmp.path()),
                "{}",
                t.client_key()
            );
            for p in t.get_collision_paths(&s, tmp.path()) {
                assert!(stays_under(tmp.path(), &p), "{}", t.client_key());
            }
        }
    });
}

#[test]
fn skill_transpilers_handle_degenerate_skills() {
    let tmp = ScratchDir::new("skill-degenerate");
    let reg = registry(tmp.path());
    let long = "é".repeat(30_000);
    for body in [
        "",
        "\n\n\n",
        "---",
        "---\n---",
        &long,
        "\u{0}\u{1b}[31m",
        "<!-- mcpm:start -->",
    ] {
        for desc in ["d", "\"quoted\"", "multi\nline", "a: b # c", &long] {
            let mut s = Skill::placeholder("edge", SkillType::Skill);
            s.body = body.to_string();
            s.frontmatter.description = desc.to_string();
            s.frontmatter.globs = Some(long.chars().take(40).collect());
            for t in reg.all() {
                for kind in [SkillType::Skill, SkillType::Rule] {
                    s.skill_type = kind;
                    assert!(t.transpile(&s, tmp.path()).is_ok(), "{}", t.client_key());
                }
            }
        }
    }
}

fn paths_of(content: &str) -> Vec<String> {
    let (fields, _) = parse_frontmatter(content).unwrap();
    match fields.iter().find(|(k, _)| k == "paths") {
        Some((_, Value::Sequence(items))) => items
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect(),
        other => panic!("paths missing or not a list: {other:?}"),
    }
}

#[test]
fn claude_code_output_re_parses_to_the_same_model() {
    let tmp = ScratchDir::new("claude-reparse");
    let reg = registry(tmp.path());
    let claude = reg.get("claude-code").unwrap();
    run_cases("skills-claude-reparse", 600, |_, rng| {
        let mut s = skill(rng, true);
        s.frontmatter.allowed_tools = None;
        s.frontmatter.globs = match rng.below(3) {
            1 if s.skill_type == SkillType::Rule => Some(rng.string("ab*/._, ", 24)),
            _ => None,
        };
        s.body = rng.garbage(80);
        let out = claude.transpile(&s, tmp.path()).unwrap();
        let globs: Vec<String> = s
            .frontmatter
            .globs
            .iter()
            .flat_map(|g| g.split(','))
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty())
            .collect();
        if s.skill_type == SkillType::Rule {
            if s.frontmatter
                .globs
                .as_deref()
                .is_some_and(|g| !g.is_empty())
            {
                assert_eq!(paths_of(&out.content), globs, "{}", out.content);
                let (_, body) = parse_frontmatter(&out.content).unwrap();
                assert_eq!(body, s.body.trim());
            } else {
                assert_eq!(out.content, format!("{}\n", s.body));
            }
        } else {
            let (fields, body) = parse_frontmatter(&out.content).unwrap();
            let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone());
            assert_eq!(get("name"), Some(Value::String(s.name().to_string())));
            assert_eq!(
                get("description"),
                Some(Value::String(s.frontmatter.description.clone()))
            );
            assert_eq!(body, s.body.trim());
        }
    });
}

#[test]
fn windsurf_truncation_respects_the_character_limit() {
    let tmp = ScratchDir::new("windsurf-limit");
    let reg = registry(tmp.path());
    let windsurf = reg.get("windsurf").unwrap();
    run_cases("skills-windsurf-limit", 300, |_, rng| {
        let mut s = skill(rng, false);
        s.frontmatter.globs = Some(rng.string("ab*/.", 40));
        s.frontmatter.description = rng.garbage(120);
        let (len, times) = (rng.range(0, 24_000), rng.range(1, 4));
        s.body = rng.garbage(len).repeat(times);
        let out = windsurf.transpile(&s, tmp.path()).unwrap();
        let chars = out.content.chars().count();
        assert!(chars <= WINDSURF_WORKSPACE_CHAR_LIMIT + 50, "{chars}");
        let truncated = out
            .content
            .contains("[truncated -- see full skill at source]");
        assert_eq!(truncated, !out.warnings.is_empty());
        if !truncated {
            assert!(chars <= WINDSURF_WORKSPACE_CHAR_LIMIT);
            assert!(out.content.ends_with(&format!("{}\n", s.body)));
        }
    });
}

#[test]
fn py_prefix_matches_python_slicing() {
    run_cases("skills-py-prefix", 1500, |_, rng| {
        let text = rng.garbage(30);
        let total = text.chars().count() as isize;
        let n = rng.range(0, 80) as isize - 40;
        let got = py_prefix(&text, n);
        let want = if n < 0 {
            (total + n).max(0)
        } else {
            n.min(total)
        } as usize;
        assert_eq!(got.chars().count(), want);
        assert!(text.starts_with(&got));
    });
    assert_eq!(py_prefix("", 5), "");
    assert_eq!(py_prefix("héllo", -1), "héll");
    assert_eq!(py_prefix("héllo", -9), "");
}

fn block_text(rng: &mut Rng) -> String {
    const VOCAB: &[&str] = &[
        MCPM_BLOCK_START,
        MCPM_BLOCK_END,
        "\n",
        "\n\n",
        " ",
        "text",
        "# Title\n",
        "\t",
        "é",
        "<!--",
    ];
    rng.tokens(VOCAB, 8)
}

#[test]
fn managed_block_injection_is_total_and_contains_the_block() {
    run_cases("skills-inject-block", 4000, |_, rng| {
        let existing = if rng.chance(50) {
            block_text(rng)
        } else {
            rng.garbage(40)
        };
        let block = rng.garbage(30);
        let out = inject_managed_block(&existing, &block);
        assert_eq!(
            out,
            inject_managed_block(&existing, &block),
            "deterministic"
        );
        assert!(out.ends_with('\n'));
        assert!(out.contains(&format!("{MCPM_BLOCK_START}\n{block}\n{MCPM_BLOCK_END}")));
    });
}

#[test]
fn managed_block_injection_is_idempotent_after_foreign_text_when_nothing_follows() {
    run_cases("skills-inject-idempotent", 3000, |_, rng| {
        let mut foreign = rng.garbage(60).replace("mcpm:", "m:");
        foreign.push('x');
        let block = rng.garbage(30);
        let first = inject_managed_block(&foreign, &block);
        assert_eq!(inject_managed_block(&first, &block), first, "{first:?}");
        let other = rng.garbage(30);
        let swapped = inject_managed_block(&first, &other);
        assert_eq!(inject_managed_block(&swapped, &other), swapped);
        assert_eq!(swapped.matches(MCPM_BLOCK_START).count(), 1);
        assert!(swapped.starts_with(foreign.trim_end()));
    });
}

#[test]
fn a_block_only_file_gets_no_leading_blank_lines() {
    run_cases("skills-inject-block-only", 300, |_, rng| {
        let block = rng.garbage(30);
        let blank = rng.string(" \n\t", 4);
        let first = inject_managed_block(&blank, &block);
        assert!(first.starts_with(MCPM_BLOCK_START));
        assert_eq!(inject_managed_block(&first, &block), first);
        assert_eq!(
            inject_managed_block(&format!("{blank}{first}"), &block),
            first
        );
    });
}

#[test]
fn text_after_the_managed_block_keeps_one_trailing_newline_across_injections() {
    let block = "b";
    let once = inject_managed_block(
        &format!("{MCPM_BLOCK_START}\nold\n{MCPM_BLOCK_END}\nafter\n"),
        block,
    );
    assert_eq!(
        once,
        format!("{MCPM_BLOCK_START}\nb\n{MCPM_BLOCK_END}\n\nafter\n")
    );
    let mut text = once.clone();
    for _ in 0..5 {
        text = inject_managed_block(&text, block);
        assert_eq!(text, once);
    }
}

fn user_text(rng: &mut Rng) -> String {
    const VOCAB: &[&str] = &[
        "text",
        "# Title\n",
        "- item",
        "\n",
        "\n\n",
        "\r\n",
        " ",
        "\t",
        "é",
        "<!-- note -->",
    ];
    let text = if rng.chance(50) {
        rng.tokens(VOCAB, 8)
    } else {
        rng.garbage(60)
    };
    text.replace("mcpm:", "m:")
}

#[test]
fn repeated_injection_is_byte_stable_and_keeps_the_text_around_the_block() {
    run_cases("skills-inject-stable", 6000, |_, rng| {
        let before = user_text(rng);
        let after = user_text(rng);
        let old = user_text(rng);
        let block = user_text(rng);
        let managed = format!("{MCPM_BLOCK_START}\n{block}\n{MCPM_BLOCK_END}");
        let with_block = rng.chance(70);
        let existing = if with_block {
            let inner = format!("{MCPM_BLOCK_START}\n{old}\n{MCPM_BLOCK_END}");
            format!("{before}{inner}{after}")
        } else {
            before.clone()
        };
        let after = if with_block { after.as_str() } else { "" };
        let mut expected = String::new();
        if !before.trim_end().is_empty() {
            expected.push_str(before.trim_end());
            expected.push_str("\n\n");
        }
        expected.push_str(&managed);
        if !after.trim().is_empty() {
            expected.push_str("\n\n");
            expected.push_str(after.trim());
        }
        expected.push('\n');
        let first = inject_managed_block(&existing, &block);
        assert_eq!(first, expected, "{existing:?}");
        let mut text = first.clone();
        for round in 0..rng.range(1, 6) {
            text = inject_managed_block(&text, &block);
            assert_eq!(text, first, "round {round}");
        }
    });
}

#[test]
fn injection_settles_after_two_rounds_whatever_markers_the_text_holds() {
    run_cases("skills-inject-settles", 4000, |_, rng| {
        let existing = block_text(rng);
        let block = rng.garbage(30).replace("mcpm:", "m:");
        let first = inject_managed_block(&existing, &block);
        let second = inject_managed_block(&first, &block);
        assert_eq!(
            inject_managed_block(&second, &block),
            second,
            "{existing:?}"
        );
        let matched = existing
            .find(MCPM_BLOCK_START)
            .is_none_or(|s| existing[s..].contains(MCPM_BLOCK_END));
        if matched {
            assert_eq!(second, first, "{existing:?}");
        }
    });
}

fn rewrite_append_mode(t: &dyn Transpiler, skills: &[Skill], root: &Path) -> String {
    let out = t.transpile_all(skills, root).unwrap().unwrap();
    fs::write(&out.output_path, &out.content).unwrap();
    out.content
}

#[test]
fn append_mode_skill_files_are_stable_and_clean_restores_foreign_text() {
    let tmp = ScratchDir::new("append-skills");
    let reg = registry(tmp.path());
    run_cases("skills-append-mode", 300, |_, rng| {
        tmp.reset();
        let foreign = if rng.chance(30) {
            String::new()
        } else {
            rng.garbage(80).replace('\0', "")
        };
        let skills: Vec<Skill> = (0..rng.below(4)).map(|_| skill(rng, false)).collect();
        for key in ["zed", "agents-md"] {
            let t = reg.get(key).unwrap();
            let file = t.get_output_path(
                &skills
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Skill::placeholder("x", SkillType::Skill)),
                tmp.path(),
            );
            let _ = fs::remove_file(&file);
            if !foreign.is_empty() {
                fs::write(&file, &foreign).unwrap();
            }
            let first = rewrite_append_mode(t, &skills, tmp.path());
            assert_eq!(first.matches(MCPM_BLOCK_START).count(), 1);
            let second = rewrite_append_mode(t, &skills, tmp.path());
            assert_eq!(second, first, "{key} idempotent");
            assert_eq!(rewrite_append_mode(t, &skills, tmp.path()), first, "{key}");
            let removed = t.clean(tmp.path(), &[]).unwrap();
            assert_eq!(removed.len(), 1);
            let expected = universal_newlines(&foreign);
            if expected.trim().is_empty() {
                assert!(!file.exists(), "{key}");
            } else {
                assert_eq!(
                    fs::read_to_string(&file).unwrap(),
                    format!("{}\n", expected.trim()),
                    "{key}"
                );
            }
            assert!(t.clean(tmp.path(), &[]).unwrap().is_empty());
        }
    });
}

#[test]
fn skill_clean_ignores_missing_and_unmarked_files() {
    let tmp = ScratchDir::new("clean-missing");
    let reg = registry(tmp.path());
    for key in ["zed", "agents-md"] {
        let t = reg.get(key).unwrap();
        assert!(t.clean(tmp.path(), &["x".into()]).unwrap().is_empty());
    }
    fs::write(tmp.path().join("AGENTS.md"), "# mine\n").unwrap();
    fs::write(
        tmp.path().join(".rules"),
        format!("{MCPM_BLOCK_START} only a start"),
    )
    .unwrap();
    for key in ["zed", "agents-md"] {
        assert!(reg
            .get(key)
            .unwrap()
            .clean(tmp.path(), &[])
            .unwrap()
            .is_empty());
    }
    assert_eq!(
        fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap(),
        "# mine\n"
    );
}

#[test]
fn every_agent_transpiler_is_total_and_deterministic() {
    let tmp = ScratchDir::new("agent-transpilers");
    let all = all_agent_transpilers();
    run_cases("agents-transpile-all-clients", 600, |_, rng| {
        let a = agent(rng, false);
        for t in &all {
            let first = t.transpile(&a, tmp.path());
            assert_eq!(first, t.transpile(&a, tmp.path()), "{}", t.client_key());
            let out = first.unwrap_or_else(|e| panic!("{}: {e}", t.client_key()));
            assert!(
                stays_under(tmp.path(), &out.output_path),
                "{}",
                t.client_key()
            );
            assert_eq!(out.output_path, t.get_output_path(&a, tmp.path()));
        }
    });
}

fn toml_safe_body(rng: &mut Rng) -> String {
    rng.garbage(120)
        .chars()
        .filter(|c| {
            *c == '\n'
                || *c == '\t'
                || (*c >= ' ' && *c != '\u{7f}' && !('\u{80}'..'\u{a0}').contains(c))
        })
        .collect()
}

#[test]
fn codex_agent_output_is_valid_toml_and_carries_the_body() {
    let tmp = ScratchDir::new("codex-agent");
    let all = all_agent_transpilers();
    let codex = all.iter().find(|t| t.client_key() == "codex-cli").unwrap();
    run_cases("agents-codex-toml", 600, |_, rng| {
        let mut a = agent(rng, true);
        a.frontmatter.description = rng.string("abc XYZ.,:!?()é日", 30) + "d";
        a.body = toml_safe_body(rng);
        let out = codex.transpile(&a, tmp.path()).unwrap();
        let doc: toml::Value = out
            .content
            .parse()
            .unwrap_or_else(|e| panic!("{e}\n{}", out.content));
        assert_eq!(doc["name"].as_str(), Some(a.name()));
        assert_eq!(
            doc["description"].as_str(),
            Some(a.frontmatter.description.as_str())
        );
        assert_eq!(
            doc["developer_instructions"].as_str(),
            Some(format!("{}\n", a.body).as_str()),
            "{}",
            out.content
        );
    });
}

#[test]
fn codex_agent_mcp_server_hint_names_an_existing_toolportctl_command() {
    let tmp = ScratchDir::new("codex-agent-hint");
    let all = all_agent_transpilers();
    let codex = all.iter().find(|t| t.client_key() == "codex-cli").unwrap();
    let (command, _) =
        crate::plus::ctl::find_command(&["server".to_string(), "install".to_string()])
            .expect("ctl server install");
    assert!(!command.planned());
    run_cases("agents-codex-hint", 20, |_, rng| {
        let mut a = agent(rng, true);
        a.frontmatter.mcp_servers = vec!["docs".into(), "search".into()];
        let out = codex.transpile(&a, tmp.path()).unwrap();
        for server in ["docs", "search"] {
            let hint =
                format!("# Configure via toolportctl: toolportctl server install {server}\n");
            assert!(out.content.contains(&hint), "{}", out.content);
        }
        assert!(!out.content.contains("mcpm install"), "{}", out.content);
    });
}

#[test]
fn markdown_agent_outputs_re_parse_to_the_same_name_and_description() {
    let tmp = ScratchDir::new("md-agent");
    let all = all_agent_transpilers();
    run_cases("agents-md-reparse", 400, |_, rng| {
        let mut a = agent(rng, true);
        a.body = rng.garbage(80);
        for t in all
            .iter()
            .filter(|t| ["claude-code", "cursor", "gemini-cli", "vscode"].contains(&t.client_key()))
        {
            let out = t.transpile(&a, tmp.path()).unwrap();
            let (fields, body) = parse_frontmatter(&out.content)
                .unwrap_or_else(|e| panic!("{}: {e}\n{}", t.client_key(), out.content));
            let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone());
            assert_eq!(
                get("name"),
                Some(Value::String(a.name().into())),
                "{}",
                t.client_key()
            );
            assert_eq!(
                get("description"),
                Some(Value::String(a.frontmatter.description.clone())),
                "{}",
                t.client_key()
            );
            assert_eq!(body, a.body.trim(), "{}", t.client_key());
        }
    });
}

#[test]
fn roomodes_agents_round_trip_through_json() {
    let tmp = ScratchDir::new("roomodes-agents");
    let all = all_agent_transpilers();
    let roo = all.iter().find(|t| t.client_key() == "roomodes").unwrap();
    run_cases("agents-roomodes", 400, |_, rng| {
        let agents: Vec<Agent> = (0..rng.below(5)).map(|_| agent(rng, false)).collect();
        let out = roo.transpile_all(&agents, tmp.path()).unwrap().unwrap();
        let J::Obj(doc) = json::parse(&out.content).unwrap() else {
            panic!("not an object")
        };
        let J::Arr(modes) = &doc[0].1 else {
            panic!("customModes is not a list")
        };
        assert_eq!(modes.len(), agents.len());
        for (mode, a) in modes.iter().zip(&agents) {
            assert_eq!(mode.get("slug").and_then(J::as_str), Some(a.name()));
            assert_eq!(
                mode.get("roleDefinition").and_then(J::as_str),
                Some(a.frontmatter.description.as_str())
            );
            assert_eq!(
                mode.get("customInstructions").and_then(J::as_str),
                Some(a.body.as_str())
            );
        }
        assert_eq!(
            out.content,
            roo.transpile_all(&agents, tmp.path())
                .unwrap()
                .unwrap()
                .content
        );
    });
}

#[test]
fn every_style_transpiler_is_total_and_deterministic() {
    let tmp = ScratchDir::new("style-transpilers");
    let all = all_style_transpilers();
    run_cases("styles-transpile-all-clients", 500, |_, rng| {
        let s = style(rng, false);
        for t in &all {
            let first = t.transpile(&s, tmp.path());
            assert_eq!(first, t.transpile(&s, tmp.path()), "{}", t.client_key());
            let out = first.unwrap_or_else(|e| panic!("{}: {e}", t.client_key()));
            assert!(
                stays_under(tmp.path(), &out.output_path),
                "{}",
                t.client_key()
            );
            assert_eq!(out.output_path, t.get_output_path(&s, tmp.path()));
        }
    });
}

#[test]
fn windsurf_style_truncation_respects_the_character_limit() {
    let tmp = ScratchDir::new("windsurf-style");
    let all = all_style_transpilers();
    let windsurf = all.iter().find(|t| t.client_key() == "windsurf").unwrap();
    run_cases("styles-windsurf-limit", 200, |_, rng| {
        let mut s = style(rng, false);
        let (len, times) = (rng.range(0, 24_000), rng.range(1, 4));
        s.body = rng.garbage(len).repeat(times);
        let out = windsurf.transpile(&s, tmp.path()).unwrap();
        let chars = out.content.chars().count();
        assert!(chars <= WINDSURF_WORKSPACE_CHAR_LIMIT + 50, "{chars}");
        let truncated = out
            .content
            .contains("[truncated -- see full style at source]");
        assert_eq!(truncated, !out.warnings.is_empty());
    });
}

fn zed_style(all: &[Box<dyn StyleTranspiler>]) -> &dyn StyleTranspiler {
    all.iter()
        .find(|t| t.client_key() == "zed")
        .unwrap()
        .as_ref()
}

#[test]
fn zed_style_block_is_stable_and_clean_restores_foreign_text() {
    let tmp = ScratchDir::new("zed-style");
    let all = all_style_transpilers();
    let zed = zed_style(&all);
    run_cases("styles-zed-block", 400, |_, rng| {
        tmp.reset();
        let file = tmp.path().join(".rules");
        let foreign = if rng.chance(25) {
            String::new()
        } else {
            rng.garbage(80).replace('\0', "")
        };
        if !foreign.is_empty() {
            fs::write(&file, &foreign).unwrap();
        }
        let a = style(rng, false);
        let first = zed.transpile(&a, tmp.path()).unwrap();
        fs::write(&file, &first.content).unwrap();
        let again = zed.transpile(&a, tmp.path()).unwrap();
        if foreign.trim().is_empty() {
            fs::write(&file, &again.content).unwrap();
            let third = zed.transpile(&a, tmp.path()).unwrap();
            assert_eq!(third.content, again.content, "converges after one rewrite");
        } else {
            assert_eq!(again.content, first.content, "idempotent");
        }
        fs::write(&file, &again.content).unwrap();
        let removed = zed.clean(tmp.path(), &[]).unwrap();
        assert_eq!(removed.len(), 1);
        let expected = universal_newlines(&foreign);
        if expected.trim().is_empty() {
            assert!(!file.exists());
        } else {
            assert_eq!(
                fs::read_to_string(&file).unwrap(),
                format!("{}\n", expected.trim())
            );
        }
    });
}

fn mode_slugs(content: &str) -> Vec<String> {
    let J::Obj(doc) = json::parse(content).unwrap() else {
        panic!("not an object")
    };
    let J::Arr(modes) = &doc[0].1 else {
        panic!("customModes is not a list")
    };
    modes
        .iter()
        .map(|m| m.get("slug").and_then(J::as_str).unwrap().to_string())
        .collect()
}

#[test]
fn roo_style_modes_keep_foreign_modes_and_rewrite_only_style_ones() {
    let tmp = ScratchDir::new("roo-style");
    let all = all_style_transpilers();
    let roo = all
        .iter()
        .find(|t| t.client_key() == "roomodes-style")
        .unwrap();
    run_cases("styles-roomodes", 400, |_, rng| {
        tmp.reset();
        let file = tmp.path().join(".roomodes");
        let foreign: Vec<String> = (0..rng.below(4))
            .map(|i| format!("mode-{i}-{}", rng.slug(6)))
            .collect();
        let mut initial: Vec<J> = foreign
            .iter()
            .map(|slug| {
                J::Obj(vec![
                    ("slug".into(), J::str(slug)),
                    ("name".into(), J::str(rng.garbage(8))),
                ])
            })
            .collect();
        for i in 0..rng.below(3) {
            let old = J::Obj(vec![("slug".into(), J::str(format!("style-old{i}")))]);
            initial.insert(rng.below(initial.len() + 1), old);
        }
        let kept = match rng.below(5) {
            0 => Vec::new(),
            1 => {
                fs::write(&file, rng.garbage(60)).unwrap();
                Vec::new()
            }
            _ => {
                let doc = J::Obj(vec![("customModes".into(), J::Arr(initial))]);
                fs::write(&file, doc.dumps()).unwrap();
                foreign.clone()
            }
        };
        let styles: Vec<Style> = (0..rng.below(4)).map(|_| style(rng, false)).collect();
        let first = roo.transpile_all(&styles, tmp.path()).unwrap().unwrap();
        fs::write(&file, &first.content).unwrap();
        let again = roo.transpile_all(&styles, tmp.path()).unwrap().unwrap();
        assert_eq!(again.content, first.content, "idempotent");
        let mut expected = kept.clone();
        expected.extend(styles.iter().map(|s| format!("style-{}", s.name())));
        assert_eq!(mode_slugs(&first.content), expected);
        let removed = roo.clean(tmp.path(), &[]).unwrap();
        match fs::read_to_string(&file) {
            Ok(text) => assert_eq!(mode_slugs(&text), kept),
            Err(_) => {
                assert!(kept.is_empty() && !removed.is_empty());
            }
        }
    });
}

#[test]
fn style_and_agent_helpers_survive_extreme_names() {
    let tmp = ScratchDir::new("extreme-names");
    let all_a = all_agent_transpilers();
    let all_s = all_style_transpilers();
    for name in ["a", "0", &"a".repeat(64), "a-b-c", "x1-y2"] {
        let a = Agent::placeholder(name);
        for t in &all_a {
            assert!(t.transpile(&a, tmp.path()).is_ok());
            assert!(t.clean(tmp.path(), &[name.to_string()]).is_ok());
        }
        let s = Style::placeholder(name);
        for t in &all_s {
            assert!(t.transpile(&s, tmp.path()).is_ok());
            assert!(t.clean(tmp.path(), &[name.to_string()]).is_ok());
        }
    }
}

#[test]
fn numeric_and_keyword_names_are_emitted_unquoted_like_mcpm() {
    let tmp = ScratchDir::new("unquoted-names");
    let reg = registry(tmp.path());
    let claude = reg.get("claude-code").unwrap();
    for name in ["123", "true", "null", "1e5"] {
        let s = Skill::placeholder(name, SkillType::Skill);
        let out = claude.transpile(&s, tmp.path()).unwrap();
        assert!(
            out.content.contains(&format!("\nname: {name}\n")),
            "{}",
            out.content
        );
    }
}
