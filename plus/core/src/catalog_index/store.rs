use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::value::{to_raw_value, RawValue};
use toolport_plus_api::CatalogEntryRecord;

use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;

pub(super) const INDEX_FILE: &str = "registry-index.json";
pub(super) const LIVE_FILE: &str = "registry-index-live.json";
pub(super) const STATE_FILE: &str = "registry-index-state.json";
pub(super) const LOCK_FILE: &str = ".registry-index.lock";
const PART_PREFIX: &str = "registry-index.part-";
const FILE_VERSION: u32 = 1;
const LIVE_CAP: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Row {
    pub id: String,
    pub entry: Box<RawValue>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MainFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    pub last_synced_at: Option<String>,
    #[serde(default)]
    pub entries: Vec<Row>,
}

/// What one flush of a running pass leaves behind, so a stopped pass resumes from `next_cursor`
/// without the whole index being rewritten at every flush.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Part {
    version: u32,
    pub full: bool,
    pub started_at: String,
    #[serde(default)]
    pub since: Option<String>,
    pub next_cursor: String,
    pub pages: u32,
    pub entries: Vec<Row>,
    pub retired: Vec<String>,
}

impl Part {
    pub fn new(
        full: bool,
        started_at: String,
        since: Option<String>,
        next_cursor: String,
        pages: u32,
    ) -> Self {
        Self {
            version: FILE_VERSION,
            full,
            started_at,
            since,
            next_cursor,
            pages,
            entries: Vec::new(),
            retired: Vec::new(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct State {
    #[serde(default)]
    pub last_attempt_at: Option<String>,
    #[serde(default)]
    pub error: Option<StateError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateError {
    pub kind: String,
    pub message: String,
}

pub(super) fn plus_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plus")
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), OpError> {
    let text = serde_json::to_string(value)
        .map_err(|error| OpError::failed("write_failed", error.to_string()))?;
    atomic_write(path, &text, Mode::Private).map_err(|error| {
        OpError::failed(
            "write_failed",
            format!("cannot write {}: {error}", path.display()),
        )
    })
}

/// A missing, unreadable, corrupt or foreign-version file is an empty index.
pub(super) fn read_main(dir: &Path) -> MainFile {
    read_json::<MainFile>(&dir.join(INDEX_FILE))
        .filter(|file| file.version == FILE_VERSION)
        .unwrap_or_default()
}

pub(super) fn write_main(
    dir: &Path,
    last_synced_at: &str,
    entries: Vec<Row>,
) -> Result<(), OpError> {
    write_json(
        &dir.join(INDEX_FILE),
        &MainFile {
            version: FILE_VERSION,
            last_synced_at: Some(last_synced_at.to_string()),
            entries,
        },
    )
}

fn part_paths(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(PART_PREFIX) && name.ends_with(".json"))
        })
        .collect();
    paths.sort();
    paths
}

/// The flushes of an unfinished pass in the order they were written. A flush that cannot be
/// read ends the list: the pass resumes from the last one before it.
pub(super) fn read_parts(dir: &Path) -> Vec<Part> {
    let mut parts = Vec::new();
    for path in part_paths(dir) {
        match read_json::<Part>(&path).filter(|part| part.version == FILE_VERSION) {
            Some(part) => parts.push(part),
            None => break,
        }
    }
    parts
}

pub(super) fn part_count(dir: &Path) -> usize {
    part_paths(dir).len()
}

pub(super) fn write_part(dir: &Path, number: usize, part: &Part) -> Result<(), OpError> {
    write_json(&dir.join(format!("{PART_PREFIX}{number:05}.json")), part)
}

pub(super) fn clear_parts(dir: &Path) {
    for path in part_paths(dir) {
        let _ = std::fs::remove_file(path);
    }
}

pub(super) fn read_state(dir: &Path) -> State {
    read_json(&dir.join(STATE_FILE)).unwrap_or_default()
}

pub(super) fn write_state(dir: &Path, state: &State) -> Result<(), OpError> {
    write_json(&dir.join(STATE_FILE), state)
}

pub(super) fn raw(record: &CatalogEntryRecord) -> Option<Box<RawValue>> {
    to_raw_value(record).ok()
}

/// What makes two entries the same server whatever registry id or label they came with.
fn identity(record: &CatalogEntryRecord) -> String {
    let args: Vec<&str> = record
        .extra
        .get("args")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect();
    format!(
        "{}|{}|{}|{}",
        record.transport,
        record.text("command").unwrap_or(""),
        args.join(" "),
        record.text("url").unwrap_or("")
    )
    .to_lowercase()
}

/// Entries a live search returned while the index could not answer. Kept apart from the synced
/// index so remembering a hit never rewrites it; a full pass replaces them.
pub(super) fn remember(dir: &Path, records: &[CatalogEntryRecord]) -> Result<(), OpError> {
    let path = dir.join(LIVE_FILE);
    let mut rows = read_json::<MainFile>(&path)
        .filter(|file| file.version == FILE_VERSION)
        .map(|file| file.entries)
        .unwrap_or_default();
    for record in records {
        let (Some(entry), id) = (raw(record), identity(record)) else {
            continue;
        };
        rows.retain(|row| row.id != id);
        rows.push(Row { id, entry });
    }
    let start = rows.len().saturating_sub(LIVE_CAP);
    let rows = rows.split_off(start);
    write_json(
        &path,
        &MainFile {
            version: FILE_VERSION,
            last_synced_at: None,
            entries: rows,
        },
    )
}

pub(super) fn forget_live(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(LIVE_FILE));
}

