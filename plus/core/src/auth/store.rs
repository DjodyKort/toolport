//! The 0600 status file and edge log under `<data dir>/auth`. The layout is the one the 1.x fork
//! wrote, so an existing file stays readable.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::probe::ProbeRegistry;
use super::types::Tracked;

pub const STATUS_VERSION: u32 = 1;
pub const EVENTS_MAX_BYTES: u64 = 128 * 1024;
pub const EVENTS_KEEP_LINES: usize = 500;

/// Kept so a status file written for an API-token probe still parses; every probe here is OAuth.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeHintKind {
    #[default]
    OAuth,
    ApiToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    pub tracked: Tracked,
    pub last_probe_at: Option<i64>,
    pub next_due_at: i64,
    #[serde(default)]
    pub hint_kind: ProbeHintKind,
    #[serde(default)]
    pub token_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusFile {
    pub version: u32,
    #[serde(default)]
    pub servers: BTreeMap<String, ServerEntry>,
    #[serde(default)]
    pub profiles: BTreeMap<String, i64>,
}

impl Default for StatusFile {
    fn default() -> Self {
        StatusFile {
            version: STATUS_VERSION,
            servers: BTreeMap::new(),
            profiles: BTreeMap::new(),
        }
    }
}

impl StatusFile {
    /// Drops what no probe of `registry` can refresh any more, so a server removed from the
    /// registry cannot keep reporting its last state. Returns whether anything was dropped.
    pub fn prune(&mut self, registry: &ProbeRegistry) -> bool {
        let gates: BTreeSet<String> = registry
            .iter()
            .filter_map(|spec| spec.profile_gate_key())
            .collect();
        let before = (self.servers.len(), self.profiles.len());
        self.servers.retain(|id, _| registry.get(id).is_some());
        self.profiles.retain(|key, _| gates.contains(key));
        before != (self.servers.len(), self.profiles.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeEvent {
    pub ts: i64,
    pub server: String,
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct AuthStore {
    dir: PathBuf,
}

pub struct Locked<'a> {
    store: &'a AuthStore,
    _lock: File,
}

fn private_options() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

impl AuthStore {
    pub fn new(dir: &Path) -> Self {
        AuthStore {
            dir: dir.to_path_buf(),
        }
    }

    pub fn status_path(&self) -> PathBuf {
        self.dir.join("status.json")
    }

    fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }

    fn corrupt_path(&self) -> PathBuf {
        self.dir.join("status.json.corrupt")
    }

    /// Exclusive across processes until the guard drops.
    pub fn lock(&self) -> Result<Locked<'_>, String> {
        std::fs::create_dir_all(&self.dir).map_err(|error| error.to_string())?;
        let lock = private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join("auth.lock"))
            .map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: the descriptor belongs to `lock`, which outlives the call.
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(Locked {
            store: self,
            _lock: lock,
        })
    }

    /// Lock-free and write-free: the status file is replaced atomically, so an inspection never
    /// creates the directory or quarantines a bad file.
    pub fn read_status(&self) -> StatusFile {
        std::fs::read_to_string(self.status_path())
            .ok()
            .and_then(|text| serde_json::from_str::<StatusFile>(&text).ok())
            .filter(|status| status.version == STATUS_VERSION)
            .unwrap_or_default()
    }
}

impl Locked<'_> {
    pub fn load_status(&self) -> StatusFile {
        let path = self.store.status_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return StatusFile::default()
            }
            Err(_) => {
                self.quarantine();
                return StatusFile::default();
            }
        };
        match serde_json::from_str::<StatusFile>(&text) {
            Ok(status) if status.version == STATUS_VERSION => status,
            _ => {
                self.quarantine();
                StatusFile::default()
            }
        }
    }

    fn quarantine(&self) {
        let _ = std::fs::rename(self.store.status_path(), self.store.corrupt_path());
    }

    pub fn save_status(&self, status: &StatusFile) -> Result<(), String> {
        let text = serde_json::to_string_pretty(status).map_err(|error| error.to_string())?;
        crate::fsx::write_private(&self.store.status_path(), &text)
    }

    /// Appends the events and, past the byte cap, keeps the newest lines only.
    pub fn append_events(&self, events: &[EdgeEvent]) -> Result<(), String> {
        if events.is_empty() {
            return Ok(());
        }
        let path = self.store.events_path();
        let mut lines: Vec<String> = File::open(&path)
            .map(|file| BufReader::new(file).lines().map_while(Result::ok).collect())
            .unwrap_or_default();
        for event in events {
            lines.push(serde_json::to_string(event).map_err(|error| error.to_string())?);
        }
        let bytes: u64 = lines.iter().map(|line| line.len() as u64 + 1).sum();
        if bytes > EVENTS_MAX_BYTES && lines.len() > EVENTS_KEEP_LINES {
            lines.drain(..lines.len() - EVENTS_KEEP_LINES);
        }
        crate::fsx::write_private(&path, &(lines.join("\n") + "\n"))
    }
}
