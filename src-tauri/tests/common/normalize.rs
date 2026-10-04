#![allow(dead_code)]

//! What a golden keeps of an output: paths, times and backup stamps that differ per run become
//! placeholders (`<WORLD>`, `<MOCK>`, `<BIN>`, `<TIME>`, `<STAMP>`), and values that change between
//! machines or releases are masked to a value of the same JSON type.

use regex::Regex;
use serde_json::{json, Value};

use crate::ctl_world::CtlWorld;

/// Keys whose value changes between machines or releases; the golden keeps only that they exist.
pub const MASKED_KEYS: &[&str] = &[
    "version",
    "pid",
    "elapsedMs",
    "durationMs",
    "tookMs",
    "head",
    "commitSha",
    "bundleBytes",
    "nextDueAt",
    "lastProbe",
    "since",
    "expiresAt",
];

/// A masked value keeps its JSON type, so the TS shape of the field stays honest.
fn mask(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if MASKED_KEYS.contains(&key.as_str()) && !inner.is_null() {
                    *inner = if inner.is_number() {
                        json!(0)
                    } else {
                        json!("<masked>")
                    };
                } else {
                    mask(inner);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mask),
        _ => {}
    }
}

pub fn world_roots(world: &CtlWorld) -> Vec<String> {
    let mut roots = Vec::new();
    if let Ok(real) = std::fs::canonicalize(&world.base) {
        roots.push(real.to_string_lossy().into_owned());
    }
    roots.push(world.path(&world.base));
    roots.dedup();
    roots
}

fn bin_dirs() -> Vec<String> {
    let dir = std::path::Path::new(env!("CARGO_BIN_EXE_toolportctl"))
        .parent()
        .unwrap();
    let mut dirs = vec![dir.to_string_lossy().into_owned()];
    if let Ok(real) = std::fs::canonicalize(dir) {
        dirs.push(real.to_string_lossy().into_owned());
    }
    dirs
}

pub fn normalize(world: &CtlWorld, envelope: &Value) -> Value {
    let time =
        Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?").unwrap();
    let backup_stamp = Regex::new(r"/\d{13}-").unwrap();
    let stamp = Regex::new(r"\b\d{8}-\d{6}\b").unwrap();
    let mut text = serde_json::to_string(envelope).unwrap();
    for root in world_roots(world) {
        text = text.replace(&root, "<WORLD>");
    }
    text = text.replace(&world.mock, "<MOCK>");
    for dir in bin_dirs() {
        text = text.replace(&dir, "<BIN>");
    }
    let text = time.replace_all(&text, "<TIME>").into_owned();
    let text = stamp.replace_all(&text, "<STAMP>").into_owned();
    let text = backup_stamp.replace_all(&text, "/<STAMP>-").into_owned();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    mask(&mut value);
    value
}
