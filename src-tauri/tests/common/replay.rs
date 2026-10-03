//! Shared replay driver for the skills/agents/styles/findings parity tests: stages a golden
//! case's vendored inputs in a temp HOME, runs the area entry, snapshots what changed and
//! compares it with the golden tree.

#![allow(dead_code)]

use super::parity::*;
use conduit_lib::plus::skills::{FixedClock, Instant};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const FROZEN: &str = "20260101T000000Z";

pub fn frozen_clock() -> FixedClock {
    FixedClock(Instant {
        unix_secs: 1_767_225_600,
        micros: 0,
    })
}

pub fn inputs_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skills-core")
}

/// Skills inputs live at the top of `skills-core`; the other areas sit under their own folder.
pub fn case_inputs(area: &str, case: &str) -> PathBuf {
    match area {
        "skills" => inputs_root().join(case),
        other => inputs_root().join(other).join(case),
    }
}

pub fn copy_dir(from: &Path, to: &Path) {
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

pub fn snapshot(home: &Path) -> BTreeMap<String, Vec<u8>> {
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

pub fn sort_output_files(text: &str) -> String {
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

/// Files an entry reports in addition to what it wrote under HOME (the `_cleaned.txt` reports).
pub type Extras = Vec<(String, String)>;

pub fn replay(area: &str, case: &str, entry: &dyn Fn(&Path, &Value, &FixedClock) -> Extras) {
    let dir = case_inputs(area, case);
    let spec: Value = serde_json::from_slice(&fs::read(dir.join("case.json")).unwrap()).unwrap();
    let args = &spec["args"];
    if let Some(frozen) = args["freeze_timestamp"].as_str() {
        assert_eq!(
            frozen, FROZEN,
            "{case}: clock below is pinned to this instant"
        );
    }
    let root = std::env::temp_dir().join(format!("parity-{area}-{}-{case}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("home")).unwrap();
    // Python resolves symlinks in hook paths and the case roots; macOS /var is a symlink.
    let root = fs::canonicalize(&root).unwrap();
    let home = root.join("home");
    if let Some(shared) = spec["input_from"].as_str() {
        copy_dir(&inputs_root().join(shared), &home);
    }
    if dir.join("input").is_dir() {
        copy_dir(&dir.join("input"), &home);
    }
    let before = snapshot(&home);
    let extras = entry(&home, args, &frozen_clock());

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
    for (rel, text) in extras {
        let out = actual.join(rel);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(out, text).unwrap();
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

    let golden_dir = fixtures_root().join(area).join(case);
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
        // Findings goldens are class C (contract) but are canonical JSON emitted by the entry,
        // so they are compared as class A here and any divergence is a real finding change.
        let class = match golden.classes[&rel].as_str() {
            "C" => "A",
            other => other,
        };
        classes.insert(rel.clone(), serde_json::json!({ "class": class }));
    }
    let manifest = serde_json::json!({ "comparison": golden.comparison, "files": classes });
    fs::write(filtered.join("manifest.json"), manifest.to_string()).unwrap();
    let filtered_case = GoldenCase::load(&filtered).unwrap();

    let result = compare_case(&filtered_case, &actual, &n);
    let _ = fs::remove_dir_all(&root);
    if let Err(problems) = result {
        panic!("{area}/{case}: diverges from mcpm golden\n{problems}");
    }
}

pub fn assert_vendored(area: &str, cases: &[&str]) {
    for case in cases {
        assert!(
            case_inputs(area, case).join("case.json").is_file(),
            "{area}/{case}"
        );
        assert!(
            fixtures_root()
                .join(area)
                .join(case)
                .join("manifest.json")
                .is_file(),
            "{area}/{case}"
        );
    }
}
