//! Replays the mcpm golden INPUTS for the wave-1 transpiler cases (claude-code, agents-md, cursor,
//! windsurf, cline, hooks) through the real transpilers and compares every produced file with the
//! golden tree, including the lock and settings.json (class B canonical JSON).

mod common;

use common::parity::*;
use conduit_lib::plus::skills::transpilers::register_wave1;
use conduit_lib::plus::skills::{
    discover_skills, save_lockfile, sync_skills, FixedClock, Instant, SyncOptions,
    TranspilerRegistry,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const CASES: &[&str] = &[
    "claude-code-project",
    "claude-code-global",
    "cursor-project",
    "windsurf-project",
    "cline-project",
    "cline-global",
    "agents-md-project",
    "agents-md-append-existing",
    "agents-md-xml-escape",
    "append-global-skipped",
    "hooks-revoke-project",
    "hooks-revoke-global",
    "hooks-install-existing-settings",
    "hooks-unsupported-client",
    "assets-changed-hash",
];

const FROZEN: &str = "20260101T000000Z";

fn inputs_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skills-core")
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
    clock: &FixedClock,
) {
    let global = args["global"].as_bool().unwrap_or(false);
    let repo = home.join(args["repo"].as_str().unwrap());
    let wanted: Vec<String> = args["clients"]
        .as_array()
        .map(|a| a.iter().map(|c| c.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    let mut registry = TranspilerRegistry::new();
    register_wave1(&mut registry);
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
    // D-027 divergence: goldens use mcpm's narrower asset allowlist; see parity_skills_core.rs.
    let result = conduit_lib::plus::skills::with_asset_policy(
        conduit_lib::plus::skills::AssetPolicy::Mcpm,
        || sync_skills(&discover_skills(&repo), &registry, &opts),
    )
    .unwrap();
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
        std::env::temp_dir().join(format!("parity-skills-wave1-{}-{case}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("home")).unwrap();
    // Hook commands are symlink-resolved like Python's Path.resolve(); macOS /var is a symlink.
    let root = fs::canonicalize(&root).unwrap();
    let home = root.join("home");
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

    run_sync_pass(&home, args, &clock);
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
        let before_second: std::collections::BTreeSet<String> = snapshot(&home).into_keys().collect();
        run_sync_pass(&home, args, &clock);
        let after: std::collections::BTreeSet<String> = snapshot(&home).into_keys().collect();
        let gone: Vec<String> = before_second.difference(&after).cloned().collect();
        cleaned_report = Some(gone.iter().map(|r| format!("{r}\n")).collect::<String>());
    }

    let excludes: Vec<String> = spec["exclude"]
        .as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    let skip = |rel: &str| excludes.iter().any(|p| glob_match(p, rel));

    let capture_all = spec["capture"].as_str() == Some("all");
    let after = snapshot(&home);
    let actual = root.join("actual");
    for (rel, data) in &after {
        if skip(rel) || (!capture_all && before.get(rel) == Some(data)) {
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
fn claude_code_matches_golden() {
    for case in [
        "claude-code-project",
        "claude-code-global",
        "assets-changed-hash",
    ] {
        replay(case);
    }
}

#[test]
fn hooks_match_golden() {
    for case in [
        "hooks-revoke-project",
        "hooks-revoke-global",
        "hooks-install-existing-settings",
        "hooks-unsupported-client",
    ] {
        replay(case);
    }
}

#[test]
fn agents_md_matches_golden() {
    for case in [
        "agents-md-project",
        "agents-md-append-existing",
        "agents-md-xml-escape",
        "append-global-skipped",
    ] {
        replay(case);
    }
}

#[test]
fn cursor_windsurf_cline_match_golden() {
    for case in [
        "cursor-project",
        "windsurf-project",
        "cline-project",
        "cline-global",
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
