//! Local index of the MCP Registry (D-118) in `<data dir>/plus/registry-index.json`. The file is
//! written after every page, so a partial index is searchable and a stopped sync resumes from
//! `resumeCursor`. `lastSyncedAt` is when the newest finished pass started: the `updated_since`
//! of the next incremental pass.

use crate::catalog::{self, CatalogEntry, RegistryError, RegistryPage};
use crate::plus::op::OpError;
use crate::plus::tasks::cron;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime};

const INDEX_FILE: &str = "registry-index.json";
const INDEX_FILE_VERSION: u32 = 1;
/// A live `search=` call is only worth its 24 to 60 s once the index is older than this.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const SYNC_EVERY: Duration = Duration::from_secs(12 * 60 * 60);
/// Between two background attempts in one process, so a failing registry is not hammered.
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);
/// Matches returned per query; a one-letter query would otherwise match the whole index.
const SEARCH_LIMIT: usize = 100;

#[derive(Clone, Copy)]
pub(crate) struct Tuning {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub retry_pause: Duration,
    pub max_pages: u32,
}

const TUNING: Tuning = Tuning {
    connect_timeout: Duration::from_secs(5),
    read_timeout: Duration::from_secs(20),
    retry_pause: Duration::from_secs(1),
    max_pages: 2_000,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryIndex {
    #[serde(default)]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) last_synced_at: Option<String>,
    #[serde(default)]
    pub(crate) resume_cursor: Option<String>,
    #[serde(default)]
    pub(crate) pass_started_at: Option<String>,
    #[serde(default)]
    pub(crate) complete: bool,
    /// Keyed by the lowercased entry name, the key the search results are de-duplicated by.
    #[serde(default)]
    pub(crate) entries: BTreeMap<String, CatalogEntry>,
}

fn key_of(name: &str) -> String {
    name.to_lowercase()
}

impl RegistryIndex {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn search(&self, query: &str) -> Vec<CatalogEntry> {
        let matches = catalog::catalog_matcher(query);
        let mut hits: Vec<CatalogEntry> = self
            .entries
            .values()
            .filter(|entry| matches(entry))
            .cloned()
            .collect();
        catalog::rank_search_results(&mut hits, query);
        hits.truncate(SEARCH_LIMIT);
        hits
    }

    fn age_secs(&self, now: i64) -> Option<i64> {
        let synced = cron::parse_rfc3339(self.last_synced_at.as_deref()?)?;
        Some(now - synced)
    }

    /// A timestamp in the future (clock set back) counts as stale.
    fn synced_within(&self, limit: Duration, now: i64) -> bool {
        self.age_secs(now)
            .is_some_and(|age| (0..limit.as_secs() as i64).contains(&age))
    }

    pub fn needs_live(&self) -> bool {
        self.needs_live_at(cron::now())
    }

    fn needs_live_at(&self, now: i64) -> bool {
        self.is_empty() || !self.synced_within(STALE_AFTER, now)
    }

    fn due(&self, now: i64) -> bool {
        !self.complete
            || self.resume_cursor.is_some()
            || !self.synced_within(SYNC_EVERY, now)
    }
}

struct Slot {
    index: Arc<RegistryIndex>,
    modified: Option<SystemTime>,
    generation: u64,
}

static SLOTS: OnceLock<Mutex<HashMap<PathBuf, Slot>>> = OnceLock::new();
static WRITER: Mutex<()> = Mutex::new(());
static RUNNING: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
static ATTEMPTS: OnceLock<Mutex<HashMap<PathBuf, i64>>> = OnceLock::new();

fn lock<T>(cell: &'static OnceLock<Mutex<T>>) -> MutexGuard<'static, T>
where
    T: Default,
{
    cell.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn default_path() -> Option<PathBuf> {
    crate::registry::conduit_dir().map(|dir| dir.join("plus").join(INDEX_FILE))
}

fn modified_at(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A missing, unreadable, corrupt or foreign-version file is an empty index.
fn read_index(path: &Path) -> RegistryIndex {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<RegistryIndex>(&raw).ok())
        .filter(|index| index.version == INDEX_FILE_VERSION)
        .unwrap_or_default()
}

/// The index of `path`, parsed once per process and again only when the file changed on disk
/// (another process synced). Readers never wait on a sync's disk write or its network calls.
pub(crate) fn load(path: &Path) -> Arc<RegistryIndex> {
    let modified = modified_at(path);
    let seen_generation = {
        let slots = lock(&SLOTS);
        match slots.get(path) {
            Some(slot) if slot.modified == modified => return Arc::clone(&slot.index),
            Some(slot) => slot.generation,
            None => 0,
        }
    };
    let index = Arc::new(read_index(path));
    let mut slots = lock(&SLOTS);
    if let Some(slot) = slots.get(path).filter(|s| s.generation != seen_generation) {
        return Arc::clone(&slot.index);
    }
    slots.insert(
        path.to_path_buf(),
        Slot {
            index: Arc::clone(&index),
            modified,
            generation: seen_generation + 1,
        },
    );
    index
}

fn publish(path: &Path, index: RegistryIndex) -> Arc<RegistryIndex> {
    let modified = modified_at(path);
    let index = Arc::new(index);
    let mut slots = lock(&SLOTS);
    let generation = slots.get(path).map_or(0, |slot| slot.generation) + 1;
    slots.insert(
        path.to_path_buf(),
        Slot {
            index: Arc::clone(&index),
            modified,
            generation,
        },
    );
    index
}

fn persist(path: &Path, index: &RegistryIndex) -> Result<(), String> {
    let json = serde_json::to_string(index).map_err(|e| e.to_string())?;
    crate::registry::atomic_write(path, &json)
}

/// Changes the newest index and writes it. Writers take turns so none starts from a copy
/// another one is about to replace; the memory copy is updated even when the disk write fails,
/// so an unwritable file only costs persistence.
fn update(
    path: &Path,
    change: impl FnOnce(&mut RegistryIndex),
) -> Result<Arc<RegistryIndex>, String> {
    let _writer = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut index = (*load(path)).clone();
    change(&mut index);
    index.version = INDEX_FILE_VERSION;
    let written = persist(path, &index);
    let published = publish(path, index);
    written
        .map(|()| published)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub(crate) fn merge_live(path: &Path, entries: &[CatalogEntry]) {
    let index = load(path);
    if entries
        .iter()
        .all(|entry| index.entries.get(&key_of(&entry.name)) == Some(entry))
    {
        return;
    }
    let _ = update(path, |index| {
        for entry in entries {
            index.entries.insert(key_of(&entry.name), entry.clone());
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    /// Resume an unfinished pass, else fetch what changed since the last one, else everything.
    Incremental,
    /// Start a new pass over the whole registry, dropping what it no longer lists.
    Full,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub kind: &'static str,
    pub pages: u32,
    pub fetched: usize,
    pub count: usize,
    pub complete: bool,
    pub last_synced_at: Option<String>,
}

#[derive(Debug)]
pub enum SyncError {
    Running,
    NoDataDir,
    Registry(RegistryError, SyncReport),
    Storage(String, SyncReport),
}

impl SyncError {
    pub fn kind(&self) -> &str {
        match self {
            SyncError::Running => "running",
            SyncError::NoDataDir => "noDataDir",
            SyncError::Registry(error, _) => &error.kind,
            SyncError::Storage(..) => "storage",
        }
    }

    pub fn message(&self) -> String {
        match self {
            SyncError::Running => "a registry sync is already running".into(),
            SyncError::NoDataDir => "the data directory could not be resolved".into(),
            SyncError::Registry(error, report) => format!(
                "the MCP Registry sync stopped after {} page(s), the index holds {} \
                 server(s): {}. Run it again to continue",
                report.pages, report.count, error.message
            ),
            SyncError::Storage(message, _) => message.clone(),
        }
    }

    pub fn report(&self) -> Option<&SyncReport> {
        match self {
            SyncError::Registry(_, report) | SyncError::Storage(_, report) => Some(report),
            _ => None,
        }
    }
}

impl From<SyncError> for OpError {
    fn from(error: SyncError) -> Self {
        let message = error.message();
        match error {
            SyncError::Running => OpError::conflict(message),
            SyncError::NoDataDir => OpError::failed("data_dir", message),
            SyncError::Registry(..) => OpError::failed("registry", message),
            SyncError::Storage(..) => OpError::failed("write_failed", message),
        }
    }
}

struct Flight(PathBuf);

impl Flight {
    fn acquire(path: &Path) -> Option<Flight> {
        lock(&RUNNING)
            .insert(path.to_path_buf())
            .then(|| Flight(path.to_path_buf()))
    }
}

impl Drop for Flight {
    fn drop(&mut self) {
        lock(&RUNNING).remove(&self.0);
    }
}

pub(crate) fn is_running(path: &Path) -> bool {
    lock(&RUNNING).contains(path)
}

struct Plan {
    kind: &'static str,
    cursor: Option<String>,
    since: Option<String>,
    pass_started_at: String,
    /// Names the pass has listed, kept only for a pass that ran from its first page in this
    /// process: that is the only one that can tell what the registry no longer lists.
    seen: Option<HashSet<String>>,
}

fn plan(index: &RegistryIndex, mode: SyncMode, now: &str) -> Plan {
    let fresh = |kind| Plan {
        kind,
        cursor: None,
        since: None,
        pass_started_at: now.to_string(),
        seen: Some(HashSet::new()),
    };
    if mode == SyncMode::Full {
        return fresh("full");
    }
    if let Some(cursor) = &index.resume_cursor {
        return Plan {
            kind: "resume",
            cursor: Some(cursor.clone()),
            since: None,
            pass_started_at: index
                .pass_started_at
                .clone()
                .unwrap_or_else(|| now.to_string()),
            seen: None,
        };
    }
    match (&index.last_synced_at, index.complete) {
        (Some(since), true) => Plan {
            kind: "incremental",
            cursor: None,
            since: Some(since.clone()),
            pass_started_at: now.to_string(),
            seen: None,
        },
        _ => fresh("full"),
    }
}

fn fetch_page(
    agent: &ureq::Agent,
    url: &str,
    retry_pause: Duration,
) -> Result<RegistryPage, RegistryError> {
    catalog::fetch_registry_page(agent, url).or_else(|_| {
        std::thread::sleep(retry_pause);
        catalog::fetch_registry_page(agent, url)
    })
}

/// Syncs the index at `path` from the registry at `base_url`, one page at a time. Nothing is
/// locked while a page is fetched; each page is merged and written before the next is asked for.
pub(crate) fn sync_at(
    path: &Path,
    base_url: &str,
    mode: SyncMode,
    tuning: Tuning,
    progress: &mut dyn FnMut(&SyncReport),
) -> Result<SyncReport, SyncError> {
    let _flight = Flight::acquire(path).ok_or(SyncError::Running)?;
    let started_at = cron::rfc3339(cron::now());
    let start = load(path);
    let Plan {
        kind,
        mut cursor,
        since,
        pass_started_at,
        mut seen,
    } = plan(&start, mode, &started_at);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(tuning.connect_timeout)
        .timeout_read(tuning.read_timeout)
        .build();
    let mut report = SyncReport {
        kind,
        pages: 0,
        fetched: 0,
        count: start.len(),
        complete: start.complete,
        last_synced_at: start.last_synced_at.clone(),
    };
    loop {
        if report.pages >= tuning.max_pages {
            let error = RegistryError {
                kind: "other".into(),
                message: format!("the registry listed more than {} pages", tuning.max_pages),
            };
            return Err(SyncError::Registry(error, report));
        }
        let url = catalog::registry_page_url(base_url, cursor.as_deref(), since.as_deref());
        let page = fetch_page(&agent, &url, tuning.retry_pause)
            .map_err(|error| SyncError::Registry(error, report.clone()))?;
        report.pages += 1;
        report.fetched += page.entries.len() + page.retired.len();
        if let Some(seen) = seen.as_mut() {
            seen.extend(page.entries.iter().map(|entry| key_of(&entry.name)));
        }
        if page.next_cursor.is_some() && page.next_cursor == cursor {
            let error = RegistryError {
                kind: "other".into(),
                message: "the registry repeated a page cursor".into(),
            };
            return Err(SyncError::Registry(error, report));
        }
        let next = page.next_cursor.clone();
        let finished = next.is_none();
        let updated = update(path, |index| {
            for name in &page.retired {
                index.entries.remove(&key_of(name));
            }
            for entry in page.entries {
                index.entries.insert(key_of(&entry.name), entry);
            }
            if finished {
                if let Some(seen) = &seen {
                    index.entries.retain(|key, _| seen.contains(key));
                }
                index.complete = true;
                index.last_synced_at = Some(pass_started_at.clone());
                index.resume_cursor = None;
                index.pass_started_at = None;
            } else if since.is_none() {
                index.resume_cursor = next.clone();
                index.pass_started_at = Some(pass_started_at.clone());
            }
        })
        .map_err(|message| {
            let mut stopped = report.clone();
            stopped.count = load(path).len();
            SyncError::Storage(message, stopped)
        })?;
        report.count = updated.len();
        report.complete = updated.complete;
        report.last_synced_at = updated.last_synced_at.clone();
        progress(&report);
        match next {
            Some(next) => cursor = Some(next),
            None => return Ok(report),
        }
    }
}

pub fn sync(
    mode: SyncMode,
    progress: &mut dyn FnMut(&SyncReport),
) -> Result<SyncReport, SyncError> {
    let path = default_path().ok_or(SyncError::NoDataDir)?;
    let result = sync_at(&path, catalog::REGISTRY_URL, mode, TUNING, progress);
    if let Err(error) = &result {
        if !matches!(error, SyncError::Running) {
            crate::gatewaylog::append(&format!(
                "toolport: registry index sync failed ({}): {}",
                error.kind(),
                error.message()
            ));
        }
    }
    result
}

pub(crate) fn status_at(path: &Path) -> Value {
    let index = load(path);
    json!({
        "count": index.len(),
        "lastSyncedAt": index.last_synced_at,
        "complete": index.complete,
        "running": is_running(path),
    })
}

pub fn status() -> Value {
    match default_path() {
        Some(path) => status_at(&path),
        None => json!({"count": 0, "lastSyncedAt": null, "complete": false, "running": false}),
    }
}

fn skipped(reason: &str, path: &Path) -> Value {
    let mut out = status_at(path);
    out["ran"] = json!(false);
    out["reason"] = json!(reason);
    out
}

/// One scheduled pass: syncs when the index is unfinished or older than 12 hours and no sync
/// ran in the last ten minutes. A failed sync is reported in `error`, not as a failure of the
/// tick, since the watcher that calls it has nobody to tell.
pub(crate) fn tick_at(
    path: &Path,
    base_url: &str,
    tuning: Tuning,
    force: bool,
    now: i64,
) -> Value {
    if is_running(path) {
        return skipped("running", path);
    }
    if !force {
        let recent = lock(&ATTEMPTS)
            .get(path)
            .is_some_and(|at| now - at < RETRY_AFTER.as_secs() as i64);
        if !load(path).due(now) || recent {
            return skipped("not-due", path);
        }
    }
    lock(&ATTEMPTS).insert(path.to_path_buf(), now);
    match sync_at(path, base_url, SyncMode::Incremental, tuning, &mut |_| {}) {
        Ok(report) => {
            let mut out = status_at(path);
            out["ran"] = json!(true);
            out["reason"] = Value::Null;
            out["report"] = json!(report);
            out
        }
        Err(SyncError::Running) => skipped("running", path),
        Err(error) => {
            let mut out = status_at(path);
            out["ran"] = json!(true);
            out["reason"] = Value::Null;
            out["error"] = json!({"kind": error.kind(), "message": error.message()});
            out
        }
    }
}

pub fn tick(force: bool) -> Result<Value, String> {
    let path = default_path().ok_or("the data directory could not be resolved")?;
    Ok(tick_at(&path, catalog::REGISTRY_URL, TUNING, force, cron::now()))
}

pub fn status_text(status: &Value) -> String {
    let count = status["count"].as_u64().unwrap_or(0);
    let synced = status["lastSyncedAt"].as_str();
    let state = match (status["complete"].as_bool().unwrap_or(false), synced) {
        (true, Some(at)) => format!("synced {at}"),
        (_, Some(at)) => format!("partial, last full sync {at}"),
        _ => "never fully synced".to_string(),
    };
    let running = if status["running"].as_bool().unwrap_or(false) {
        ", a sync is running"
    } else {
        ""
    };
    format!("Registry index: {count} servers, {state}{running}")
}

#[cfg(test)]
#[path = "catalog_index_testkit.rs"]
pub(crate) mod testkit;

#[cfg(test)]
#[path = "catalog_index_tests.rs"]
mod tests;
