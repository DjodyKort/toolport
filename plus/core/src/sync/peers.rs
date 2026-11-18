//! What the other machines published about their server checkouts, as the last pull read it.
//! Kept next to the sync state; the attention rows and `sync repos` compare it with this
//! machine's checkouts as they are now.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fsx::{atomic_write, Mode};
use crate::setup::inspect::Opened;
use crate::setup::repo_heads::{MachineHeads, DIR};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peers {
    /// The remote revision the records were read from.
    #[serde(default)]
    pub revision: Option<String>,
    /// When this machine read them, in epoch seconds.
    #[serde(default, rename = "seenAt")]
    pub seen_at: i64,
    #[serde(default)]
    pub machines: Vec<MachineHeads>,
}

fn path(base: &Path) -> PathBuf {
    base.join("plus").join("sync").join("peers.json")
}

impl Peers {
    pub fn read(base: &Path) -> Self {
        std::fs::read_to_string(path(base))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, base: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).unwrap_or_default() + "\n";
        atomic_write(&path(base), &text, Mode::Private)
            .map_err(|e| format!("{}: {e}", path(base).display()))
    }
}

/// The records of every machine but `own` that a sync file carries, as written.
pub fn records_of(opened: &mut Opened, own: &str) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    for name in opened.names_under(DIR) {
        let Ok(value) = opened.read_json(&name) else {
            continue;
        };
        let Some(machine) = value["machine"].as_str().filter(|m| *m != own) else {
            continue;
        };
        found.push((machine.to_string(), value));
    }
    found
}

pub fn typed(records: &[(String, Value)]) -> Vec<MachineHeads> {
    records
        .iter()
        .filter_map(|(_, value)| serde_json::from_value(value.clone()).ok())
        .collect()
}
