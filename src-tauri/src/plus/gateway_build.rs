//! Progress of a gateway's initial catalog build.
//!
//! A gateway connects its downstream servers on a background thread after it starts. The tracker
//! records which servers are connected, failed or still connecting and how many tools the catalog
//! holds so far, and publishes that as `gateway-build-<pid>.json` in the data directory so
//! `toolportctl status` and the app can show a cold start while it is still running. Each gateway
//! process (host daemon, client-spawned gateway, HTTP bridge) owns one file; a file whose process
//! is gone is ignored by the reader and removed by the next gateway that starts.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_PREFIX: &str = "gateway-build-";
const FILE_SUFFIX: &str = ".json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerPhase {
    Connecting,
    Connected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProgress {
    pub id: String,
    pub state: ServerPhase,
    pub tools: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildState {
    pub pid: u32,
    /// `daemon`, `stdio` (a client-spawned gateway) or `http`.
    pub role: String,
    pub building: bool,
    pub servers_total: usize,
    pub servers_connected: usize,
    pub servers_failed: usize,
    /// Tools a client would see in the catalog built from the servers connected so far.
    pub tools_so_far: usize,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub servers: Vec<ServerProgress>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

pub struct BuildTracker {
    state: Mutex<BuildState>,
    dir: Option<PathBuf>,
    partial_served: AtomicBool,
}

impl BuildTracker {
    /// `dir` is where the state file is published; `None` keeps the state in memory only.
    pub fn new(role: &str, dir: Option<PathBuf>) -> Self {
        let now = now_ms();
        let tracker = Self {
            state: Mutex::new(BuildState {
                pid: std::process::id(),
                role: role.to_string(),
                building: true,
                servers_total: 0,
                servers_connected: 0,
                servers_failed: 0,
                tools_so_far: 0,
                started_at_ms: now,
                updated_at_ms: now,
                servers: Vec::new(),
            }),
            dir,
            partial_served: AtomicBool::new(false),
        };
        if let Some(dir) = tracker.dir.as_deref() {
            prune_dead(dir);
        }
        tracker.publish();
        tracker
    }

    pub fn snapshot(&self) -> BuildState {
        self.lock().clone()
    }

    /// The servers about to be connected, in registry order.
    pub fn plan(&self, ids: &[String]) {
        self.update(|state| {
            state.servers_total = ids.len();
            state.servers = ids
                .iter()
                .map(|id| ServerProgress {
                    id: id.clone(),
                    state: ServerPhase::Connecting,
                    tools: 0,
                })
                .collect();
        });
    }

    pub fn server_connected(&self, id: &str, tools: usize, tools_so_far: usize) {
        self.update(|state| {
            if let Some(server) = state.servers.iter_mut().find(|server| server.id == id) {
                server.state = ServerPhase::Connected;
                server.tools = tools;
            }
            state.tools_so_far = tools_so_far;
            recount(state);
        });
    }

    pub fn server_failed(&self, id: &str) {
        self.update(|state| {
            if let Some(server) = state.servers.iter_mut().find(|server| server.id == id) {
                server.state = ServerPhase::Failed;
            }
            recount(state);
        });
    }

    pub fn finish(&self, tools: usize) {
        self.update(|state| {
            state.building = false;
            state.tools_so_far = tools;
            recount(state);
        });
    }

    pub fn is_building(&self) -> bool {
        self.lock().building
    }

    /// A client was answered with a catalog that was still missing servers. The build then
    /// announces each server that joins, not only the end of the build.
    pub fn note_partial_served(&self) {
        self.partial_served.store(true, Ordering::SeqCst);
    }

    pub fn partial_served(&self) -> bool {
        self.partial_served.load(Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BuildState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn update(&self, change: impl FnOnce(&mut BuildState)) {
        {
            let mut state = self.lock();
            change(&mut state);
            state.updated_at_ms = now_ms();
        }
        self.publish();
    }

    fn publish(&self) {
        let Some(dir) = self.dir.as_deref() else {
            return;
        };
        let state = self.snapshot();
        let Ok(raw) = serde_json::to_string(&state) else {
            return;
        };
        let _ = crate::registry::atomic_write(&state_path(dir, state.pid), &raw);
    }
}

impl Drop for BuildTracker {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.as_deref() {
            let _ = std::fs::remove_file(state_path(dir, std::process::id()));
        }
    }
}

fn recount(state: &mut BuildState) {
    let count = |wanted: ServerPhase| {
        state
            .servers
            .iter()
            .filter(|server| server.state == wanted)
            .count()
    };
    state.servers_connected = count(ServerPhase::Connected);
    state.servers_failed = count(ServerPhase::Failed);
}

fn state_path(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("{FILE_PREFIX}{pid}{FILE_SUFFIX}"))
}

fn state_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX))
        })
        .collect()
}

