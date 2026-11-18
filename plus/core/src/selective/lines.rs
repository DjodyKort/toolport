//! How much of an item's text differs between the pinned copy and the latest version, for the
//! summary beside a Compare button. Lines are counted as a multiset, so a moved line costs
//! nothing and an edited line counts once as removed and once as added.

use crate::context::fsx::{self, Kind as FileKind};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

const MAX_FILES: usize = 500;
const MAX_FILE: usize = 1024 * 1024;
const NESTED: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Lines {
    pub added: usize,
    pub removed: usize,
    pub files: usize,
}

fn texts(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    collect(root, "", 0, &mut out);
    out
}

fn collect(dir: &Path, prefix: &str, depth: usize, out: &mut BTreeMap<String, String>) {
    if fsx::is_file(dir) {
        if let Some(text) = fsx::read_text(dir, MAX_FILE) {
            out.insert(String::new(), text);
        }
        return;
    }
    if depth > NESTED {
        return;
    }
    for entry in fsx::list_dir(dir) {
        if entry.symlink || out.len() >= MAX_FILES {
            continue;
        }
        let name = format!("{prefix}{}", entry.name);
        match entry.kind {
            FileKind::Dir => collect(&entry.path, &format!("{name}/"), depth + 1, out),
            FileKind::File => {
                if let Some(text) = fsx::read_text(&entry.path, MAX_FILE) {
                    out.insert(name, text);
                }
            }
            FileKind::Other => {}
        }
    }
}

fn counts(text: &str) -> BTreeMap<&str, usize> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        *map.entry(line).or_insert(0) += 1;
    }
    map
}

fn surplus(of: &BTreeMap<&str, usize>, against: &BTreeMap<&str, usize>) -> usize {
    of.iter()
        .map(|(line, n)| n.saturating_sub(against.get(line).copied().unwrap_or(0)))
        .sum()
}

pub fn between(old: &Path, new: &Path) -> Lines {
    let (before, after) = (texts(old), texts(new));
    let mut lines = Lines::default();
    for name in before
        .keys()
        .chain(after.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (a, b) = (
            before.get(name).map(String::as_str).unwrap_or(""),
            after.get(name).map(String::as_str).unwrap_or(""),
        );
        if a == b {
            continue;
        }
        let (ca, cb) = (counts(a), counts(b));
        lines.removed += surplus(&ca, &cb);
        lines.added += surplus(&cb, &ca);
        lines.files += 1;
    }
    lines
}
