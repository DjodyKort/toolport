//! Where a library skill came from, for the skills that did not start in the library.
//!
//! `mcpm-skills-origins.json` sits at the repository root, not inside a skill folder, so the
//! sync never installs it. A skill without an entry is the library's own.

use super::json::{self, J};
use std::collections::BTreeMap;
use std::path::Path;

pub const ORIGINS_FILE: &str = "mcpm-skills-origins.json";
pub const TAP_PREFIX: &str = "tap:";

pub fn read_origins(repo: &Path) -> BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(repo.join(ORIGINS_FILE)) else {
        return BTreeMap::new();
    };
    let Ok(J::Obj(items)) = json::parse(&text) else {
        return BTreeMap::new();
    };
    items
        .into_iter()
        .filter_map(|(name, v)| Some((name, v.as_str()?.to_string())))
        .collect()
}

pub fn record_origin(repo: &Path, skill: &str, source: &str) -> Result<(), String> {
    let mut origins = read_origins(repo);
    origins.insert(skill.to_string(), source.to_string());
    let doc = J::Obj(
        origins
            .iter()
            .map(|(k, v)| (k.clone(), J::str(v)))
            .collect(),
    );
    crate::fsx::atomic_write(&repo.join(ORIGINS_FILE), &doc.dumps())
}

pub fn is_tap(source: &str) -> bool {
    source.starts_with(TAP_PREFIX)
}