fn read_state(path: &Path) -> Option<BuildState> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Every published build whose gateway process is still running, the host daemon first and then
/// the most recently updated. Reads only, so inspection never changes the data directory.
pub fn read_live(dir: &Path) -> Vec<BuildState> {
    let mut live: Vec<BuildState> = state_files(dir)
        .iter()
        .filter_map(|path| read_state(path))
        .filter(|state| crate::gateway_publish::pid_is_running(state.pid))
        .collect();
    live.sort_by(|a, b| {
        (b.role == "daemon")
            .cmp(&(a.role == "daemon"))
            .then(b.updated_at_ms.cmp(&a.updated_at_ms))
    });
    live
}

fn prune_dead(dir: &Path) {
    for path in state_files(dir) {
        let dead = match read_state(&path) {
            Some(state) => !crate::gateway_publish::pid_is_running(state.pid),
            None => true,
        };
        if dead {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gateway-build-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn progress_counts_connected_failed_and_pending_servers() {
        let tracker = BuildTracker::new("stdio", None);
        tracker.plan(&ids(&["a", "b", "c"]));
        let planned = tracker.snapshot();
        assert!(planned.building);
        assert_eq!(planned.servers_total, 3);
        assert_eq!(planned.servers_connected, 0);

        tracker.server_connected("a", 4, 4);
        tracker.server_failed("b");
        let mid = tracker.snapshot();
        assert!(mid.building);
        assert_eq!(
            (mid.servers_connected, mid.servers_failed, mid.tools_so_far),
            (1, 1, 4)
        );
        assert_eq!(mid.servers[2].state, ServerPhase::Connecting);

        tracker.server_connected("c", 2, 6);
        tracker.finish(6);
        let done = tracker.snapshot();
        assert!(!done.building);
        assert_eq!(
            (
                done.servers_connected,
                done.servers_failed,
                done.tools_so_far
            ),
            (2, 1, 6)
        );
    }

    #[test]
    fn the_state_file_is_published_read_back_and_removed_with_the_tracker() {
        let dir = scratch("file");
        let tracker = BuildTracker::new("daemon", Some(dir.clone()));
        tracker.plan(&ids(&["a"]));
        tracker.server_connected("a", 3, 3);

        let live = read_live(&dir);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].pid, std::process::id());
        assert_eq!(live[0].role, "daemon");
        assert_eq!(live[0].servers_connected, 1);
        assert_eq!(live[0].tools_so_far, 3);

        drop(tracker);
        assert!(read_live(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_of_a_dead_process_is_ignored_and_pruned_by_the_next_tracker() {
        let dir = scratch("stale");
        let stale = BuildState {
            pid: u32::MAX - 1,
            role: "stdio".to_string(),
            building: true,
            servers_total: 2,
            servers_connected: 0,
            servers_failed: 0,
            tools_so_far: 0,
            started_at_ms: 1,
            updated_at_ms: 1,
            servers: Vec::new(),
        };
        let path = state_path(&dir, stale.pid);
        std::fs::write(&path, serde_json::to_string(&stale).unwrap()).unwrap();
        std::fs::write(dir.join(format!("{FILE_PREFIX}garbage{FILE_SUFFIX}")), "{").unwrap();
        assert!(
            read_live(&dir).is_empty(),
            "a dead pid is not a live gateway"
        );
        assert!(path.exists(), "reading never changes the directory");

        let _tracker = BuildTracker::new("stdio", Some(dir.clone()));
        assert!(!path.exists());
        assert_eq!(
            state_files(&dir).len(),
            1,
            "only the new tracker's own file is left"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_daemon_is_listed_before_a_newer_client_gateway() {
        let dir = scratch("order");
        let mut daemon = BuildTracker::new("daemon", None).snapshot();
        daemon.updated_at_ms = 10;
        let mut stdio = daemon.clone();
        stdio.role = "stdio".to_string();
        stdio.updated_at_ms = 20;
        // Both rows claim this live test process, which is all `read_live` needs.
        std::fs::write(
            dir.join("gateway-build-1.json"),
            serde_json::to_string(&stdio).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("gateway-build-2.json"),
            serde_json::to_string(&daemon).unwrap(),
        )
        .unwrap();
        let roles: Vec<String> = read_live(&dir)
            .into_iter()
            .map(|state| state.role)
            .collect();
        assert_eq!(roles, ["daemon", "stdio"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serving_a_partial_catalog_is_remembered() {
        let tracker = BuildTracker::new("stdio", None);
        assert!(!tracker.partial_served());
        tracker.note_partial_served();
        assert!(tracker.partial_served());
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let tracker = BuildTracker::new("http", None);
        tracker.plan(&ids(&["a"]));
        let value = serde_json::to_value(tracker.snapshot()).unwrap();
        for key in [
            "pid",
            "role",
            "building",
            "serversTotal",
            "serversConnected",
            "serversFailed",
            "toolsSoFar",
            "startedAtMs",
            "updatedAtMs",
            "servers",
        ] {
            assert!(value.get(key).is_some(), "missing {key}: {value}");
        }
        assert_eq!(value["servers"][0]["state"], "connecting");
    }
}
