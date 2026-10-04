use super::{
    BridgeError, ChildEnv, Emitter, EventKind, JobEvent, JobId, JobResult, Secret,
};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(20);
const DRAIN: Duration = Duration::from_millis(1500);
const CANCEL_WAIT: Duration = Duration::from_secs(10);
const MAX_STDOUT: usize = 32 * 1024 * 1024;
const MAX_STDERR_LINES: usize = 4000;
const MAX_LINE: usize = 8 * 1024;

#[derive(Default)]
struct State {
    cancel: bool,
    result: Option<JobResult>,
    finished_at: Option<Instant>,
}

pub(super) struct Job {
    pid: u32,
    state: Mutex<State>,
    wake: Condvar,
    done: Condvar,
}

impl Job {
    pub(super) fn pid(&self) -> u32 {
        self.pid
    }

    pub(super) fn finished_at(&self) -> Option<Instant> {
        self.lock().finished_at
    }

    pub(super) fn wait(&self) -> JobResult {
        let mut state = self.lock();
        loop {
            if let Some(result) = &state.result {
                return result.clone();
            }
            state = self.done.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(super) fn cancel(&self) {
        let mut state = self.lock();
        if state.result.is_some() {
            return;
        }
        state.cancel = true;
        self.wake.notify_all();
        let deadline = Instant::now() + CANCEL_WAIT;
        while state.result.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            state = self
                .done
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Default)]
struct Capture {
    stdout: Vec<u8>,
    stdout_truncated: bool,
    stderr: Vec<String>,
    stderr_truncated: bool,
}

pub(super) fn spawn(
    binary: &Path,
    env: &ChildEnv,
    id: JobId,
    argv: Vec<String>,
    secret: Option<Secret>,
    emitter: Arc<dyn Emitter>,
) -> Result<Arc<Job>, BridgeError> {
    let mut command = Command::new(binary);
    if env.clear {
        command.env_clear();
    }
    for (key, value) in &env.vars {
        command.env(key, value);
    }
    if !argv.iter().any(|a| a == "--json") {
        command.arg("--json");
    }
    command
        .args(&argv)
        .stdin(if secret.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|e| BridgeError::Spawn(format!("{}: {e}", binary.display())))?;

    let secret = secret.map(Arc::new);
    if let (Some(secret), Some(mut stdin)) = (secret.clone(), child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(secret.bytes());
        });
    }

    let capture = Arc::new(Mutex::new(Capture::default()));
    let seq = Arc::new(AtomicU64::new(1));
    let (done_tx, done_rx) = mpsc::channel();
    if let Some(pipe) = child.stdout.take() {
        let (capture, done) = (capture.clone(), done_tx.clone());
        std::thread::spawn(move || {
            read_stdout(pipe, &capture);
            let _ = done.send(());
        });
    }
    if let Some(pipe) = child.stderr.take() {
        let ctx = StderrCtx {
            id: id.clone(),
            seq: seq.clone(),
            capture: capture.clone(),
            secret: secret.clone(),
            emitter: emitter.clone(),
        };
        let done = done_tx.clone();
        std::thread::spawn(move || {
            read_stderr(pipe, &ctx);
            let _ = done.send(());
        });
    }
    drop(done_tx);

    let job = Arc::new(Job {
        pid: child.id(),
        state: Mutex::new(State::default()),
        wake: Condvar::new(),
        done: Condvar::new(),
    });
    let monitor = Monitor {
        job: job.clone(),
        child,
        id,
        capture,
        seq,
        secret,
        emitter,
        done_rx,
    };
    std::thread::spawn(move || monitor.run());
    Ok(job)
}

fn read_some(pipe: &mut impl Read, buf: &mut [u8]) -> usize {
    loop {
        match pipe.read(buf) {
            Ok(n) => return n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return 0,
        }
    }
}

fn read_stdout(mut pipe: impl Read, capture: &Mutex<Capture>) {
    let mut chunk = [0u8; 8192];
    loop {
        let n = read_some(&mut pipe, &mut chunk);
        if n == 0 {
            break;
        }
        let mut capture = capture.lock().unwrap_or_else(|e| e.into_inner());
        let room = MAX_STDOUT.saturating_sub(capture.stdout.len());
        capture.stdout.extend_from_slice(&chunk[..n.min(room)]);
        capture.stdout_truncated |= n > room;
    }
}

struct StderrCtx {
    id: JobId,
    seq: Arc<AtomicU64>,
    capture: Arc<Mutex<Capture>>,
    secret: Option<Arc<Secret>>,
    emitter: Arc<dyn Emitter>,
}

impl StderrCtx {
    fn line(&self, bytes: &[u8]) {
        let mut text = String::from_utf8_lossy(bytes).into_owned();
        if let Some(secret) = &self.secret {
            text = secret.scrub(&text);
        }
        let text = text.trim_end_matches('\r').to_string();
        if text.trim().is_empty() {
            return;
        }
        {
            let mut capture = self.capture.lock().unwrap_or_else(|e| e.into_inner());
            if capture.stderr.len() < MAX_STDERR_LINES {
                capture.stderr.push(text.clone());
            } else {
                capture.stderr_truncated = true;
            }
        }
        self.emitter.emit(&JobEvent {
            job: self.id.clone(),
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
            kind: EventKind::Stderr,
            line: Some(text),
            exit_code: None,
            cancelled: None,
        });
    }
}

