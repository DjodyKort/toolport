//! The files a person chose to keep as they are although they differ from the library copy.
//! A sync that finds such a file with the content it had when it was kept stops holding it back;
//! a file edited again is held again. The record is `<data dir>/plus/skills-kept.json`.

use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FILE: &str = "skills-kept.json";

/// `<client>:<skill>/<path in the skill folder>` to the content hash the file had when kept.
pub type Kept = BTreeMap<String, String>;

pub fn key(client: &str, skill: &str, path: &str) -> String {
    format!("{client}:{skill}/{path}")
}

fn path(base: &Path) -> PathBuf {
    base.join("plus").join(FILE)
}

/// A record that is missing or cannot be read keeps nothing, so every file stays held.
pub fn load(base: &Path) -> Kept {
    std::fs::read_to_string(path(base))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(base: &Path, kept: &Kept) -> Result<(), OpError> {
    let text = serde_json::to_string_pretty(kept).unwrap_or_default() + "\n";
    atomic_write(&path(base), &text, Mode::Private)
        .map_err(|e| OpError::failed("skills", format!("{}: {e}", path(base).display())))
}
