#![allow(dead_code)]

//! Golden `--json` envelopes under `tests/fixtures/ctl-envelopes/`. A golden is compared as JSON
//! (not text), so a formatter run over the file does not matter. `CTL_ENVELOPE_BLESS=1` rewrites
//! them and is rejected when `CI` is set: a changed envelope is a reviewed change to the contract.

use serde_json::Value;
use std::path::{Path, PathBuf};

pub const BLESS_VAR: &str = "CTL_ENVELOPE_BLESS";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ctl-envelopes")
}

/// `skills tap add` is stored as `skills-tap-add.json`.
pub fn file_stem(case: &str) -> String {
    case.split_whitespace().collect::<Vec<_>>().join("-")
}

pub fn path_of(case: &str) -> PathBuf {
    root().join(format!("{}.json", file_stem(case)))
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

pub fn write(case: &str, value: &Value) {
    let path = path_of(case);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut text = serde_json::to_string_pretty(value).unwrap();
    text.push('\n');
    std::fs::write(&path, text).unwrap();
}

pub fn read(case: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path_of(case)).ok()?;
    Some(serde_json::from_str(&text).unwrap_or_else(|e| panic!("golden {case} is not JSON: {e}")))
}

pub fn assert_golden(case: &str, actual: &Value) {
    if bless_requested() {
        write(case, actual);
        return;
    }
    let expected = read(case).unwrap_or_else(|| {
        panic!(
            "no golden for `{case}` ({}); run with {BLESS_VAR}=1 and review the new file",
            path_of(case).display()
        )
    });
    if let Some(difference) = first_difference(&expected, actual) {
        panic!(
            "envelope of `{case}` drifted from {}: {difference}\n\
             a deliberate contract change: rerun with {BLESS_VAR}=1, review the diff, update the TS type",
            path_of(case).display()
        );
    }
}
