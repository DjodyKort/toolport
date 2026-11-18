#![allow(dead_code)]

//! What a golden keeps of an output: paths, times and backup stamps that differ per run become
//! placeholders (`<WORLD>`, `<MOCK>`, `<BIN>`, `<TIME>`, `<STAMP>`; `<PORT>`, `<PKCE>` and `<STATE>` for the loopback port and random parts of
//! a consent URL; `<TOOLS>` for the tool count the doctor reports, which every new tool changes), and values that change between
//! machines or releases are masked to a value of the same JSON type.

use regex::Regex;
use serde_json::{json, Value};

use super::world::World;

/// Keys whose value changes between machines or releases; the golden keeps only that they exist.
pub const MASKED_KEYS: &[&str] = &[
    "version",
    "pid",
    "elapsedMs",
    "durationMs",
    "latencyMs",
    "tookMs",
    "head",
    "commitSha",
    "bundleBytes",
    "nextDueAt",
    "lastProbe",
    "planHash",
    "since",
    "expiresAt",
    "fileSize",
    "totalSize",
    "machine",
    "revision",
    "remoteSeen",
    "hash",
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

/// How Claude Code spells a folder in the name of a directory it keeps per project.
fn slugged(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub fn world_roots(world: &World) -> Vec<String> {
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

pub fn normalize(world: &World, envelope: &Value) -> Value {
    let time =
        Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?").unwrap();
    let backup_stamp = Regex::new(r"/\d{13}-").unwrap();
    let setup_name = Regex::new(r"toolport-setup-\d{4}-\d{2}-\d{2}\.zip").unwrap();
    let stamp = Regex::new(r"\b\d{8}-\d{6}(?:-\d{2})?\b").unwrap();
    let run_id = Regex::new(r"run-\d{10}-[0-9a-f]{8}").unwrap();
    let world_slug = Regex::new(r"_tmp_plus-world-[A-Za-z_-]+-\d+-\d+").unwrap();
    let about_bytes = Regex::new(r"about \d+ bytes").unwrap();
    let plan_hash = Regex::new(r#""plan":"[0-9a-f]{12}""#).unwrap();
    let loopback_port = Regex::new(r"(?i)(127\.0\.0\.1(?::|%3A))\d+").unwrap();
    let pkce_challenge = Regex::new(r"code_challenge=[^&\x22]+").unwrap();
    let oauth_state = Regex::new(r"\bstate=[^&\x22]+").unwrap();
    let tool_count = Regex::new(r"\b\d+ tools, (\d+) resources").unwrap();
    let mut text = serde_json::to_string(envelope).unwrap();
    for root in world_roots(world) {
        let name = std::path::Path::new(&world.mock).file_name().unwrap();
        text = text.replace(&format!("{root}/{}", name.to_string_lossy()), "<MOCK>");
    }
    for root in world_roots(world) {
        text = text.replace(&root, "<WORLD>");
    }
    for root in world_roots(world) {
        text = text.replace(&slugged(&root), "<WORLD-SLUG>");
    }
    text = text.replace(&world.mock, "<MOCK>");
    for dir in bin_dirs() {
        text = text.replace(&dir, "<BIN>");
    }
    let text = setup_name
        .replace_all(&text, "toolport-setup-<DATE>.zip")
        .into_owned();
    let text = time.replace_all(&text, "<TIME>").into_owned();
    let text = stamp.replace_all(&text, "<STAMP>").into_owned();
    let text = run_id.replace_all(&text, "run-<RUN>").into_owned();
    let text = world_slug.replace_all(&text, "<WORLD-SLUG>").into_owned();
    let text = about_bytes
        .replace_all(&text, "about <N> bytes")
        .into_owned();
    let text = plan_hash
        .replace_all(&text, r#""plan":"<PLAN>""#)
        .into_owned();
    let text = backup_stamp.replace_all(&text, "/<STAMP>-").into_owned();
    let text = loopback_port.replace_all(&text, "${1}<PORT>").into_owned();
    let text = pkce_challenge
        .replace_all(&text, "code_challenge=<PKCE>")
        .into_owned();
    let text = oauth_state.replace_all(&text, "state=<STATE>").into_owned();
    let text = tool_count
        .replace_all(&text, "<TOOLS> tools, ${1} resources")
        .into_owned();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    mask(&mut value);
    value
}
