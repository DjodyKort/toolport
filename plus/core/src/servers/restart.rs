//! Restart one server on demand. A client reconnecting does not restart a server's process: the
//! gateway keeps each stdio child until the registry that started it changes or it idles for five
//! minutes, so a server whose code changed keeps running the old code. This is the explicit way:
//! the caller drops a request file in the data dir, every running gateway's watcher tick answers it
//! for the servers its router holds, and the caller reads the answers back.
//!
//! The caller can first ask how many calls are in flight (`--if-idle`): a restart ends them, so the
//! screen asks for confirmation only when there are some.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use toolport_plus_api::{GatewayRouter, Host};

use super::{find, read_servers};
use crate::op::OpError;

const DIR: &str = "server-restarts";
const REQUEST: &str = "req-";
const OUTCOME: &str = "res-";
const ACK: &str = "ack-";
/// A request nobody picked up in this long is stale: dropped, never run late.
const TTL_MS: u64 = 120_000;
pub const DEFAULT_WAIT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub poll: Duration,
    /// After the first answer, how long to keep reading for the other gateway processes.
    pub settle: Duration,
    /// How long to wait for any gateway to say it took the request before concluding that none is
    /// running. The watcher ticks every second; a request given up on is withdrawn, so it cannot
    /// run late.
    pub ack_wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(250),
            settle: Duration::from_millis(1_500),
            ack_wait: Duration::from_secs(6),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Restart,
    /// Count the calls in flight and change nothing.
    Probe,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Restart => "restart",
            Kind::Probe => "probe",
        }
    }
}

/// The files are written and read as plain JSON objects; keys are camelCase.
trait Record: Sized {
    fn to_value(&self) -> Value;
    fn from_value(value: &Value) -> Option<Self>;
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(String::from)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub nonce: String,
    pub server_id: String,
    pub server: String,
    pub at_ms: u64,
    pub kind: Kind,
}

impl Record for Request {
    fn to_value(&self) -> Value {
        json!({
            "nonce": self.nonce, "serverId": self.server_id, "server": self.server,
            "atMs": self.at_ms, "kind": self.kind.as_str(),
        })
    }

    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            nonce: text(value, "nonce")?,
            server_id: text(value, "serverId")?,
            server: text(value, "server")?,
            at_ms: value.get("atMs")?.as_u64()?,
            kind: match value.get("kind").and_then(Value::as_str) {
                Some("probe") => Kind::Probe,
                _ => Kind::Restart,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub nonce: String,
    pub pid: u32,
    pub server_id: String,
    pub restarted: bool,
    pub detail: String,
    /// Calls running on or waiting for the server in this gateway process.
    pub in_flight: Option<usize>,
}

impl Record for Outcome {
    fn to_value(&self) -> Value {
        let mut value = json!({
            "nonce": self.nonce, "pid": self.pid, "serverId": self.server_id,
            "restarted": self.restarted, "detail": self.detail,
        });
        if let Some(calls) = self.in_flight {
            value["inFlight"] = json!(calls);
        }
        value
    }

    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            nonce: text(value, "nonce")?,
            pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
            server_id: text(value, "serverId")?,
            restarted: value.get("restarted")?.as_bool()?,
            detail: text(value, "detail")?,
            in_flight: value
                .get("inFlight")
                .and_then(Value::as_u64)
                .map(|n| n as usize),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Ack {
    nonce: String,
    pid: u32,
}

impl Record for Ack {
    fn to_value(&self) -> Value {
        json!({"nonce": self.nonce, "pid": self.pid})
    }

    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            nonce: text(value, "nonce")?,
            pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
        })
    }
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

fn read_all<T: Record>(base: &Path, prefix: &str) -> Vec<(PathBuf, T)> {
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
            let value: Value = serde_json::from_str(&text).ok()?;
            Some((e.path(), T::from_value(&value)?))
        })
        .collect()
}

/// Ask every running gateway about `server_id`. The nonce of the result names the answers.
pub fn request(base: &Path, kind: Kind, server_id: &str, server: &str) -> std::io::Result<Request> {
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
        kind,
    };
    std::fs::create_dir_all(dir(base))?;
    let text = request.to_value().to_string();
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

/// The requests still worth answering at `now`; stale requests and leftovers are removed.
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

/// Which requests a process has taken, so one request restarts a server once although the
/// watcher ticks every second until the request file is gone.
#[derive(Default)]
pub struct Claims(Mutex<HashSet<String>>);

