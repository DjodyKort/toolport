//! The two facts Toolport takes from Claude Code's `.claude.json`: the folders Claude Code has run in and
//! whether a project approved external `@imports`. Nothing else in that file is read or kept.

use crate::context::fsx;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CAP: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub struct Known {
    pub folders: Vec<PathBuf>,
    approvals: BTreeMap<String, bool>,
}

impl Known {
    pub fn read(file: &Path) -> Self {
        let Some(text) = fsx::read_text(file, CAP) else {
            return Self::default();
        };
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(&text) else {
            return Self::default();
        };
        let mut known = Self::default();
        for (path, project) in root
            .get("projects")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if !Path::new(path).is_absolute() {
                continue;
            }
            known.folders.push(PathBuf::from(path));
            if let Some(approved) = project
                .get("hasClaudeMdExternalIncludesApproved")
                .and_then(Value::as_bool)
            {
                known.approvals.insert(path.clone(), approved);
            }
        }
        known.folders.sort();
        known
    }

    /// What the project at `root` answered to the approval of external imports, when it did.
    pub fn external_imports_approved(&self, root: &Path) -> Option<bool> {
        self.approvals.get(&fsx::display(root)).copied()
    }

    /// The recorded folders at or under any of `roots`.
    pub fn under(&self, roots: &[PathBuf]) -> Vec<PathBuf> {
        self.folders
            .iter()
            .filter(|f| roots.iter().any(|r| f.starts_with(r)))
            .cloned()
            .collect()
    }
}
