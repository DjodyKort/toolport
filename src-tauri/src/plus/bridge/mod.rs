//! Child-process bridge between the app and the bundled `toolportctl` (D-060). The app never
//! re-implements a handler: it runs `toolportctl --json <argv>` as a child process, streams the
//! child's stderr lines as events, parses the single-line envelope on stdout and can cancel the
//! job. A secret travels over the child's stdin pipe only: never argv, never an event, never a log.
//!
//! This module has no Tauri dependency. The `AppHandle` side is a thin `Emitter` in `desktop.rs`.

#[cfg(feature = "desktop")]
pub mod app;
mod job;
mod secret;

pub use secret::Secret;

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const BINARY_NAME: &str = "toolportctl";
pub const EVENT_NAME: &str = "plus-ctl";

const FORBIDDEN_FLAGS: [&str; 2] = ["--home", "--data-dir"];
const FINISHED_KEEP: usize = 64;
const FINISHED_MAX_AGE: Duration = Duration::from_secs(600);

pub type JobId = String;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    Stderr,
    Exit,
}

/// One progress event of a job. `seq` counts per job from 1; a consumer that missed events can
/// still read every stderr line from the final [`JobResult`].
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub job: JobId,
    pub seq: u64,
    pub kind: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
}

pub trait Emitter: Send + Sync + 'static {
    fn emit(&self, event: &JobEvent);
}

impl<F: Fn(&JobEvent) + Send + Sync + 'static> Emitter for F {
    fn emit(&self, event: &JobEvent) {
        self(event)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub job: JobId,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub cancelled: bool,
    pub envelope: Option<Value>,
    pub parse_error: Option<String>,
    pub stderr: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug)]
pub enum BridgeError {
    BinaryNotFound(String),
    Forbidden(String),
    Usage(String),
    Spawn(String),
    UnknownJob(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryNotFound(why) => write!(f, "toolportctl not found: {why}"),
            Self::Forbidden(what) => write!(f, "argument not allowed through the bridge: {what}"),
            Self::Usage(what) => write!(f, "{what}"),
            Self::Spawn(why) => write!(f, "could not start toolportctl: {why}"),
            Self::UnknownJob(id) => write!(f, "unknown job: {id}"),
        }
    }
}

impl std::error::Error for BridgeError {}

/// `toolportctl` next to `exe`, the way Tauri's `externalBin` installs it.
pub fn binary_beside(exe: &Path) -> Option<PathBuf> {
    let candidate = exe
        .parent()?
        .join(format!("{BINARY_NAME}{}", std::env::consts::EXE_SUFFIX));
    candidate.is_file().then_some(candidate)
}

pub fn default_binary() -> Result<PathBuf, BridgeError> {
    let exe = std::env::current_exe().map_err(|e| BridgeError::BinaryNotFound(e.to_string()))?;
    binary_beside(&exe).ok_or_else(|| {
        BridgeError::BinaryNotFound(format!("no {BINARY_NAME} next to {}", exe.display()))
    })
}

/// The GUI picks the data directory the way the app does: through the inherited environment.
/// Flags that redirect the child's home or data directory are refused.
pub fn validate_argv(argv: &[String]) -> Result<(), BridgeError> {
    if argv.is_empty() {
        return Err(BridgeError::Usage("argv is empty".into()));
    }
    for arg in argv {
        if arg.contains('\0') {
            return Err(BridgeError::Usage("argv contains a NUL byte".into()));
        }
        let key = arg.split_once('=').map_or(arg.as_str(), |(key, _)| key);
        if FORBIDDEN_FLAGS.contains(&key) {
            return Err(BridgeError::Forbidden(key.to_string()));
        }
    }
    Ok(())
}

#[derive(Clone, Default)]
struct ChildEnv {
    clear: bool,
    vars: Vec<(OsString, OsString)>,
}

pub struct Bridge {
    binary: Option<PathBuf>,
    env: ChildEnv,
    jobs: Mutex<HashMap<JobId, Arc<job::Job>>>,
    next: AtomicU64,
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new()
    }
}

