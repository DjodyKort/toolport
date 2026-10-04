use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use super::types::Tracked;

pub const STATUS_VERSION: u32 = 1;
pub const EVENTS_MAX_BYTES: u64 = 128 * 1024;
pub const EVENTS_KEEP_LINES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    pub tracked: Tracked,
    pub last_probe_at: Option<i64>,
    pub next_due_at: i64,
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
    pub fn tracked_map(&self) -> BTreeMap<String, Tracked> {
        self.servers
            .iter()
            .map(|(id, entry)| (id.clone(), entry.tracked.clone()))
            .collect()
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
    events_max_bytes: u64,
    events_keep_lines: usize,
}

pub struct Locked<'a> {
    store: &'a AuthStore,
    _lock: File,
}

impl AuthStore {
    pub fn new(dir: &Path) -> Self {
        AuthStore {
            dir: dir.to_path_buf(),
            events_max_bytes: EVENTS_MAX_BYTES,
            events_keep_lines: EVENTS_KEEP_LINES,
        }
    }

    #[cfg(test)]
    pub fn with_event_caps(mut self, max_bytes: u64, keep_lines: usize) -> Self {
        self.events_max_bytes = max_bytes;
        self.events_keep_lines = keep_lines;
        self
    }

    pub fn status_path(&self) -> PathBuf {
        self.dir.join("status.json")
    }

    pub fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }

    pub fn notified_path(&self) -> PathBuf {
        self.dir.join("notified.json")
    }

    pub fn corrupt_path(&self) -> PathBuf {
        self.dir.join("status.json.corrupt")
    }

    pub fn lock(&self) -> Result<Locked<'_>, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let lock = crate::plus::obs::store::open_private(&self.dir.join("auth.lock"))
            .map_err(|e| e.to_string())?;
        lock.lock_exclusive().map_err(|e| e.to_string())?;
        Ok(Locked {
            store: self,
            _lock: lock,
        })
    }
}

impl Locked<'_> {
    pub fn load_status(&self) -> StatusFile {
        let path = self.store.status_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return StatusFile::default(),
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
        let text = serde_json::to_string_pretty(status).map_err(|e| e.to_string())?;
        crate::registry::atomic_write(&self.store.status_path(), &text)
    }

    pub fn append_events(&self, events: &[EdgeEvent]) -> Result<(), String> {
        if events.is_empty() {
            return Ok(());
        }
        let mut buf = String::new();
        for event in events {
            buf.push_str(&serde_json::to_string(event).map_err(|e| e.to_string())?);
            buf.push('\n');
        }
        crate::registry::append_line_locked(
            &self.store.events_path(),
            &buf,
            self.store.events_max_bytes,
            self.store.events_keep_lines,
            None,
        )
    }

    pub fn read_events(&self) -> Vec<EdgeEvent> {
        let Ok(file) = File::open(self.store.events_path()) else {
            return Vec::new();
        };
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str::<EdgeEvent>(&line).ok())
            .collect()
    }
}
