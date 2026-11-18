//! `<data dir>/plus/server-repos.json`: the build command each server's update may run (while its
//! text stays the same) and the one update per server that `server repo undo` can still take back.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::fsx::{atomic_write, Mode};

const FILE: &str = "server-repos.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub command: String,
    pub at: i64,
}

/// What an applied update moved, with what is needed to put it back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    pub root: String,
    pub branch: String,
    pub from: String,
    pub to: String,
    pub at: i64,
    /// The build command the update ran; the undo runs it again on the old files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

/// A branch switch that `server repo switch --undo` can still take back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Switched {
    pub root: String,
    pub from_branch: String,
    pub from_head: String,
    pub to_branch: String,
    pub to_head: String,
    /// The target did not exist as a local branch; the switch made it, and undo removes it again.
    pub created: bool,
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStore {
    #[serde(default)]
    pub builds: BTreeMap<String, Build>,
    #[serde(default)]
    pub applied: BTreeMap<String, Applied>,
    #[serde(default)]
    pub switched: BTreeMap<String, Switched>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn path(base: &Path) -> PathBuf {
    base.join("plus").join(FILE)
}

impl RepoStore {
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

    /// The command this server's update may run without being asked again. It is the text that
    /// was agreed to: a different command is a new yes.
    pub fn remembered(&self, server_id: &str) -> Option<&str> {
        self.builds.get(server_id).map(|b| b.command.as_str())
    }
}
