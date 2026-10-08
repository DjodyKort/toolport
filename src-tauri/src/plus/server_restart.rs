//! Restart one server on demand. A client reconnecting does not restart a server's process: the
//! gateway keeps each stdio child for as long as the registry that started it is unchanged, so a
//! server whose code changed keeps running the old code. This is the explicit way: the caller
//! drops a request file, every running gateway's watcher tick restarts its own copy of that one
//! server and writes an outcome file, and the caller reads the outcomes back.

use crate::downstream;
use crate::plus::op::OpError;
use crate::plus::servers;
use crate::plus::status::readable_registry;
use crate::router::Router;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DIR: &str = "server-restarts";
const REQUEST: &str = "req-";
const OUTCOME: &str = "res-";
const ACK: &str = "ack-";
/// A request nobody picked up in this long is stale: dropped, never run late.
const TTL_MS: u64 = 120_000;
const POLL: Duration = Duration::from_millis(250);
/// After the first outcome, how long to keep reading for the other gateway processes.
const SETTLE: Duration = Duration::from_millis(1_500);
/// How long to wait for any gateway to say it picked the request up before concluding that none
/// is running. The watcher ticks every second; a gateway busy rebuilding can be later than this,
/// which is why a request that was given up on is withdrawn instead of being left to run late.
const ACK_WAIT: Duration = Duration::from_secs(6);
pub const DEFAULT_WAIT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub nonce: String,
    pub server_id: String,
    pub server: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub nonce: String,
    pub pid: u32,
    pub server_id: String,
    pub restarted: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ack {
    pub nonce: String,
    pub pid: u32,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn dir(base: &Path) -> PathBuf {
    base.join(DIR)
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn read_all<T: for<'de> Deserialize<'de>>(base: &Path, prefix: &str) -> Vec<(PathBuf, T)> {
    let Ok(entries) = std::fs::read_dir(dir(base)) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with(prefix) && name.ends_with(".json")
        })
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            Some((e.path(), serde_json::from_str(&text).ok()?))
        })
        .collect()
}

/// Ask every running gateway to restart `server_id`. Returns the request, whose nonce names the
/// outcomes to read back.
pub fn request(base: &Path, server_id: &str, server: &str) -> std::io::Result<Request> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let at_ms = now_ms();
    let nonce = format!(
        "{at_ms:x}-{:x}-{:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let request = Request {
        nonce: nonce.clone(),
        server_id: server_id.to_string(),
        server: server.to_string(),
        at_ms,
    };
    std::fs::create_dir_all(dir(base))?;
    let text = serde_json::to_string(&request).map_err(std::io::Error::other)?;
    write_atomic(&dir(base).join(format!("{REQUEST}{nonce}.json")), &text)?;
    Ok(request)
}

fn file_age_stale(path: &Path, now: u64) -> bool {
    let modified = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);
    modified.is_some_and(|m| now.saturating_sub(m) > TTL_MS)
}

/// The requests still worth running at `now`; stale requests and stale outcomes are removed.
pub fn pending(base: &Path, now: u64) -> Vec<Request> {
    for (path, _) in read_all::<Outcome>(base, OUTCOME) {
        if file_age_stale(&path, now) {
            let _ = std::fs::remove_file(path);
        }
    }
    for (path, _) in read_all::<Ack>(base, ACK) {
        if file_age_stale(&path, now) {
            let _ = std::fs::remove_file(path);
        }
    }
    let mut live = Vec::new();
    for (path, request) in read_all::<Request>(base, REQUEST) {
        if now.saturating_sub(request.at_ms) > TTL_MS {
            let _ = std::fs::remove_file(path);
        } else {
            live.push(request);
        }
    }
    live
}

/// True the first time this process sees `nonce`, so one request restarts a server once even
/// though the watcher ticks every second until the request file is gone.
pub fn claim(nonce: &str) -> bool {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(nonce.to_string())
}

pub fn record(base: &Path, outcome: &Outcome) -> std::io::Result<()> {
    std::fs::create_dir_all(dir(base))?;
    let text = serde_json::to_string(outcome).map_err(std::io::Error::other)?;
    write_atomic(
        &dir(base).join(format!("{OUTCOME}{}-{}.json", outcome.nonce, outcome.pid)),
        &text,
    )
}

/// A gateway says it took the request and is restarting the server now.
pub fn acknowledge(base: &Path, nonce: &str) -> std::io::Result<()> {
    let ack = Ack { nonce: nonce.to_string(), pid: std::process::id() };
    std::fs::create_dir_all(dir(base))?;
    let text = serde_json::to_string(&ack).map_err(std::io::Error::other)?;
    write_atomic(&dir(base).join(format!("{ACK}{nonce}-{}.json", ack.pid)), &text)
}

