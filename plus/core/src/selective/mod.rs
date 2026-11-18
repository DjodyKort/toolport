//! Selective loading (D-146): pick single skills, agents, commands, hooks and rules of a big tap
//! or plugin and keep that choice across updates.
//!
//! The choice lives in a selection store of Toolport's own. What Claude Code loads is a generated
//! slice plugin in the local marketplace `toolport-selections` ([`slice`]), enabled through
//! `SettingsStore::write_key` only, and only after a plan the caller names by its hash. An update
//! never changes what is on: [`ops::plan`] compares the pinned content with the latest version,
//! item by item, and applying that plan is the only way the pin moves.

pub mod check;
pub mod discover;
pub mod handlers;
pub mod lines;
pub mod ops;
pub mod plan;
pub mod policy;
pub mod slice;
pub mod store;

#[cfg(all(test, unix))]
mod tests;
#[cfg(all(test, unix))]
mod tests_history;

pub use ops::Env;

use std::path::{Path, PathBuf};

/// Where the selection store, the generated marketplace and the undo journals live.
pub fn base_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("selective")
}
