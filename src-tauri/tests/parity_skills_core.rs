//! Replays the mcpm golden INPUTS for the stale-cleanup, collision and asset cases through the
//! skills core with fixture transpilers. Transpiler output itself is out of scope here (SKL-3/4),
//! so every file a fixture transpiler writes is excluded from both sides; what remains is the
//! lock, `_cleaned.txt`, `_deleted.txt`, backups with INDEX.json and the copied assets.

mod common;

use common::parity::*;
use conduit_lib::plus::skills::collisions::Resolution;
use conduit_lib::plus::skills::parser::Activation;
use conduit_lib::plus::skills::transpiler::TranspileResult;
use conduit_lib::plus::skills::{
    discover_skills, save_lockfile, sync_skills, with_asset_policy, AssetPolicy, FixedClock,
    Instant, Skill, SkillType, SyncOptions, Transpiler, TranspilerRegistry,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const CASES: &[&str] = &[
    "stale-cleanup-project",
    "stale-cleanup-global",
    "collision-migrate-project",
    "collision-migrate-global",
    "collision-preexisting-output",
    "collision-warn-project",
    "assets-allowlist-project",
    "assets-allowlist-global",
    "assets-changed-hash",
];

const FROZEN: &str = "20260101T000000Z";

fn inputs_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skills-core")
}

#[derive(Clone, Copy)]
enum Kind {
    ClaudeCode,
    CodexCli,
    Cursor,
    GeminiCli,
}

struct Fixture {
    kind: Kind,
    home: PathBuf,
    written: Rc<RefCell<BTreeSet<PathBuf>>>,
}

