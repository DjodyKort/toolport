//! `<data dir>/plus/profile-ledger.json`: per folder, the bundle that was applied and every key
//! Toolport owns there, with the value it replaced and the value it wrote. Undo and status read
//! only this. It holds key names and scalar values, never the files' other content.

use super::bundle_io;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owned {
    pub path: Vec<String>,
    /// `set`: a key of an object; `entry`: an item of a list.
    pub kind: String,
    #[serde(default)]
    pub existed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    pub written: String,
}

impl Owned {
    pub fn label(&self) -> String {
        self.path.join(".")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsRec {
    pub file: String,
    pub created_file: bool,
    pub created_dir: bool,
    pub created_containers: Vec<String>,
    pub owned: Vec<Owned>,
    pub sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRec {
    pub file: String,
    pub created_file: bool,
    pub prefix: String,
    pub layers: Vec<String>,
    pub lines: usize,
    pub sha256: String,
    pub file_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludeRec {
    pub file: String,
    pub line: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub created_file: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRec {
    pub bundle: String,
    pub applied_at: String,
    pub settings: SettingsRec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockRec>,
    #[serde(default)]
    pub excludes: Vec<ExcludeRec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub version: u32,
    pub folders: BTreeMap<String, FolderRec>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            version: 1,
            folders: BTreeMap::new(),
        }
    }
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("profile-ledger.json")
}

pub fn load(data_dir: &Path) -> Ledger {
    bundle_io::read_exact(&path(data_dir))
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Read-modify-write that re-reads the file right before writing.
pub fn update(data_dir: &Path, mut change: impl FnMut(&mut Ledger)) -> Result<(), String> {
    bundle_io::update(&path(data_dir), 5, |text| {
        let mut ledger = text
            .and_then(|t| serde_json::from_str::<Ledger>(t).ok())
            .unwrap_or_default();
        change(&mut ledger);
        let text = serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())? + "\n";
        Ok((bundle_io::Action::Write(text), ()))
    })
}