impl Claims {
    /// True the first time `nonce` is claimed.
    pub fn claim(&self, nonce: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(nonce.to_string())
    }
}

fn process_claims() -> &'static Claims {
    static CLAIMS: OnceLock<Claims> = OnceLock::new();
    CLAIMS.get_or_init(Claims::default)
}

fn record(base: &Path, outcome: &Outcome) -> std::io::Result<()> {
    std::fs::create_dir_all(dir(base))?;
    let text = outcome.to_value().to_string();
    write_atomic(
        &dir(base).join(format!("{OUTCOME}{}-{}.json", outcome.nonce, outcome.pid)),
        &text,
    )
}

fn acknowledge(base: &Path, nonce: &str, pid: u32) -> std::io::Result<()> {
    let ack = Ack {
        nonce: nonce.to_string(),
        pid,
    };
    std::fs::create_dir_all(dir(base))?;
    let text = ack.to_value().to_string();
    write_atomic(&dir(base).join(format!("{ACK}{nonce}-{pid}.json")), &text)
}

fn acknowledged(base: &Path, nonce: &str) -> Vec<u32> {
    let mut pids: Vec<u32> = read_all::<Ack>(base, &format!("{ACK}{nonce}-"))
        .into_iter()
        .map(|(_, a)| a.pid)
        .collect();
    pids.sort_unstable();
    pids
}

fn outcomes(base: &Path, nonce: &str) -> Vec<Outcome> {
    let mut found: Vec<Outcome> = read_all::<Outcome>(base, &format!("{OUTCOME}{nonce}-"))
        .into_iter()
        .map(|(_, o)| o)
        .collect();
    found.sort_by_key(|o| o.pid);
    found
}

fn finish(base: &Path, nonce: &str) {
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

/// What a gateway does on each watcher tick: answer the requests that name a server its router
/// holds, once per request and process. A probe is answered at once. A restart runs on its own
/// thread, because starting the fresh process can take seconds and the tick also reloads the
/// registry; the thread marks the server's lists stale when it is done. A request for a server
/// held elsewhere is left for that gateway; one this router does not hold yet (still connecting)
/// is looked at again on the next tick until it goes stale.
pub fn serve(base: &Path, gateway: Arc<dyn GatewayRouter>) -> Vec<JoinHandle<()>> {
    serve_with(base, gateway, std::process::id(), process_claims())
}

pub fn serve_with(
    base: &Path,
    gateway: Arc<dyn GatewayRouter>,
    pid: u32,
    claims: &Claims,
) -> Vec<JoinHandle<()>> {
    let mut running = Vec::new();
    for request in pending(base, now_ms()) {
        if !gateway.holds_server(&request.server_id) || !claims.claim(&request.nonce) {
            continue;
        }
        let _ = acknowledge(base, &request.nonce, pid);
        let outcome = Outcome {
            nonce: request.nonce.clone(),
            pid,
            server_id: request.server_id.clone(),
            restarted: false,
            detail: String::new(),
            in_flight: None,
        };
        if request.kind == Kind::Probe {
            let calls = gateway.calls_in_flight(&request.server_id).unwrap_or(0);
            let _ = record(
                base,
                &Outcome {
                    detail: match calls {
                        0 => "idle".to_string(),
                        n => format!("{n} call{} in flight", if n == 1 { "" } else { "s" }),
                    },
                    in_flight: Some(calls),
                    ..outcome
                },
            );
            continue;
        }
        let base = base.to_path_buf();
        let gateway = Arc::clone(&gateway);
        let spawned = std::thread::Builder::new()
            .name(format!("restart-{}", request.server_id))
            .spawn(move || {
                let answer = gateway.restart_server(&request.server_id);
                let (restarted, detail) = match answer {
                    Some(answer) => (answer.restarted, answer.detail),
                    None => (false, "the gateway no longer holds the server".to_string()),
                };
                if restarted {
                    gateway.mark_lists_stale();
                }
                eprintln!(
                    "toolport: restart of '{}' asked for: {detail}",
                    request.server
                );
                let _ = record(
                    &base,
                    &Outcome {
                        restarted,
                        detail,
                        ..outcome
                    },
                );
            });
        if let Ok(handle) = spawned {
            running.push(handle);
        }
    }
    running
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// `if_idle` was asked and a call is running: nothing was restarted.
    Busy,
}

pub struct RestartArgs {
    pub key: String,
    pub dry_run: bool,
    pub wait: Duration,
    pub if_idle: bool,
    pub timing: Timing,
}

impl RestartArgs {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            dry_run: false,
            wait: DEFAULT_WAIT,
            if_idle: false,
            timing: Timing::default(),
        }
    }
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Restarted => "restarted",
            Status::Failed => "failed",
            Status::NotRunning => "not_running",
            Status::Requested => "requested",
            Status::DryRun => "dry_run",
            Status::Busy => "busy",
        }
    }
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
            "status": self.status.as_str(),
            "gateways": self.outcomes.iter().map(|o| {
                let mut gateway = json!({"pid": o.pid, "restarted": o.restarted, "detail": o.detail});
                if let Some(calls) = o.in_flight {
                    gateway["inFlight"] = json!(calls);
                }
                gateway
            }).collect::<Vec<_>>(),
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
            Status::Busy => {
                let busy: Vec<String> = self
                    .outcomes
                    .iter()
                    .filter(|o| o.in_flight.unwrap_or(0) > 0)
                    .map(|o| format!("pid {}: {}", o.pid, o.detail))
                    .collect();
                format!(
                    "Did not restart {} ({}): {}. A restart ends those calls; run it again without --if-idle to go ahead.",
                    self.name,
                    self.id,
                    busy.join("; ")
                )
            }
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