impl Transpiler for Fixture {
    fn client_key(&self) -> &str {
        match self.kind {
            Kind::ClaudeCode => "claude-code",
            Kind::CodexCli => "codex-cli",
            Kind::Cursor => "cursor",
            Kind::GeminiCli => "gemini-cli",
        }
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        let output_path = self.get_output_path(skill, root);
        self.written.borrow_mut().insert(output_path.clone());
        let manual = skill.frontmatter.activation == Activation::Manual;
        let downgrade = match self.kind {
            Kind::CodexCli => manual,
            Kind::GeminiCli => manual && skill.skill_type == SkillType::Skill,
            _ => false,
        };
        let warnings = if downgrade {
            vec![format!(
                "{}: activation 'manual' downgraded to 'agent'",
                self.client_key()
            )]
        } else {
            Vec::new()
        };
        Ok(TranspileResult {
            output_path,
            content: "fixture\n".into(),
            warnings,
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        let n = skill.name();
        match self.kind {
            Kind::ClaudeCode if skill.skill_type == SkillType::Rule => {
                root.join(".claude/rules").join(format!("{n}.md"))
            }
            Kind::ClaudeCode => root.join(".claude/skills").join(n).join("SKILL.md"),
            Kind::CodexCli if root == self.home => {
                root.join(".codex/skills").join(n).join("SKILL.md")
            }
            Kind::CodexCli => root.join(".agents/skills").join(n).join("SKILL.md"),
            Kind::Cursor => root.join(".cursor/rules").join(n).join("RULE.md"),
            Kind::GeminiCli => root.join(".gemini/skills").join(n).join("SKILL.md"),
        }
    }

    fn get_collision_paths(&self, skill: &Skill, root: &Path) -> Vec<PathBuf> {
        let n = skill.name();
        match self.kind {
            Kind::ClaudeCode => vec![
                root.join(".claude/commands").join(format!("{n}.md")),
                root.join(".claude/agents").join(format!("{n}.md")),
            ],
            Kind::Cursor => vec![
                root.join(".cursor/rules").join(format!("{n}.md")),
                root.join(".cursor/rules").join(format!("{n}.mdc")),
            ],
            _ => Vec::new(),
        }
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn snapshot(home: &Path) -> BTreeMap<String, Vec<u8>> {
    list_files(home)
        .unwrap()
        .into_iter()
        .map(|(rel, path)| (rel, fs::read(path).unwrap()))
        .collect()
}

fn glob_match(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, tail)) => {
            text.starts_with(head)
                && (0..=text.len() - head.len()).any(|i| {
                    text.is_char_boundary(head.len() + i)
                        && glob_match(tail, &text[head.len() + i..])
                })
        }
    }
}

fn run_sync_pass(
    home: &Path,
    args: &Value,
    written: &Rc<RefCell<BTreeSet<PathBuf>>>,
    clock: &FixedClock,
) {
    let global = args["global"].as_bool().unwrap_or(false);
    let repo = home.join(args["repo"].as_str().unwrap());
    let wanted: Vec<String> = args["clients"]
        .as_array()
        .map(|a| a.iter().map(|c| c.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    let mut registry = TranspilerRegistry::new();
    for kind in [
        Kind::ClaudeCode,
        Kind::CodexCli,
        Kind::Cursor,
        Kind::GeminiCli,
    ] {
        registry.register(Box::new(Fixture {
            kind,
            home: home.to_path_buf(),
            written: written.clone(),
        }));
    }
    let lock_dir = if global {
        home.join(".config/mcpm")
    } else {
        repo.clone()
    };
    let opts = SyncOptions {
        output_root: if global {
            home.to_path_buf()
        } else {
            repo.clone()
        },
        lock_dir: lock_dir.clone(),
        global_mode: global,
        dry_run: false,
        migrate: args["migrate"].as_bool(),
        client_keys: (!wanted.is_empty()).then_some(wanted),
        clock,
    };
    // D-027 divergence: the goldens were recorded with mcpm's narrower allowlist (no .html/.csv/.js),
    // so the replay pins that policy; the widened default is covered by non-golden tests in
    // plus::skills::tests (default_allowlist_adds_html_csv_js_but_not_zip,
    // sync_copies_templates_balie_html_to_the_client_skill_dir).
    let result = with_asset_policy(AssetPolicy::Mcpm, || {
        sync_skills(&discover_skills(&repo), &registry, &opts)
    })
    .unwrap();
    for Resolution { backup_path, .. } in &result.collisions.resolutions {
        if let Some(p) = backup_path {
            assert!(p.starts_with(&opts.output_root));
        }
    }
    save_lockfile(&lock_dir, &result.lockfile).unwrap();
}

fn sort_output_files(text: &str) -> String {
    let mut v: Value = serde_json::from_str(text).unwrap();
    if let Some(root) = v.as_object_mut() {
        for bucket in ["skills", "rules", "agents", "styles"] {
            let Some(entries) = root.get_mut(bucket).and_then(Value::as_object_mut) else {
                continue;
            };
            for entry in entries.values_mut() {
                let Some(files) = entry.get_mut("output_files").and_then(Value::as_object_mut)
                else {
                    continue;
                };
                for list in files.values_mut() {
                    if let Some(arr) = list.as_array_mut() {
                        arr.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                    }
                }
            }
        }
    }
    serde_json::to_string_pretty(&v).unwrap()
}

fn replay(case: &str) {
    let dir = inputs_root().join(case);
    let spec: Value = serde_json::from_slice(&fs::read(dir.join("case.json")).unwrap()).unwrap();
    let args = &spec["args"];
    if let Some(frozen) = args["freeze_timestamp"].as_str() {
        assert_eq!(
            frozen, FROZEN,
            "{case}: clock below is pinned to this instant"
        );
    }
    let root =
        std::env::temp_dir().join(format!("parity-skills-core-{}-{case}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    if let Some(shared) = spec["input_from"].as_str() {
        copy_dir(&inputs_root().join(shared), &home);
    }
    if dir.join("input").is_dir() {
        copy_dir(&dir.join("input"), &home);
    }
    let before = snapshot(&home);
    let clock = FixedClock(Instant {
        unix_secs: 1_767_225_600,
        micros: 0,
    });
    let written = Rc::new(RefCell::new(BTreeSet::new()));

    run_sync_pass(&home, args, &written, &clock);
    let mut cleaned_report = None;
    if let Some(remove) = args["then_remove"].as_array() {
        for rel in remove {
            let p = home.join(rel.as_str().unwrap());
            if p.is_dir() {
                fs::remove_dir_all(&p).unwrap();
            } else if p.exists() {
                fs::remove_file(&p).unwrap();
            }
        }
        let before_second: BTreeSet<String> = snapshot(&home).into_keys().collect();
        run_sync_pass(&home, args, &written, &clock);
        let after: BTreeSet<String> = snapshot(&home).into_keys().collect();
        let gone: Vec<String> = before_second.difference(&after).cloned().collect();
        cleaned_report = Some(gone.iter().map(|r| format!("{r}\n")).collect::<String>());
    }

    let rel_written: BTreeSet<String> = written
        .borrow()
        .iter()
        .map(|p| {
            p.strip_prefix(&home)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let excludes: Vec<String> = spec["exclude"]
        .as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    let skip = |rel: &str| rel_written.contains(rel) || excludes.iter().any(|p| glob_match(p, rel));

    let after = snapshot(&home);
    let actual = root.join("actual");
    for (rel, data) in &after {
        if skip(rel) || before.get(rel) == Some(data) {
            continue;
        }
        let out = actual.join(rel);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(out, data).unwrap();
    }
    let deleted: Vec<&String> = before
        .keys()
        .filter(|r| !after.contains_key(*r) && !skip(r))
        .collect();
    if !deleted.is_empty() {
        let text: String = deleted.iter().map(|r| format!("{r}\n")).collect();
        fs::write(actual.join("_deleted.txt"), text).unwrap();
    }
    if let Some(text) = cleaned_report {
        fs::write(actual.join("_cleaned.txt"), text).unwrap();
    }
    let n = Normalizers {
        root: Some(root.to_string_lossy().into_owned()),
        home: Some(home.to_string_lossy().into_owned()),
        ignore_json_keys: Vec::new(),
    };
    let sync_re = regex::Regex::new(r#"("synced_at"\s*:\s*)"[^"]*""#).unwrap();
    for (rel, path) in list_files(&actual).unwrap() {
        if rel.ends_with("mcpm-skills.lock") {
            let text = fs::read_to_string(&path).unwrap();
            let text = sync_re.replace(&text, r#"$1"<SYNCED_AT>""#).into_owned();
            fs::write(&path, sort_output_files(&text)).unwrap();
        }
    }

    let golden_dir = fixtures_root().join("skills").join(case);
    let golden = GoldenCase::load(&golden_dir).unwrap();
    let filtered = root.join("golden");
    let mut classes = serde_json::Map::new();
    for (rel, path) in list_files(&golden.tree()).unwrap() {
        if skip(&rel) {
            continue;
        }
        let out = filtered.join("tree").join(&rel);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        let mut bytes = fs::read(path).unwrap();
        if rel.ends_with("mcpm-skills.lock") {
            bytes = sort_output_files(std::str::from_utf8(&bytes).unwrap()).into_bytes();
        }
        fs::write(out, bytes).unwrap();
        classes.insert(
            rel.clone(),
            serde_json::json!({ "class": golden.classes[&rel] }),
        );
    }
    let manifest = serde_json::json!({ "comparison": golden.comparison, "files": classes });
    fs::write(filtered.join("manifest.json"), manifest.to_string()).unwrap();
    let filtered_case = GoldenCase::load(&filtered).unwrap();

    let result = compare_case(&filtered_case, &actual, &n);
    let _ = fs::remove_dir_all(&root);
    if let Err(problems) = result {
        panic!("{case}: skills core diverges from mcpm golden\n{problems}");
    }
}

#[test]
fn stale_cleanup_matches_golden() {
    for case in ["stale-cleanup-project", "stale-cleanup-global"] {
        replay(case);
    }
}

#[test]
fn collisions_match_golden() {
    for case in [
        "collision-migrate-project",
        "collision-migrate-global",
        "collision-preexisting-output",
        "collision-warn-project",
    ] {
        replay(case);
    }
}

#[test]
fn asset_copy_and_hash_match_golden() {
    for case in [
        "assets-allowlist-project",
        "assets-allowlist-global",
        "assets-changed-hash",
    ] {
        replay(case);
    }
}

#[test]
fn every_declared_case_has_vendored_inputs_and_a_golden() {
    for case in CASES {
        assert!(
            inputs_root().join(case).join("case.json").is_file(),
            "{case}"
        );
        assert!(
            fixtures_root()
                .join("skills")
                .join(case)
                .join("manifest.json")
                .is_file(),
            "{case}"
        );
    }
}
