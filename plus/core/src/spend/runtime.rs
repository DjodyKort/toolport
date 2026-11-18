//! The receiver's host: a thread in the desktop app, with a tick from a gateway process as a
//! fallback for headless installs. The first process to bind the port wins; the others find it
//! taken and do nothing. Every tick also prunes the events once an hour.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::config::{self, SpendConfig};
use super::receiver::{Limits, Receiver};
use super::store::Store;
use super::transcript;
use super::{now_ms, spend_dir};

pub const STATE_FILE: &str = "receiver.json";
const HEARTBEAT_FRESH_MS: i64 = 20_000;
const PRUNE_EVERY_MS: i64 = 3_600_000;
const APP_TICK: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    App,
    Gateway,
}

impl Owner {
    fn name(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Gateway => "gateway",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverState {
    pub owner: String,
    pub pid: u32,
    pub port: u16,
    pub started_ms: i64,
    pub heartbeat_ms: i64,
    pub requests: u64,
    pub events: u64,
    pub rejected: u64,
    pub dropped: u64,
    pub last_event_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct Running {
    receiver: Receiver,
    owner: Owner,
    started_ms: i64,
    last_prune_ms: i64,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE)
}

fn read_state(dir: &Path) -> Option<ReceiverState> {
    serde_json::from_str(&std::fs::read_to_string(state_path(dir)).ok()?).ok()
}

fn write_state(dir: &Path, state: &ReceiverState) {
    if let Ok(text) = serde_json::to_string(state) {
        let _ = crate::fsx::atomic_write(&state_path(dir), &text, crate::fsx::Mode::Private);
    }
}

fn base_of(host: &dyn Host) -> Option<PathBuf> {
    host.registry_path()?.parent().map(Path::to_path_buf)
}

pub fn home_roots(host: &dyn Host) -> Vec<PathBuf> {
    let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    host.home_dir()
        .map(|home| transcript::roots_under(&home, config_dir.as_deref()))
        .unwrap_or_default()
}

pub fn tick_host(host: &dyn Host, owner: Owner) {
    let Some(base) = base_of(host) else {
        return;
    };
    tick(&spend_dir(&base), &home_roots(host), owner, now_ms());
}

pub fn tick(dir: &Path, roots: &[PathBuf], owner: Owner, now: i64) {
    let cfg = config::load(dir);
    let mut guard = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    if !cfg.enabled {
        stop_locked(&mut guard, dir);
        return;
    }
    if guard
        .as_ref()
        .is_some_and(|r| r.receiver.port() != cfg.port)
    {
        stop_locked(&mut guard, dir);
    }
    if guard.is_none() {
        match Receiver::bind(cfg.port, dir.to_path_buf(), Limits::default()) {
            Ok(receiver) => {
                *guard = Some(Running {
                    receiver,
                    owner,
                    started_ms: now,
                    last_prune_ms: now,
                });
            }
            Err(error) => {
                if owner == Owner::App && !other_owner_alive(dir, now) {
                    record_error(dir, &cfg, owner, now, &error.to_string());
                }
                return;
            }
        }
    }
    let Some(running) = guard.as_mut() else {
        return;
    };
    let c = &running.receiver.counters;
    use std::sync::atomic::Ordering::Relaxed;
    write_state(
        dir,
        &ReceiverState {
            owner: running.owner.name().to_string(),
            pid: std::process::id(),
            port: running.receiver.port(),
            started_ms: running.started_ms,
            heartbeat_ms: now,
            requests: c.requests.load(Relaxed),
            events: c.events.load(Relaxed),
            rejected: c.rejected.load(Relaxed),
            dropped: c.dropped.load(Relaxed),
            last_event_ms: c.last_event_ms.load(Relaxed),
            error: None,
        },
    );
    if now - running.last_prune_ms >= PRUNE_EVERY_MS {
        running.last_prune_ms = now;
        drop(guard);
        let _ = prune_with(dir, roots, &cfg, now, false);
    }
}

fn other_owner_alive(dir: &Path, now: i64) -> bool {
    read_state(dir).is_some_and(|s| {
        s.error.is_none()
            && s.pid != std::process::id()
            && now - s.heartbeat_ms < HEARTBEAT_FRESH_MS
    })
}

fn record_error(dir: &Path, cfg: &SpendConfig, owner: Owner, now: i64, error: &str) {
    write_state(
        dir,
        &ReceiverState {
            owner: owner.name().to_string(),
            pid: std::process::id(),
            port: cfg.port,
            heartbeat_ms: now,
            error: Some(format!("cannot listen on 127.0.0.1:{}: {error}", cfg.port)),
            ..ReceiverState::default()
        },
    );
}

fn stop_locked(guard: &mut Option<Running>, dir: &Path) {
    if guard.take().is_some() {
        let _ = std::fs::remove_file(state_path(dir));
    }
}

pub fn stop(dir: &Path) {
    let mut guard = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    stop_locked(&mut guard, dir);
}

pub fn prune_with(
    dir: &Path,
    roots: &[PathBuf],
    cfg: &SpendConfig,
    now: i64,
    dry_run: bool,
) -> Result<super::store::PruneReport, String> {
    let sessions = std::cell::OnceCell::new();
    let project_of = |id: &str| {
        sessions
            .get_or_init(|| transcript::scan(roots))
            .get(id)
            .and_then(|s| s.cwd.clone())
    };
    Store::new(dir).prune(
        now,
        cfg.retention_days,
        cfg.max_bytes(),
        &project_of,
        dry_run,
    )
}

pub fn spawn_app<H: Host + Send + 'static>(host: H) {
    let _ = std::thread::Builder::new()
        .name("spend-app-ticker".into())
        .spawn(move || loop {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tick_host(&host, Owner::App);
            }));
            std::thread::sleep(APP_TICK);
        });
}

pub fn status(dir: &Path, now: i64) -> Value {
    let cfg = config::load(dir);
    let store = Store::new(dir);
    let stats = store.stats();
    let state = read_state(dir);
    let alive = state
        .as_ref()
        .is_some_and(|s| s.error.is_none() && now - s.heartbeat_ms < HEARTBEAT_FRESH_MS);
    let error = state.as_ref().and_then(|s| s.error.clone());
    let phase = if !cfg.enabled {
        "off"
    } else if error.is_some() {
        "error"
    } else if !alive {
        "not-listening"
    } else if stats.events == 0 && stats.daily_lines == 0 {
        "waiting"
    } else {
        "collecting"
    };
    json!({
        "state": phase,
        "enabled": cfg.enabled,
        "receiver": {
            "address": format!("127.0.0.1:{}", cfg.port),
            "listening": alive,
            "owner": state.as_ref().filter(|_| alive).map(|s| s.owner.clone()),
            "requests": state.as_ref().map_or(0, |s| s.requests),
            "events": state.as_ref().map_or(0, |s| s.events),
            "rejected": state.as_ref().map_or(0, |s| s.rejected),
            "dropped": state.as_ref().map_or(0, |s| s.dropped),
            "error": error,
        },
        "lastEvent": stats.newest_ms.map(super::format_ts),
        "store": {
            "events": stats.events,
            "bytes": stats.bytes,
            "maxBytes": cfg.max_bytes(),
            "oldest": stats.oldest_ms.map(super::format_ts),
            "retentionDays": cfg.retention_days,
            "dailyLines": stats.daily_lines,
            "skippedLines": stats.skipped_lines,
        },
        "showCost": cfg.show_cost,
        "profilesSetUp": super::setup::applied_profiles(dir),
    })
}
