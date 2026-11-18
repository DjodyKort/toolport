//! The local index of the MCP Registry (D-118): every latest, active server in
//! `<data dir>/plus/registry-index.json`, so a catalog search no longer waits 24 to 60 s for the
//! registry's own cold `search=`. Upstream's catalog search asks [`answer`] first: the index
//! answers while it was synced within a day, and the live search is the fallback, its hits kept
//! in a small file beside the index.
//!
//! A sync pages the registry (`limit=100`, cursor), flushes every tenth page as a small part file
//! and writes the index once at the end, so a stopped pass resumes and the 21 MB file is not
//! rewritten per flush. Later syncs ask only for what changed (`updated_since`).

mod store;
mod sync;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use toolport_plus_api::{CatalogEntryRecord, Host};

pub use store::StateError;
pub use sync::{Mode, Report, REGISTRY_URL};

use crate::op::{ErrorKind, OpError};
use crate::tasks::cron::parse_rfc3339;
use crate::tasks::store::lock;

/// A live `search=` call is worth its wait only once the index is older than this.
pub const FRESH_FOR: Duration = Duration::from_secs(24 * 60 * 60);
pub const SYNC_EVERY: Duration = Duration::from_secs(12 * 60 * 60);
/// Between two background attempts, so a failing registry is not hammered.
pub const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);
/// Matches an index answers per query; a one-letter query would otherwise match all of it.
const SEARCH_LIMIT: usize = 100;
const TICK_EVERY: Duration = Duration::from_secs(60);
const FIRST_TICK_AFTER: Duration = Duration::from_secs(15);

fn plus_dir(host: &dyn Host) -> Option<PathBuf> {
    host.data_dir().map(|dir| store::plus_dir(&dir))
}

fn age_secs(last_synced_at: Option<&str>, now: i64) -> Option<i64> {
    Some(now - parse_rfc3339(last_synced_at?)?)
}

/// A timestamp in the future (clock set back) counts as old.
fn within(last_synced_at: Option<&str>, limit: Duration, now: i64) -> bool {
    age_secs(last_synced_at, now).is_some_and(|age| (0..limit.as_secs() as i64).contains(&age))
}

fn matches(host: &dyn Host, query: &str, only_fresh: bool) -> Option<Vec<CatalogEntryRecord>> {
    let loaded = store::load(&plus_dir(host)?);
    if loaded.is_empty() {
        return None;
    }
    if only_fresh && !within(loaded.last_synced_at.as_deref(), FRESH_FOR, host.now_unix()) {
        return None;
    }
    Some(loaded.search(query, SEARCH_LIMIT))
}

/// What the catalog search shows for `query`: the index while it is fresh, otherwise the live
/// search, whose hits join the index. When the live search fails and the index still holds
/// something, the older index answers instead of an error.
pub fn answer<E>(
    host: &dyn Host,
    query: &str,
    live: impl FnOnce(&str) -> Result<Vec<CatalogEntryRecord>, E>,
) -> Result<Vec<CatalogEntryRecord>, E> {
    if let Some(hits) = matches(host, query, true) {
        return Ok(hits);
    }
    match live(query) {
        Ok(hits) => {
            if let Some(dir) = plus_dir(host) {
                let _ = store::remember(&dir, &hits);
            }
            Ok(hits)
        }
        Err(error) => match matches(host, query, false) {
            Some(hits) if !hits.is_empty() => Ok(hits),
            _ => Err(error),
        },
    }
}

pub const STATE_FRESH: &str = "fresh";
pub const STATE_STALE: &str = "stale";
pub const STATE_SYNCING: &str = "syncing";
pub const STATE_ERROR: &str = "error";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub count: usize,
    pub last_synced_at: Option<String>,
    pub last_attempt_at: Option<String>,
    /// `fresh`, `stale`, `syncing`, or `error` (the last attempt failed and the index is stale).
    pub state: &'static str,
    /// An earlier pass stopped part-way; the next sync continues it.
    pub resumable: bool,
    pub error: Option<StateError>,
    /// Whether this build syncs in the background.
    pub background: bool,
}

fn syncing(dir: &std::path::Path) -> bool {
    let lock_path = dir.join(store::LOCK_FILE);
    lock_path.exists() && lock(&lock_path, Duration::ZERO).is_none()
}

pub fn background_enabled() -> bool {
    !std::env::var("TOOLPORT_CATALOG_INDEX_SYNC")
        .is_ok_and(|value| matches!(value.trim(), "0" | "off" | "false"))
}

pub fn status(host: &dyn Host) -> Status {
    let Some(dir) = plus_dir(host) else {
        return Status {
            count: 0,
            last_synced_at: None,
            last_attempt_at: None,
            state: STATE_STALE,
            resumable: false,
            error: None,
            background: background_enabled(),
        };
    };
    let loaded = store::load(&dir);
    let state = store::read_state(&dir);
    let fresh = within(loaded.last_synced_at.as_deref(), FRESH_FOR, host.now_unix());
    let state_name = if syncing(&dir) {
        STATE_SYNCING
    } else if fresh {
        STATE_FRESH
    } else if state.error.is_some() {
        STATE_ERROR
    } else {
        STATE_STALE
    };
    Status {
        count: loaded.synced_count(),
        last_synced_at: loaded.last_synced_at.clone(),
        last_attempt_at: state.last_attempt_at,
        state: state_name,
        resumable: store::part_count(&dir) > 0,
        error: state.error,
        background: background_enabled(),
    }
}

/// Syncs the index from the MCP Registry, calling `progress` after every page.
pub fn sync(
    host: &dyn Host,
    mode: Mode,
    progress: &mut dyn FnMut(&Report),
) -> Result<Report, OpError> {
    sync::sync_with(host, mode, sync::TUNING, progress)
}

#[derive(Debug, PartialEq)]
pub enum Tick {
    NotDue,
    Busy,
    Synced(Report),
    Failed(OpError),
}

/// One background pass: syncs when the index is unfinished or older than [`SYNC_EVERY`] and no
/// attempt started in the last [`RETRY_AFTER`].
pub fn tick(host: &dyn Host) -> Tick {
    tick_with(host, sync::TUNING)
}

fn tick_with(host: &dyn Host, tuning: sync::Tuning) -> Tick {
    let Some(dir) = plus_dir(host) else {
        return Tick::NotDue;
    };
    let now = host.now_unix();
    let loaded = store::load(&dir);
    let attempt = store::read_state(&dir).last_attempt_at;
    let recent = within(attempt.as_deref(), RETRY_AFTER, now);
    let due = store::part_count(&dir) > 0
        || loaded.is_empty()
        || !within(loaded.last_synced_at.as_deref(), SYNC_EVERY, now);
    if !due || recent {
        return Tick::NotDue;
    }
    match sync::sync_with(host, Mode::Incremental, tuning, &mut |_| {}) {
        Ok(report) => Tick::Synced(report),
        Err(error) if error.kind == ErrorKind::Conflict => Tick::Busy,
        Err(error) => Tick::Failed(error),
    }
}

/// The desktop app's ticker, on its own thread: a first sync takes minutes, which the task
/// scheduler's tick must not wait for. Does nothing when `TOOLPORT_CATALOG_INDEX_SYNC` is off.
pub fn spawn_app_ticker<H: Host + Send + 'static>(host: H) {
    if !background_enabled() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("catalog-index-ticker".into())
        .spawn(move || {
            std::thread::sleep(FIRST_TICK_AFTER);
            loop {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(&host)));
                std::thread::sleep(TICK_EVERY);
            }
        });
}
