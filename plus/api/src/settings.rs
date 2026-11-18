//! Claude Code settings files, reached through upstream's own writer so comments, formatting
//! and every key other than the one written survive a write.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub trait SettingsStore {
    /// The settings files of the Claude Code profiles that exist, best known first.
    fn claude_settings_paths(&self) -> Vec<PathBuf>;

    /// The parsed file (an empty object when it does not exist) and its text when it does.
    fn read_settings(&self, path: &Path) -> Result<(Value, Option<String>), String>;

    /// Writes `root["hooks"]` back after a backup of what was there; nothing else changes.
    /// `original` is the text `read_settings` returned.
    fn write_hooks(&self, path: &Path, original: Option<&str>, root: &Value) -> Result<(), String> {
        self.write_key(path, original, root, "hooks")
    }

    /// [`write_hooks`](Self::write_hooks) for another top-level key (`enabledPlugins`,
    /// `extraKnownMarketplaces`): `root[key]` is written back, or the key removed when `root`
    /// has none. A change to that key since `original` was read is refused.
    fn write_key(
        &self,
        path: &Path,
        original: Option<&str>,
        root: &Value,
        key: &str,
    ) -> Result<(), String>;
}
