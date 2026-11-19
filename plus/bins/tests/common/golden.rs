#![allow(dead_code)]

//! Golden `--json` envelopes under `plus/fixtures/ctl-envelopes/` (and the self-MCP results under
//! `plus/fixtures/selfmcp-envelopes/`). A golden is compared as JSON (not text), so a formatter
//! run over the file does not matter. `CTL_ENVELOPE_BLESS=1` rewrites them and is rejected when
//! `CI` is set: a changed envelope is a reviewed change to the contract.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const BLESS_VAR: &str = "CTL_ENVELOPE_BLESS";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/ctl-envelopes")
}

pub fn selfmcp_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/selfmcp-envelopes")
}

/// `skills tap add` is stored as `skills-tap-add.json`.
pub fn file_stem(case: &str) -> String {
    case.split_whitespace().collect::<Vec<_>>().join("-")
}

pub fn path_of(case: &str) -> PathBuf {
    path_in(&root(), case)
}

pub fn path_in(dir: &Path, case: &str) -> PathBuf {
    let stem = file_stem(case);
    if cfg!(target_os = "macos") && PLATFORM_CASES.contains(&stem.as_str()) {
        return dir.join(format!("{stem}{MACOS_SUFFIX}"));
    }
    dir.join(format!("{stem}.json"))
}

/// Cases whose output carries the client config paths upstream picks per operating system: on
/// macOS they compare against `<stem>.macos.json` next to the Linux golden.
const PLATFORM_CASES: &[&str] = &["clients_list", "resource-clients"];
const MACOS_SUFFIX: &str = ".macos.json";

/// The case stem a golden file belongs to, for either platform's file.
pub fn stem_of_file(name: &str) -> &str {
    name.strip_suffix(MACOS_SUFFIX)
        .or_else(|| name.strip_suffix(".json"))
        .unwrap_or(name)
}

pub fn bless_guard(bless: Option<&str>, ci: Option<&str>) -> Result<bool, String> {
    let on = |v: Option<&str>| v.is_some_and(|v| !v.is_empty() && v != "0" && v != "false");
    if on(bless) && on(ci) {
        return Err(format!("{BLESS_VAR} is rejected when CI is set"));
    }
    Ok(on(bless))
}

fn bless_requested() -> bool {
    bless_guard(
        std::env::var(BLESS_VAR).ok().as_deref(),
        std::env::var("CI").ok().as_deref(),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

/// First place where two JSON values differ, as a pointer with short renderings of both sides.
pub fn first_difference(expected: &Value, actual: &Value) -> Option<String> {
    fn short(v: &Value) -> String {
        let text = v.to_string();
        if text.chars().count() > 120 {
            format!("{}...", text.chars().take(120).collect::<String>())
        } else {
            text
        }
    }
    fn walk(path: &str, a: &Value, b: &Value) -> Option<String> {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                for (key, left) in x {
                    let here = format!("{path}/{key}");
                    match y.get(key) {
                        Some(right) => {
                            if let Some(found) = walk(&here, left, right) {
                                return Some(found);
                            }
                        }
                        None => return Some(format!("{here}: golden has it, output does not")),
                    }
                }
                y.keys()
                    .find(|key| !x.contains_key(*key))
                    .map(|key| format!("{path}/{key}: output has it, golden does not"))
            }
            (Value::Array(x), Value::Array(y)) => {
                for (i, (left, right)) in x.iter().zip(y).enumerate() {
                    if let Some(found) = walk(&format!("{path}/{i}"), left, right) {
                        return Some(found);
                    }
                }
                (x.len() != y.len())
                    .then(|| format!("{path}: golden has {} items, output {}", x.len(), y.len()))
            }
            _ if a == b => None,
            _ => Some(format!(
                "{path}: golden {} vs output {}",
                short(a),
                short(b)
            )),
        }
    }
    walk("", expected, actual)
}