fn read_stderr(mut pipe: impl Read, ctx: &StderrCtx) {
    let mut chunk = [0u8; 4096];
    let mut line: Vec<u8> = Vec::new();
    loop {
        let n = read_some(&mut pipe, &mut chunk);
        if n == 0 {
            break;
        }
        for &byte in &chunk[..n] {
            if byte == b'\n' {
                ctx.line(&line);
                line.clear();
            } else if line.len() < MAX_LINE {
                line.push(byte);
            }
        }
    }
    if !line.is_empty() {
        ctx.line(&line);
    }
}

struct Monitor {
    job: Arc<Job>,
    child: Child,
    id: JobId,
    capture: Arc<Mutex<Capture>>,
    seq: Arc<AtomicU64>,
    secret: Option<Arc<Secret>>,
    emitter: Arc<dyn Emitter>,
    done_rx: Receiver<()>,
}

impl Monitor {
    fn run(mut self) {
        let (status, cancelled) = self.wait_for_exit();
        self.drain_pipes();
        let result = self.result(status, cancelled);
        self.emitter.emit(&JobEvent {
            job: self.id.clone(),
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
            kind: EventKind::Exit,
            line: None,
            exit_code: result.exit_code,
            cancelled: Some(result.cancelled),
        });
        let mut state = self.job.lock();
        state.result = Some(result);
        state.finished_at = Some(Instant::now());
        self.job.done.notify_all();
    }

    fn wait_for_exit(&mut self) -> (Option<ExitStatus>, bool) {
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return (Some(status), false),
                Ok(None) => {}
                Err(_) => return (None, false),
            }
            let state = self.job.lock();
            if state.cancel {
                drop(state);
                return (kill_and_reap(&mut self.child), true);
            }
            let _ = self
                .job
                .wake
                .wait_timeout(state, POLL)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    fn drain_pipes(&self) {
        let deadline = Instant::now() + DRAIN;
        for _ in 0..2 {
            let left = deadline.saturating_duration_since(Instant::now());
            if self.done_rx.recv_timeout(left).is_err() {
                break;
            }
        }
    }

    fn result(&self, status: Option<ExitStatus>, cancelled: bool) -> JobResult {
        let mut capture = self.capture.lock().unwrap_or_else(|e| e.into_inner());
        let mut text = String::from_utf8_lossy(&capture.stdout).into_owned();
        if let Some(secret) = &self.secret {
            text = secret.scrub(&text);
        }
        let (envelope, parse_error) = if cancelled {
            (None, None)
        } else if capture.stdout_truncated {
            (None, Some("stdout exceeded the size limit".to_string()))
        } else {
            match parse_envelope(&text) {
                Ok(envelope) => (Some(envelope), None),
                Err(why) => (None, Some(why)),
            }
        };
        JobResult {
            job: self.id.clone(),
            exit_code: status.and_then(|s| s.code()),
            signal: status.and_then(signal_of),
            cancelled,
            envelope,
            parse_error,
            stderr: std::mem::take(&mut capture.stderr),
            truncated: capture.stdout_truncated || capture.stderr_truncated,
        }
    }
}

#[cfg(unix)]
fn signal_of(status: ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(&status)
}

#[cfg(not(unix))]
fn signal_of(_: ExitStatus) -> Option<i32> {
    None
}

/// The child leads its own process group, so one SIGKILL reaches the helpers it started (a
/// live `claude -p`, an inspected stdio server). Detached helpers that put themselves in their
/// own group, like the compression proxy, survive on purpose.
fn kill_and_reap(child: &mut Child) -> Option<ExitStatus> {
    #[cfg(unix)]
    unsafe {
        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
    }
    let _ = child.kill();
    child.wait().ok()
}

fn parse_envelope(stdout: &str) -> Result<Value, String> {
    let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());
    let Some(line) = lines.next() else {
        return Err("no output on stdout".into());
    };
    if lines.next().is_some() {
        return Err("stdout holds more than one line".into());
    }
    let value: Value =
        serde_json::from_str(line).map_err(|e| format!("stdout is not JSON: {e}"))?;
    let well_formed = value["ok"].is_boolean()
        && value["command"].is_string()
        && value["schemaVersion"].is_u64();
    if well_formed {
        Ok(value)
    } else {
        Err("stdout is not a toolportctl envelope".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_must_be_one_well_formed_line() {
        let ok = r#"{"ok":true,"command":"status","schemaVersion":1,"data":{}}"#;
        assert_eq!(parse_envelope(ok).unwrap()["command"], "status");
        assert_eq!(parse_envelope(&format!("{ok}\n")).unwrap()["ok"], true);
        assert!(parse_envelope("").unwrap_err().contains("no output"));
        assert!(parse_envelope(&format!("{ok}\n{ok}"))
            .unwrap_err()
            .contains("more than one"));
        assert!(parse_envelope("not json").unwrap_err().contains("not JSON"));
        assert!(parse_envelope(r#"{"ok":true}"#)
            .unwrap_err()
            .contains("not a toolportctl envelope"));
    }
}
