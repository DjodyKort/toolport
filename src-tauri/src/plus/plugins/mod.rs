//! Claude Code plugins and hooks as Toolport sees them (D-074, D-075): one reader that asks
//! `claude plugin ...` when the binary is there and reads the files otherwise, the effective
//! `enabledPlugins` of a folder, the `plugins ls|show` rows and the `hooks ls` inventory.
//!
//! Nothing in here starts a hook or a plugin process; the only child process is the `claude`
//! binary, and only with the fixed `plugin list|details|configure|marketplace update` arguments.

pub mod adapters;
pub mod api;
pub mod claude;
pub mod cli;
pub mod config;
pub mod hooks;
pub mod installed;
pub mod manifest;
pub mod report;
pub mod settings;

pub use claude::{ClaudeRunner, SystemClaude};
pub use installed::PluginStatus;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod config_tests;

use std::path::{Path, PathBuf};

pub const ENV_MANAGED_SETTINGS: &str = "TOOLPORT_CLAUDE_MANAGED_SETTINGS";

/// The folders the readers look at. `host` is this machine; tests build one over a temp tree.
#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    pub claude_home: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub managed_settings: Option<PathBuf>,
    pub profiles_root: PathBuf,
}

impl Env {
    pub fn host() -> Option<Self> {
        let (roots, _) = crate::plus::sources::host_world()?;
        let managed_settings = match std::env::var_os(ENV_MANAGED_SETTINGS) {
            Some(path) if path.is_empty() => None,
            Some(path) => Some(PathBuf::from(path)),
            None => Some(crate::plus::context::compact::managed_settings_path()),
        };
        Some(Self {
            profiles_root: roots.profiles_root(),
            home: roots.home,
            claude_home: roots.claude_home,
            data_dir: crate::registry::conduit_dir(),
            managed_settings,
        })
    }

    pub fn layers(&self, cwd: Option<&Path>) -> settings::Layers {
        settings::Layers::load(
            &self.claude_home,
            cwd,
            self.managed_settings.as_deref(),
            Some(&self.home),
        )
    }
}

/// `--cwd` as an absolute path; `None` when the flag was not given.
pub fn absolute_cwd(raw: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(raw);
    let abs = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot resolve --cwd: {e}"))?
            .join(path)
    };
    Ok(Some(crate::plus::sources::fsx::canonical(&abs)))
}
