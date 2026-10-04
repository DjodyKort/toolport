//! Edge notifications for the desktop app (B2-03): which sign-in problems are worth a toast, and
//! only once. Edges come from the probe event log; what was already announced lives in a small
//! 0600 file next to the status cache. `surfaces::plan_notifications` does the dedupe.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::cache::{AuthStore, EdgeEvent, StatusFile};
use super::probe::Clock;
use super::surfaces::{self, Notification, NOTIFY_DEDUPE_SECS};
use super::SystemClock;

/// `cursor` is the newest event already considered; `sent` maps a dedupe key to when it was sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notified {
    #[serde(default)]
    pub cursor: i64,
    #[serde(default)]
    pub sent: BTreeMap<String, i64>,
}

/// An edge only counts while its server is still in the state it moved to: a login fixed since
/// then is not worth announcing.
pub fn due(
    events: &[EdgeEvent],
    status: &StatusFile,
    notified: &mut Notified,
    now: i64,
) -> Vec<Notification> {
    let current: Vec<EdgeEvent> = events
        .iter()
        .filter(|event| event.ts > notified.cursor)
        .filter(|event| {
            status
                .servers
                .get(&event.server)
                .is_some_and(|entry| entry.tracked.state.name() == event.to)
        })
        .cloned()
        .collect();
    let planned = surfaces::plan_notifications(&current, &notified.sent, now, NOTIFY_DEDUPE_SECS);
    let newest = events.iter().map(|event| event.ts).max().unwrap_or(0);
    notified.cursor = notified.cursor.max(newest);
    notified
        .sent
        .retain(|_, sent_at| now - *sent_at < NOTIFY_DEDUPE_SECS);
    for note in &planned {
        notified.sent.insert(note.dedupe_key.clone(), now);
    }
    planned
}

pub fn notifications_at(store: &AuthStore, now: i64) -> Result<Vec<Notification>, String> {
    let guard = store.lock()?;
    let events = guard.read_events();
    let status = guard.load_status();
    let path = store.notified_path();
    let before: Notified = crate::plus::jsonfs::read_json(&path).unwrap_or_default();
    let mut notified = before.clone();
    let planned = due(&events, &status, &mut notified, now);
    if notified != before {
        let text = serde_json::to_string_pretty(&notified).map_err(|e| e.to_string())?;
        crate::registry::atomic_write(&path, &text)?;
    }
    Ok(planned)
}

/// A first run on a machine without any probe history creates nothing on disk.
pub fn notifications_handler(_args: Value) -> Result<Value, String> {
    let dir = surfaces::auth_dir().ok_or_else(|| "data directory unavailable".to_string())?;
    let store = AuthStore::new(&dir);
    if !store.events_path().exists() {
        return Ok(json!({"notifications": []}));
    }
    let planned = notifications_at(&store, SystemClock.now())?;
    Ok(json!({"notifications": planned}))
}
