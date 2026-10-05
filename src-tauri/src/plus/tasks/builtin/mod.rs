//! Example tasks that ship inside the binary (MIG-AUTO-3). A shipped task is listed, disabled,
//! until the user first changes it: that write copies it into the data dir and records its id, so
//! a later `task rm` is not undone by the list showing it again. Reading never writes.

use super::store;
use std::path::PathBuf;

const SHIPPED: [(&str, &str); 1] = [("moodle-token", include_str!("moodle-token.json"))];

fn marker() -> Option<PathBuf> {
    store::tasks_dir().ok().map(|d| d.join(".shipped"))
}

fn claimed() -> Vec<String> {
    marker().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

pub fn ids() -> Vec<&'static str> {
    SHIPPED.iter().map(|(id, _)| *id).collect()
}

pub fn unclaimed_text(id: &str) -> Option<&'static str> {
    let (_, text) = SHIPPED.iter().find(|(i, _)| *i == id)?;
    (!claimed().iter().any(|c| c == id)).then_some(*text)
}

pub fn unclaimed_ids() -> Vec<&'static str> {
    let taken = claimed();
    ids().into_iter().filter(|id| !taken.iter().any(|c| c == id)).collect()
}

pub fn claim(id: &str) {
    if !ids().contains(&id) {
        return;
    }
    let mut taken = claimed();
    if taken.iter().any(|c| c == id) {
        return;
    }
    taken.push(id.to_string());
    if let Some(path) = marker() {
        let _ = store::write(&path, &(taken.join("\n") + "\n"));
    }
}
