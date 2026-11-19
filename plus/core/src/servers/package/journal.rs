//! `<data dir>/plus/server-updates.json`: the one package update per server that
//! `server update --undo` can still take back, with what is needed to put it back. A replaced
//! binary is kept next to it in `<data dir>/plus/server-updates/<server id>/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use toolport_plus_api::McpmSource;

use crate::fsx::{atomic_write, Mode};

const FILE: &str = "server-updates.json";
const KEPT_DIR: &str = "server-updates";

/// One argument of the server that the update rewrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgChange {
    pub index: usize,
    pub before: String,
    pub after: String,
}

/// A binary the update replaced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    /// The binary that was there, kept for the undo.
    pub backup: String,
    /// The digest of the binary the update installed: the undo goes back only while it is still
    /// the one in place.
    pub installed_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Applied {
    /// `npm`, `pypi`, `docker` or `github`.
    pub kind: String,
    pub package: String,
    pub from: String,
    pub to: String,
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arg: Option<ArgChange>,
    /// `mcpmSource` as it was before the update, when the update changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_before: Option<McpmSource>,
    /// What the update wrote into the source (the image tag or the release version); the undo
    /// goes back only while the source still says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FileChange>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    #[serde(default)]
    pub applied: BTreeMap<String, Applied>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn path(base: &Path) -> PathBuf {
    base.join("plus").join(FILE)
}

/// Where the binary an update replaced is kept.
pub fn kept_dir(base: &Path, server_id: &str) -> PathBuf {
    let safe: String = server_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    base.join("plus").join(KEPT_DIR).join(safe)
}

impl Journal {
    pub fn read(base: &Path) -> Result<Self, String> {
        let file = path(base);
        match std::fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("{} is not valid: {e}", file.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", file.display())),
        }
    }

    pub fn write(&self, base: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).unwrap_or_default() + "\n";
        atomic_write(&path(base), &text, Mode::Private)
            .map_err(|e| format!("{}: {e}", path(base).display()))
    }
}