/// Sends a request and reads the answers: until every gateway that took it has answered (plus a
/// short settle for processes slower to tick), the deadline, or no gateway taking it in time.
fn ask(
    base: &Path,
    sent: &Request,
    wait: Duration,
    timing: Timing,
    unanswered: &str,
) -> Vec<Outcome> {
    let started = Instant::now();
    let deadline = started + wait;
    let nobody_by = started + timing.ack_wait.min(wait);
    let mut settled: Option<Instant> = None;
    loop {
        std::thread::sleep(timing.poll);
        let now = Instant::now();
        let acked = acknowledged(base, &sent.nonce);
        let done = outcomes(base, &sent.nonce);
        if acked.is_empty() && now >= nobody_by {
            break;
        }
        if !acked.is_empty() && done.len() >= acked.len() {
            let until = *settled.get_or_insert(now + timing.settle);
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
    let acked = acknowledged(base, &sent.nonce);
    let mut found = outcomes(base, &sent.nonce);
    for pid in acked {
        if !found.iter().any(|o| o.pid == pid) {
            found.push(Outcome {
                nonce: sent.nonce.clone(),
                pid,
                server_id: sent.server_id.clone(),
                restarted: false,
                detail: format!("{unanswered} after {} s", wait.as_secs()),
                in_flight: None,
            });
        }
    }
    found.sort_by_key(|o| o.pid);
    finish(base, &sent.nonce);
    found
}

/// Restart `key` (an id or a name) in every running gateway and report what each one did.
pub fn restart(host: &dyn Host, args: &RestartArgs) -> Result<Restart, OpError> {
    let servers = read_servers(host)?;
    let server = find(&servers, &args.key)
        .ok_or_else(|| OpError::not_found(format!("no server '{}'", args.key)))?;
    let (id, name) = (server.id.clone(), server.name.clone());
    if args.dry_run {
        return Ok(Restart {
            id,
            name,
            status: Status::DryRun,
            outcomes: Vec::new(),
        });
    }
    let base = host
        .data_dir()
        .ok_or_else(|| OpError::failed("restart", "no data directory"))?;
    if args.if_idle {
        let wait = args.timing.ack_wait + args.timing.settle;
        let probe = request(&base, Kind::Probe, &id, &name)
            .map_err(|e| OpError::failed("restart", e.to_string()))?;
        let answers = ask(
            &base,
            &probe,
            wait.min(args.wait),
            args.timing,
            "no answer to the question",
        );
        if answers.iter().any(|o| o.in_flight.unwrap_or(0) > 0) {
            return Ok(Restart {
                id,
                name,
                status: Status::Busy,
                outcomes: answers,
            });
        }
    }
    let sent = request(&base, Kind::Restart, &id, &name)
        .map_err(|e| OpError::failed("restart", e.to_string()))?;
    if args.wait.is_zero() {
        return Ok(Restart {
            id,
            name,
            status: Status::Requested,
            outcomes: Vec::new(),
        });
    }
    let found = ask(&base, &sent, args.wait, args.timing, "still restarting");
    Ok(Restart {
        id,
        name,
        status: status_of(&found),
        outcomes: found,
    })
}

#[cfg(test)]
mod tests;