pub fn acknowledged(base: &Path, nonce: &str) -> Vec<u32> {
    let mut pids: Vec<u32> = read_all::<Ack>(base, &format!("{ACK}{nonce}-"))
        .into_iter()
        .map(|(_, a)| a.pid)
        .collect();
    pids.sort_unstable();
    pids
}

pub fn outcomes(base: &Path, nonce: &str) -> Vec<Outcome> {
    let mut found: Vec<Outcome> = read_all::<Outcome>(base, &format!("{OUTCOME}{nonce}-"))
        .into_iter()
        .map(|(_, o)| o)
        .collect();
    found.sort_by_key(|o| o.pid);
    found
}

pub fn finish(base: &Path, nonce: &str) {
    for (path, _) in read_all::<Request>(base, &format!("{REQUEST}{nonce}")) {
        let _ = std::fs::remove_file(path);
    }
    for (path, _) in read_all::<Outcome>(base, &format!("{OUTCOME}{nonce}-")) {
        let _ = std::fs::remove_file(path);
    }
    for (path, _) in read_all::<Ack>(base, &format!("{ACK}{nonce}-")) {
        let _ = std::fs::remove_file(path);
    }
}

/// What a gateway does on each watcher tick: restart the servers a pending request names and this
/// router holds, once per request, and mark their tool lists stale so they are read again on the
/// same tick. A request for a server held elsewhere is left for that gateway; one this router does
/// not hold yet (still connecting) is retried on the next tick until it goes stale.
pub fn serve(base: &Path, router: &Router, dirty: &AtomicU8) {
    for request in pending(base, now_ms()) {
        if !router.holds_server(&request.server_id) || !claim(&request.nonce) {
            continue;
        }
        let _ = acknowledge(base, &request.nonce);
        let restarted = router.restart_server(&request.server_id).unwrap_or(false);
        let detail = if restarted {
            dirty.fetch_or(
                downstream::change::TOOLS | downstream::change::RESOURCES | downstream::change::PROMPTS,
                Ordering::SeqCst,
            );
            "restarted".to_string()
        } else {
            "could not bring a fresh process up; the old one is still in place".to_string()
        };
        eprintln!("toolport: restart of '{}' asked for: {detail}", request.server);
        let _ = record(
            base,
            &Outcome {
                nonce: request.nonce,
                pid: std::process::id(),
                server_id: request.server_id,
                restarted,
                detail,
            },
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Every gateway that runs the server restarted it.
    Restarted,
    /// At least one gateway that runs the server could not bring it back.
    Failed,
    /// No running gateway holds the server, so there is nothing to restart: it starts fresh at
    /// its next call.
    NotRunning,
    /// Asked, and not waiting for the answer.
    Requested,
    DryRun,
}

pub struct RestartArgs {
    pub key: String,
    pub dry_run: bool,
    pub wait: Duration,
}

pub struct Restart {
    pub id: String,
    pub name: String,
    pub status: Status,
    pub outcomes: Vec<Outcome>,
}

impl Restart {
    pub fn failed(&self) -> bool {
        self.status == Status::Failed
    }

    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "status": self.status,
            "gateways": self.outcomes.iter().map(|o| json!({
                "pid": o.pid,
                "restarted": o.restarted,
                "detail": o.detail,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn line(&self) -> String {
        let count = self.outcomes.len();
        match self.status {
            Status::DryRun => format!(
                "Would restart {} ({}) in every running gateway; its tools refresh afterwards.",
                self.name, self.id
            ),
            Status::Requested => format!(
                "Asked every running gateway to restart {} ({}); not waiting for the answer.",
                self.name, self.id
            ),
            Status::NotRunning => format!(
                "No running gateway holds {} ({}), so there is nothing to restart. It starts fresh at its next call.",
                self.name, self.id
            ),
            Status::Restarted => format!(
                "Restarted {} ({}) in {count} gateway process{}; its tools refresh now.",
                self.name,
                self.id,
                if count == 1 { "" } else { "es" }
            ),
            Status::Failed => {
                let why: Vec<String> = self
                    .outcomes
                    .iter()
                    .filter(|o| !o.restarted)
                    .map(|o| format!("pid {}: {}", o.pid, o.detail))
                    .collect();
                format!(
                    "Could not restart {} ({}) in every gateway: {}",
                    self.name,
                    self.id,
                    why.join("; ")
                )
            }
        }
    }
}

fn status_of(outcomes: &[Outcome]) -> Status {
    if outcomes.is_empty() {
        Status::NotRunning
    } else if outcomes.iter().all(|o| o.restarted) {
        Status::Restarted
    } else {
        Status::Failed
    }
}

/// Restart `key` (an id or a name) in every running gateway and report what each one did.
pub fn restart(args: &RestartArgs) -> Result<Restart, OpError> {
    let reg = readable_registry()?;
    let server = servers::find(&reg, &args.key)
        .ok_or_else(|| OpError::not_found(format!("no server '{}'", args.key)))?;
    let (id, name) = (server.id.clone(), server.name.clone());
    if args.dry_run {
        return Ok(Restart { id, name, status: Status::DryRun, outcomes: Vec::new() });
    }
    let base = crate::registry::conduit_dir()
        .ok_or_else(|| OpError::failed("restart", "no data directory"))?;
    let sent = request(&base, &id, &name).map_err(|e| OpError::failed("restart", e.to_string()))?;
    if args.wait.is_zero() {
        return Ok(Restart { id, name, status: Status::Requested, outcomes: Vec::new() });
    }
    let started = Instant::now();
    let deadline = started + args.wait;
    let nobody_by = started + ACK_WAIT.min(args.wait);
    let mut settled: Option<Instant> = None;
    loop {
        std::thread::sleep(POLL);
        let now = Instant::now();
        let acked = acknowledged(&base, &sent.nonce);
        let done = outcomes(&base, &sent.nonce);
        if acked.is_empty() && now >= nobody_by {
            break;
        }
        if !acked.is_empty() && done.len() >= acked.len() {
            let until = *settled.get_or_insert(now + SETTLE);
            if now >= until {
                break;
            }
        } else {
            settled = None;
        }
        if now >= deadline {
            break;
        }
    }
    let acked = acknowledged(&base, &sent.nonce);
    let mut found = outcomes(&base, &sent.nonce);
    for pid in acked {
        if !found.iter().any(|o| o.pid == pid) {
            found.push(Outcome {
                nonce: sent.nonce.clone(),
                pid,
                server_id: id.clone(),
                restarted: false,
                detail: format!("still restarting after {} s", args.wait.as_secs()),
            });
        }
    }
    found.sort_by_key(|o| o.pid);
    finish(&base, &sent.nonce);
    Ok(Restart { id, name, status: status_of(&found), outcomes: found })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toolport-restart-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn outcome(nonce: &str, pid: u32, restarted: bool) -> Outcome {
        Outcome {
            nonce: nonce.into(),
            pid,
            server_id: "srv".into(),
            restarted,
            detail: if restarted { "restarted".into() } else { "could not connect".into() },
        }
    }

    #[test]
    fn a_request_is_pending_until_it_is_finished() {
        let base = scratch("pending");
        let sent = request(&base, "srv", "Server").unwrap();
        let live = pending(&base, now_ms());
        assert_eq!(live, vec![sent.clone()]);
        finish(&base, &sent.nonce);
        assert!(pending(&base, now_ms()).is_empty());
    }

    #[test]
    fn a_stale_request_is_dropped_instead_of_run_late() {
        let base = scratch("stale");
        let sent = request(&base, "srv", "Server").unwrap();
        assert!(pending(&base, sent.at_ms + TTL_MS + 1).is_empty());
        assert!(read_all::<Request>(&base, REQUEST).is_empty());
    }

    #[test]
    fn outcomes_are_per_request_and_ordered_by_process() {
        let base = scratch("outcomes");
        record(&base, &outcome("a", 30, true)).unwrap();
        record(&base, &outcome("a", 10, false)).unwrap();
        record(&base, &outcome("b", 20, true)).unwrap();
        let pids: Vec<u32> = outcomes(&base, "a").iter().map(|o| o.pid).collect();
        assert_eq!(pids, [10, 30]);
        finish(&base, "a");
        assert!(outcomes(&base, "a").is_empty());
        assert_eq!(outcomes(&base, "b").len(), 1);
    }

    #[test]
    fn an_acknowledgement_names_the_gateway_and_goes_with_the_request() {
        let base = scratch("ack");
        acknowledge(&base, "n1").unwrap();
        assert_eq!(acknowledged(&base, "n1"), [std::process::id()]);
        assert!(acknowledged(&base, "n2").is_empty());
        finish(&base, "n1");
        assert!(acknowledged(&base, "n1").is_empty());
    }

    #[test]
    fn a_request_is_claimed_once_per_process() {
        assert!(claim("claim-test-nonce"));
        assert!(!claim("claim-test-nonce"));
    }

    #[test]
    fn the_status_follows_what_every_gateway_did() {
        assert_eq!(status_of(&[]), Status::NotRunning);
        assert_eq!(status_of(&[outcome("n", 1, true), outcome("n", 2, true)]), Status::Restarted);
        assert_eq!(status_of(&[outcome("n", 1, true), outcome("n", 2, false)]), Status::Failed);
    }

    #[test]
    fn the_lines_name_the_server_and_say_what_happened() {
        let done = Restart {
            id: "id1".into(),
            name: "odoo".into(),
            status: Status::Restarted,
            outcomes: vec![outcome("n", 7, true)],
        };
        assert_eq!(done.line(), "Restarted odoo (id1) in 1 gateway process; its tools refresh now.");
        let none = Restart { status: Status::NotRunning, outcomes: Vec::new(), ..done };
        assert!(none.line().contains("starts fresh at its next call"));
    }
}
