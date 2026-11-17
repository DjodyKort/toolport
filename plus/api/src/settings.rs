//! Claude Code settings files, reached through upstream's own writer so comments, formatting
//! and every key other than `hooks` survive a write.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub trait SettingsStore {
    /// The settings files of the Claude Code profiles that exist, best known first.
    fn claude_settings_paths(&self) -> Vec<PathBuf>;

    /// The parsed file (an empty object when it does not exist) and its text when it does.
    fn read_settings(&self, path: &Path) -> Result<(Value, Option<String>), String>;

    /// Writes `root["hooks"]` back after a backup of what was there; nothing else changes.
    /// `original` is the text `read_settings` returned.
    fn write_hooks(&self, path: &Path, original: Option<&str>, root: &Value) -> Result<(), String>;
}