impl Bridge {
    /// Looks for `toolportctl` next to the running executable on every start.
    pub fn new() -> Self {
        Self {
            binary: None,
            env: ChildEnv::default(),
            jobs: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        }
    }

    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        let mut bridge = Self::new();
        bridge.binary = Some(path.into());
        bridge
    }

    /// Test seam: the child starts with an empty environment plus what `env` adds.
    pub fn clear_env(mut self) -> Self {
        self.env.clear = true;
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.vars.push((key.into(), value.into()));
        self
    }

    fn binary(&self) -> Result<PathBuf, BridgeError> {
        match &self.binary {
            Some(path) => Ok(path.clone()),
            None => default_binary(),
        }
    }

    pub fn start(
        &self,
        argv: Vec<String>,
        secret: Option<Secret>,
        emitter: Arc<dyn Emitter>,
    ) -> Result<JobId, BridgeError> {
        validate_argv(&argv)?;
        let binary = self.binary()?;
        let id = format!("job-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let job = job::spawn(&binary, &self.env, id.clone(), argv, secret, emitter)?;
        let mut jobs = self.lock();
        prune(&mut jobs);
        jobs.insert(id.clone(), job);
        Ok(id)
    }

    /// Blocks until the job has finished and hands over its result once.
    pub fn wait(&self, job: &str) -> Result<JobResult, BridgeError> {
        let handle = self.get(job)?;
        let result = handle.wait();
        self.lock().remove(job);
        Ok(result)
    }

    /// Kills the job's process group and returns once the child has been reaped. A job that has
    /// already finished is left alone.
    pub fn cancel(&self, job: &str) -> Result<(), BridgeError> {
        let handle = self.get(job)?;
        handle.cancel();
        Ok(())
    }

    pub fn pid(&self, job: &str) -> Option<u32> {
        self.get(job).ok().map(|handle| handle.pid())
    }

    fn get(&self, job: &str) -> Result<Arc<job::Job>, BridgeError> {
        self.lock()
            .get(job)
            .cloned()
            .ok_or_else(|| BridgeError::UnknownJob(job.to_string()))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<JobId, Arc<job::Job>>> {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let running: Vec<Arc<job::Job>> = self.lock().values().cloned().collect();
        for job in running {
            job.cancel();
        }
    }
}

fn prune(jobs: &mut HashMap<JobId, Arc<job::Job>>) {
    let now = Instant::now();
    jobs.retain(|_, job| job.finished_at().is_none_or(|at| now - at < FINISHED_MAX_AGE));
    let mut finished: Vec<(JobId, Instant)> = jobs
        .iter()
        .filter_map(|(id, job)| job.finished_at().map(|at| (id.clone(), at)))
        .collect();
    if finished.len() > FINISHED_KEEP {
        finished.sort_by_key(|(_, at)| *at);
        for (id, _) in &finished[..finished.len() - FINISHED_KEEP] {
            jobs.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn home_and_data_dir_flags_never_pass_the_bridge() {
        for bad in [
            argv(&["skills", "ls", "--home", "/tmp/x"]),
            argv(&["skills", "ls", "--home=/tmp/x"]),
            argv(&["--data-dir", "/tmp/x", "status"]),
            argv(&["status", "--data-dir=/tmp/x"]),
        ] {
            assert!(
                matches!(validate_argv(&bad), Err(BridgeError::Forbidden(_))),
                "{bad:?}"
            );
        }
        assert!(validate_argv(&argv(&["skills", "ls", "--repo", "/tmp/x"])).is_ok());
        assert!(validate_argv(&argv(&["server", "info", "--homepage"])).is_ok());
        assert!(matches!(validate_argv(&[]), Err(BridgeError::Usage(_))));
    }

    #[test]
    fn the_binary_is_found_next_to_the_executable() {
        let dir = std::env::temp_dir().join(format!("bridge-beside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(format!("app{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(binary_beside(&exe), None);
        let ctl = dir.join(format!("{BINARY_NAME}{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&ctl, b"").unwrap();
        assert_eq!(binary_beside(&exe), Some(ctl));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_binary_is_reported_without_a_fallback() {
        let bridge = Bridge::with_binary("/nonexistent/toolportctl");
        let emitter: Arc<dyn Emitter> = Arc::new(|_: &JobEvent| {});
        let error = bridge
            .start(argv(&["status"]), None, emitter)
            .unwrap_err();
        assert!(matches!(error, BridgeError::Spawn(_)), "{error}");
    }

    #[test]
    fn an_unknown_job_is_an_error_for_wait_and_cancel() {
        let bridge = Bridge::new();
        assert!(matches!(bridge.wait("job-9"), Err(BridgeError::UnknownJob(_))));
        assert!(matches!(bridge.cancel("job-9"), Err(BridgeError::UnknownJob(_))));
    }
}
