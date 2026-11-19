//! Claude Code plugins as Toolport sees them: which are installed (`claude plugin list` when the
//! binary answers, the files otherwise), the marketplace inside a tap clone, which plugins are
//! switched on, and the link between a tap and Claude Code (`skills tap link|unlink|update`).
//!
//! Settings are written only through `SettingsStore::write_key`, and only the `enabledPlugins`
//! and `extraKnownMarketplaces` keys. The only child process is the `claude` binary, with the
//! fixed `plugin list`, `plugin update`, `plugin validate` and `plugin marketplace remove`
//! arguments.

pub mod claude;
pub mod installed;
pub mod inventory;
pub mod link;
pub mod marketplace;
pub mod settings;

pub use claude::{ClaudeRunner, SystemClaude};

#[cfg(test)]
mod inventory_tests;
#[cfg(test)]
mod tests;

use crate::context::fsx;
use serde_json::Value;
use std::path::{Path, PathBuf};

const JSON_CAP: usize = 4 * 1024 * 1024;

/// The folders the readers look at. `host` is this machine; tests build one over a temp tree.
#[derive(Clone, Debug)]
pub struct Env {
    pub claude_home: PathBuf,
    pub data_dir: PathBuf,
}

impl Env {
    pub fn host() -> Option<Self> {
        let (_, places, _) = crate::sources::host_world()?;
        Some(Self {
            claude_home: places.claude_home,
            data_dir: crate::fsx::data_dir()?,
        })
    }

    pub fn user_settings(&self) -> PathBuf {
        self.claude_home.join("settings.json")
    }
}

/// A JSON file, `None` when it is missing, too big or not JSON.
pub(crate) fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fsx::read_text(path, JSON_CAP)?).ok()
}