#[derive(Deserialize)]
struct Head {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

struct Searchable {
    name: String,
    haystack: String,
    entry: Box<RawValue>,
}

/// The index as a search reads it: each entry stays JSON text, with its lowercased name and
/// searchable text beside it. It is parsed into a record only when it is a hit, so the whole
/// registry costs about the size of its file in memory.
pub(super) struct Loaded {
    pub last_synced_at: Option<String>,
    synced: usize,
    rows: Vec<Searchable>,
}

impl Loaded {
    pub fn synced_count(&self) -> usize {
        self.synced
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<CatalogEntryRecord> {
        let query = query.trim().to_lowercase();
        let terms: Vec<&str> = query.split_whitespace().collect();
        let mut hits: Vec<(i32, &Searchable)> = self
            .rows
            .iter()
            .filter(|row| {
                row.haystack.contains(&query) || terms.iter().all(|t| row.haystack.contains(t))
            })
            .map(|row| (score(&row.name, &query, &terms), row))
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        hits.into_iter()
            .take(limit)
            .filter_map(|(_, row)| serde_json::from_str(row.entry.get()).ok())
            .collect()
    }
}

/// Only picks which hits survive the cut: upstream ranks the merged list again.
fn score(name: &str, query: &str, terms: &[&str]) -> i32 {
    if name == query {
        3
    } else if name.starts_with(query) {
        2
    } else if name.contains(query) || terms.iter().all(|term| name.contains(term)) {
        1
    } else {
        0
    }
}

fn searchable(row: Row) -> Searchable {
    let head: Head = serde_json::from_str(row.entry.get()).unwrap_or(Head {
        name: String::new(),
        description: String::new(),
    });
    let name = head.name.to_lowercase();
    Searchable {
        haystack: format!("{} {name} {}", row.id, head.description.to_lowercase()),
        name,
        entry: row.entry,
    }
}

type Stamp = Option<(SystemTime, u64)>;
type Cache = HashMap<PathBuf, ((Stamp, Stamp), Arc<Loaded>)>;
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn stamp_of(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Parsed once per process and again only when a file changed on disk (another process synced).
pub(super) fn load(dir: &Path) -> Arc<Loaded> {
    let stamp = (
        stamp_of(&dir.join(INDEX_FILE)),
        stamp_of(&dir.join(LIVE_FILE)),
    );
    let cache = CACHE.get_or_init(Mutex::default);
    let cached = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(dir)
        .filter(|(seen, _)| *seen == stamp)
        .map(|(_, loaded)| Arc::clone(loaded));
    if let Some(loaded) = cached {
        return loaded;
    }
    let main = read_main(dir);
    let synced = main.entries.len();
    let live = read_json::<MainFile>(&dir.join(LIVE_FILE))
        .filter(|file| file.version == FILE_VERSION)
        .map(|file| file.entries)
        .unwrap_or_default();
    let loaded = Arc::new(Loaded {
        last_synced_at: main.last_synced_at,
        synced,
        rows: main
            .entries
            .into_iter()
            .chain(live)
            .map(searchable)
            .collect(),
    });
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(dir.to_path_buf(), (stamp, Arc::clone(&loaded)));
    loaded
}