/// Replaces the file in one step, so a test that reads the golden while another blesses it never
/// sees it empty. The temporary file sits next to `dir`, not in it: the tests that list `dir` would
/// otherwise trip over it.
pub fn write_in(dir: &Path, case: &str, value: &Value) {
    let mut text = serde_json::to_string_pretty(value).unwrap();
    text.push('\n');
    toolport_plus::fsx::atomic_write(
        &path_in(dir, case),
        &text,
        toolport_plus::fsx::Mode::KeepOrDefault,
    )
    .unwrap();
}

pub fn read_in(dir: &Path, case: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path_in(dir, case)).ok()?;
    Some(serde_json::from_str(&text).unwrap_or_else(|e| panic!("golden {case} is not JSON: {e}")))
}

pub fn write(case: &str, value: &Value) {
    write_in(&root(), case, value)
}

pub fn read(case: &str) -> Option<Value> {
    read_in(&root(), case)
}

pub fn assert_golden(case: &str, actual: &Value) {
    assert_golden_in(&root(), case, actual)
}

pub fn assert_golden_in(dir: &Path, case: &str, actual: &Value) {
    if bless_requested() {
        write_in(dir, case, actual);
        return;
    }
    let expected = read_in(dir, case).unwrap_or_else(|| {
        panic!(
            "no golden for `{case}` ({}); run with {BLESS_VAR}=1 and review the new file",
            path_in(dir, case).display()
        )
    });
    if let Some(difference) = first_difference(&expected, actual) {
        let shown: String = actual.to_string().chars().take(3000).collect();
        panic!(
            "envelope of `{case}` drifted from {}: {difference}\n\
             actual: {shown}\n\
             a deliberate contract change: rerun with {BLESS_VAR}=1, review the diff, update the TS type",
            path_in(dir, case).display()
        );
    }
}

/// The `commands` golden is a directory, so two branches that add different commands touch
/// different files: `envelope.json` holds the case without the two arrays and `counts` (derived
/// from them), `<stem>.json` is one row of `data.commands` and `tools/<name>.json` one of
/// `data.tools`. The directory has no order; the contract test asserts the order of the output.
const ENVELOPE_FILE: &str = "envelope.json";
const TOOLS_DIR: &str = "tools";

pub fn commands_dir() -> PathBuf {
    root().join("commands")
}

type Rows = Vec<(String, Value)>;

fn split_commands(case: &Value) -> (Value, Rows, Rows) {
    let mut head = case.clone();
    let data = head["envelope"]["data"].as_object_mut().unwrap();
    let rows = |data: &mut serde_json::Map<String, Value>, key: &str, name: &str| -> Rows {
        let rows = data.remove(key).unwrap();
        let rows: Rows = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let id = row[name].as_str().unwrap();
                let stem = if key == "commands" {
                    file_stem(id)
                } else {
                    id.to_string()
                };
                (stem, row.clone())
            })
            .collect();
        let stems: BTreeSet<&String> = rows.iter().map(|(stem, _)| stem).collect();
        assert_eq!(stems.len(), rows.len(), "two {key} share a file name");
        assert!(
            !stems.contains(&ENVELOPE_FILE.trim_end_matches(".json").to_string())
                && !stems.contains(&TOOLS_DIR.to_string()),
            "a {key} row is named like a file of the directory"
        );
        rows
    };
    let commands = rows(data, "commands", "id");
    let tools = rows(data, "tools", "name");
    data.remove("counts");
    (head, commands, tools)
}

