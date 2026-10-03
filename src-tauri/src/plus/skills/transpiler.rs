//! Transpiler abstraction and registry; concrete per-client transpilers live in later items.

use super::parser::Skill;
use std::path::{Path, PathBuf};

pub const MCPM_BLOCK_START: &str = "<!-- mcpm:start -->";
pub const MCPM_BLOCK_END: &str = "<!-- mcpm:end -->";

/// Client keys that write relative to a project and are skipped in global mode. Mirrors mcpm
/// verbatim, including its `vscode-copilot` spelling that never matches the `vscode` class key.
pub const PROJECT_ONLY_TRANSPILERS: &[&str] = &["vscode-copilot", "zed", "agents-md"];

/// Keys whose output aggregates every skill into one file through `transpile_all`.
pub const APPEND_MODE_TRANSPILERS: &[&str] = &["zed", "agents-md"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranspileResult {
    pub output_path: PathBuf,
    pub content: String,
    pub warnings: Vec<String>,
}

pub trait Transpiler {
    fn client_key(&self) -> &str;

    fn transpile(&self, skill: &Skill, output_root: &Path) -> Result<TranspileResult, String>;

    fn get_output_path(&self, skill: &Skill, output_root: &Path) -> PathBuf;

    fn get_collision_paths(&self, _skill: &Skill, _output_root: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Append-mode transpilers combine all skills into one output.
    fn transpile_all(
        &self,
        _skills: &[Skill],
        _output_root: &Path,
    ) -> Option<Result<TranspileResult, String>> {
        None
    }

    /// Returns the identifiers installed so a later sync can revoke them.
    fn install_hooks(&self, _skill: &Skill, _output_root: &Path) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    fn uninstall_hooks(&self, _output_root: &Path, _ids: &[String]) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
pub struct TranspilerRegistry {
    items: Vec<Box<dyn Transpiler>>,
}

impl TranspilerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A repeated key replaces the earlier registration in place, like a dict assignment.
    pub fn register(&mut self, transpiler: Box<dyn Transpiler>) {
        match self
            .items
            .iter_mut()
            .find(|t| t.client_key() == transpiler.client_key())
        {
            Some(slot) => *slot = transpiler,
            None => self.items.push(transpiler),
        }
    }

    pub fn get(&self, key: &str) -> Option<&dyn Transpiler> {
        self.items
            .iter()
            .find(|t| t.client_key() == key)
            .map(|t| t.as_ref())
    }

    pub fn all(&self) -> impl Iterator<Item = &dyn Transpiler> {
        self.items.iter().map(|t| t.as_ref())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Replaces the managed block when both delimiters exist, otherwise appends it; whitespace
/// around the block is renormalised exactly like mcpm's `inject_managed_block`.
pub fn inject_managed_block(existing: &str, block: &str) -> String {
    let managed = format!("{MCPM_BLOCK_START}\n{block}\n{MCPM_BLOCK_END}");
    if let (Some(start), Some(end)) = (
        existing.find(MCPM_BLOCK_START),
        existing.find(MCPM_BLOCK_END),
    ) {
        let before = existing[..start].trim_end();
        let after = existing[end + MCPM_BLOCK_END.len()..].trim_start();
        let mut parts = vec![before, managed.as_str()];
        if !after.is_empty() {
            parts.push(after);
        }
        return format!("{}\n", parts.join("\n\n"));
    }
    if !existing.trim().is_empty() {
        return format!("{}\n\n{managed}\n", existing.trim_end());
    }
    format!("{managed}\n")
}
