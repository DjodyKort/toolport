//! Example tasks that ship inside the binary (MIG-AUTO-3). A shipped task is listed, disabled,
//! until the user first changes it: that write copies it into the data dir and records its id, so
//! a later `task rm` is not undone by the list showing it again. Reading never writes.

use super::store;
use std::path::{Path, PathBuf};

pub const SKILLS_AUTO_SYNC: &str = "skills-auto-sync";
pub const UPDATES_CHECK: &str = "updates-check";
pub const SELECTIVE_UPDATE_CHECK: &str = "selective-update-check";
pub const SYNC_PULL: &str = "sync-pull";

const SHIPPED: [(&str, &str); 5] = [
    ("moodle-token", include_str!("moodle-token.json")),
    (SKILLS_AUTO_SYNC, include_str!("skills-auto-sync.json")),
    (UPDATES_CHECK, include_str!("updates-check.json")),
    (
        SELECTIVE_UPDATE_CHECK,
        include_str!("selective-update-check.json"),
    ),
    (SYNC_PULL, include_str!("sync-pull.json")),
];

fn marker(base: &Path) -> PathBuf {
    store::tasks_dir(base).join(".shipped")
}

fn claimed(base: &Path) -> Vec<String> {
    std::fs::read_to_string(marker(base))
        .ok()
        .map(|t| {
            t.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn ids() -> Vec<&'static str> {
    SHIPPED.iter().map(|(id, _)| *id).collect()
}

pub fn unclaimed_text(base: &Path, id: &str) -> Option<&'static str> {
    let (_, text) = SHIPPED.iter().find(|(i, _)| *i == id)?;
    (!claimed(base).iter().any(|c| c == id)).then_some(*text)
}

pub fn unclaimed_ids(base: &Path) -> Vec<&'static str> {
    let taken = claimed(base);
    ids()
        .into_iter()
        .filter(|id| !taken.iter().any(|c| c == id))
        .collect()
}

pub fn claim(base: &Path, id: &str) {
    if !ids().contains(&id) {
        return;
    }
    let mut taken = claimed(base);
    if taken.iter().any(|c| c == id) {
        return;
    }
    taken.push(id.to_string());
    let _ = store::write(&marker(base), &(taken.join("\n") + "\n"));
}