fn json_stems(dir: &Path) -> Vec<String> {
    let mut stems: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .filter_map(|entry| {
                    entry
                        .file_name()
                        .to_str()?
                        .strip_suffix(".json")
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();
    stems.sort();
    stems
}

fn read_rows(dir: &Path, skip: &[&str]) -> Vec<Value> {
    json_stems(dir)
        .into_iter()
        .filter(|stem| !skip.contains(&stem.as_str()))
        .map(|stem| {
            let path = dir.join(format!("{stem}.json"));
            let text = std::fs::read_to_string(&path).unwrap();
            serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("golden {} is not JSON: {e}", path.display()))
        })
        .collect()
}

fn by_key(rows: &mut [Value], key: &str) {
    rows.sort_by(|a, b| a[key].as_str().cmp(&b[key].as_str()));
}

fn with_counts(mut case: Value, commands: Vec<Value>, tools: Vec<Value>) -> Value {
    let leaves = commands
        .iter()
        .filter(|row| row["kind"] == "command")
        .count();
    let data = case["envelope"]["data"].as_object_mut().unwrap();
    data.insert(
        "counts".into(),
        json!({
            "rows": commands.len(),
            "commands": leaves,
            "groups": commands.len() - leaves,
            "tools": tools.len(),
        }),
    );
    data.insert("commands".into(), Value::Array(commands));
    data.insert("tools".into(), Value::Array(tools));
    case
}

/// The golden rebuilt from the directory, rows sorted by `id` and `name`.
pub fn read_commands() -> Option<Value> {
    let dir = commands_dir();
    let text = std::fs::read_to_string(dir.join(ENVELOPE_FILE)).ok()?;
    let head: Value = serde_json::from_str(&text).expect("commands/envelope.json is JSON");
    let mut commands = read_rows(&dir, &[ENVELOPE_FILE.trim_end_matches(".json")]);
    let mut tools = read_rows(&dir.join(TOOLS_DIR), &[]);
    by_key(&mut commands, "id");
    by_key(&mut tools, "name");
    Some(with_counts(head, commands, tools))
}

fn write_value(path: &Path, value: &Value) {
    let mut text = serde_json::to_string_pretty(value).unwrap();
    text.push('\n');
    toolport_plus::fsx::atomic_write(path, &text, toolport_plus::fsx::Mode::KeepOrDefault).unwrap();
}

fn write_commands(case: &Value) {
    let (head, commands, tools) = split_commands(case);
    let dir = commands_dir();
    write_value(&dir.join(ENVELOPE_FILE), &head);
    for (stem, row) in &commands {
        write_value(&dir.join(format!("{stem}.json")), row);
    }
    for (stem, row) in &tools {
        write_value(&dir.join(TOOLS_DIR).join(format!("{stem}.json")), row);
    }
    let keep = |rows: &Rows, extra: Option<&str>| -> BTreeSet<String> {
        rows.iter()
            .map(|(stem, _)| stem.clone())
            .chain(extra.map(String::from))
            .collect()
    };
    let drop_stale = |dir: &Path, keep: BTreeSet<String>| {
        for stem in json_stems(dir)
            .into_iter()
            .filter(|stem| !keep.contains(stem))
        {
            std::fs::remove_file(dir.join(format!("{stem}.json"))).unwrap();
        }
    };
    drop_stale(&dir, keep(&commands, Some("envelope")));
    drop_stale(&dir.join(TOOLS_DIR), keep(&tools, None));
}

/// Compares the `commands` case with the directory, ignoring the order of the rows.
pub fn assert_commands_golden(case: &Value) {
    if bless_requested() {
        write_commands(case);
        return;
    }
    let dir = commands_dir();
    let expected = read_commands().unwrap_or_else(|| {
        panic!(
            "no golden for `commands` ({}); run with {BLESS_VAR}=1 and review the new files",
            dir.display()
        )
    });
    let mut actual = case.clone();
    let data = actual["envelope"]["data"].as_object_mut().unwrap();
    for (key, name) in [("commands", "id"), ("tools", "name")] {
        by_key(data.get_mut(key).unwrap().as_array_mut().unwrap(), name);
    }
    if let Some(difference) = first_difference(&expected, &actual) {
        panic!(
            "envelope of `commands` drifted from {}: {difference}\n\
             a deliberate contract change: rerun with {BLESS_VAR}=1, review the diff, update the TS type",
            dir.display()
        );
    }
}

/// Every `.json` file under `dir`, sub-directories included.
pub fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(json_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "json") {
            found.push(path);
        }
    }
    found.sort();
    found
}
