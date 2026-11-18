//! Memory and history per repository: auto memory, instruction files and sessions (D-144, D-145).
//! Every operation takes explicit roots and a data directory, so tests run in temp dirs and
//! nothing reads the real `~/.claude`.

pub mod claude_json;
pub mod clear;
pub mod instructions;
pub mod inventory;
pub mod ops;
pub mod overview;
pub mod resolve;
pub mod session_ops;
pub mod sessions;
pub mod store;
pub mod switch;

#[cfg(test)]
mod tests;

use std::path::Path;

/// What every operation reads from the machine, explicit so tests supply their own.
pub struct World<'a> {
    pub home: &'a Path,
    /// Toolport's data folder: the ledger, the trash and the archive.
    pub data_dir: &'a Path,
    pub now: i64,
    pub count_tokens: &'a dyn Fn(&str) -> u64,
    pub managed_settings: Option<&'a Path>,
    pub policy_claude_md: Option<&'a Path>,
    /// `CLAUDE_CODE_DISABLE_AUTO_MEMORY` as this process sees it; a terminal Toolport did not
    /// start is invisible to it.
    pub env_disable: Option<String>,
}

impl World<'_> {
    pub fn layers(&self, repo: &resolve::Repo) -> resolve::Layers {
        resolve::Layers::load(
            self.home,
            repo,
            self.managed_settings,
            self.env_disable.clone(),
        )
    }
}
