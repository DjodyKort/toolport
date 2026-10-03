//! Stdio sign-in capture (B2-01). A stdio server that authenticates out of band prints a consent
//! URL on stderr when started as `<command> <args> auth`. This is the one launcher behind the
//! stdio probe kind and `toolportctl auth login`.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use super::http_probes::{hint, hint_str};
use super::probe::{Probe, ProbeKind, ProbeRegistry, ProbeSpec};
use super::types::ProbeOutcome;
use crate::launch_inputs::{redact_env_secrets, resolve_args, ResolvedArgs};
use crate::registry::{Registry, ServerEntry};

pub const AUTH_SUBCOMMAND: &str = "auth";
pub const PROBE_URL_WAIT: Duration = Duration::from_secs(20);
pub const LOGIN_URL_WAIT: Duration = Duration::from_secs(45);
pub const LOGIN_FLOW_WAIT: Duration = Duration::from_secs(300);
pub const HINT_KIND: &str = "stdio";

const POLL: Duration = Duration::from_millis(50);
const EXIT_GRACE: Duration = Duration::from_secs(2);
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(300);
const TAIL_LINES: usize = 10;
const MAX_LINE_BYTES: u64 = 16 * 1024;
const QUEUE_LINES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchFault {
    Unsupported,
    Refused,
    Spawn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchError {
    pub fault: LaunchFault,
    pub message: String,
}

fn fault(fault: LaunchFault, message: impl Into<String>) -> LaunchError {
    LaunchError {
        fault,
        message: message.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub url: Option<String>,
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub tail: Vec<String>,
}

pub struct Session {
    child: Option<Child>,
    lines: Receiver<String>,
    tail: Vec<String>,
    exit: Option<i32>,
    server: ServerEntry,
    env: Vec<(String, String)>,
    resolved: ResolvedArgs,
}

pub fn find_consent_url(line: &str) -> Option<String> {
    let start = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| line.find(scheme))
        .min()?;
    let token = line[start..].split_whitespace().next()?;
    let url = token.trim_end_matches([',', '.', ')', '"', '\'']);
    let parsed = url::Url::parse(url).ok()?;
    (parsed.username().is_empty() && parsed.password().is_none()).then(|| url.to_string())
}

fn pump(stderr: impl Read, queue: SyncSender<String>) {
    let mut reader = BufReader::new(stderr);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match (&mut reader)
            .take(MAX_LINE_BYTES)
            .read_until(b'\n', &mut buf)
        {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf)
                    .trim_end_matches(['\r', '\n'])
                    .to_string();
                let _ = queue.try_send(line);
            }
        }
    }
}

pub fn start(server: &ServerEntry) -> Result<Session, LaunchError> {
    let command = match (&server.command, server.transport.as_str()) {
        (Some(command), "stdio") => command.clone(),
        _ => {
            return Err(fault(
                LaunchFault::Unsupported,
                "the auth flow only applies to stdio servers",
            ))
        }
    };
    let resolved = resolve_args(server).map_err(|m| fault(LaunchFault::Refused, m))?;
    let mut env: Vec<(String, String)> = Vec::new();
    for var in &server.env {
        let value = if var.secret {
            crate::secrets::get_secret(&server.id, &var.key)
        } else {
            var.value.clone()
        };
        if let Some(value) = value {
            env.push((var.key.clone(), value));
        }
    }
    let scrub = |message: String| redact_env_secrets(server, &env, resolved.redact(message));
    let (command, mut args) = crate::downstream::normalize_invocation(&command, &resolved.args);
    args.push(AUTH_SUBCOMMAND.to_string());
    crate::downstream::screen_spawn_command(&command, &args)
        .and_then(|()| crate::downstream::screen_spawn_env(&env))
        .map_err(|m| fault(LaunchFault::Refused, scrub(m)))?;
    let mut cmd = Command::new(crate::downstream::resolve_command(&command));
    crate::hostenv::strip_bundled_env(&mut cmd);
    cmd.args(&args).envs(env.iter().cloned());
    let configured: HashSet<&str> = env.iter().map(|(key, _)| key.as_str()).collect();
    crate::downstream::strip_gateway_control_env(&mut cmd, &configured);
    #[cfg(not(windows))]
    cmd.env("PATH", crate::downstream::augmented_path());
    if let Some(dir) = server
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
    {
        let dir = crate::downstream::validate_cwd(dir)
            .map_err(|m| fault(LaunchFault::Refused, scrub(m)))?;
        cmd.current_dir(dir);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        &mut cmd,
        windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
    );
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| fault(LaunchFault::Spawn, scrub(format!("could not start: {e}"))))?;
    let (queue, lines) = sync_channel(QUEUE_LINES);
    match child.stderr.take() {
        Some(stderr) => {
            std::thread::spawn(move || pump(stderr, queue));
        }
        None => drop(queue),
    }
    Ok(Session {
        child: Some(child),
        lines,
        tail: Vec::new(),
        exit: None,
        server: server.clone(),
        env,
        resolved,
    })
}

impl Session {
    fn scrub(&self, message: String) -> String {
        redact_env_secrets(&self.server, &self.env, self.resolved.redact(message))
    }

    /// A consent URL is the one line that must come through, so only values the launcher bound
    /// into the arguments are held back, not the whole message.
    fn scrub_url(&self, url: String) -> Option<String> {
        let url = redact_env_secrets(&self.server, &self.env, url);
        let bound = self
            .resolved
            .args
            .iter()
            .zip(&self.server.args)
            .filter(|(resolved, configured)| resolved != configured)
            .map(|(resolved, _)| resolved.as_str());
        let leaks = bound
            .into_iter()
            .any(|value| !value.is_empty() && url.contains(value));
        (!leaks).then_some(url)
    }

    fn push_tail(&mut self, line: String) {
        if self.server.launch.is_some() {
            return;
        }
        let line = self.scrub(line);
        self.tail.push(line);
        if self.tail.len() > TAIL_LINES {
            self.tail.remove(0);
        }
    }

    fn poll_exit(&mut self) -> Option<i32> {
        if self.exit.is_none() {
            let status = self.child.as_mut()?.try_wait().ok().flatten()?;
            self.exit = Some(status.code().unwrap_or(-1));
        }
        self.exit
    }

    fn capture(&self, url: Option<String>, exit: Option<i32>) -> Capture {
        Capture {
            timed_out: url.is_none() && exit.is_none(),
            url,
            exit,
            tail: self.tail.clone(),
        }
    }

    /// Returns at the first consent URL, leaving the child running to catch the redirect.
    pub fn wait_url(&mut self, wait: Duration) -> Capture {
        let deadline = Instant::now() + wait;
        let mut url = None;
        let mut exited_at: Option<Instant> = None;
        let mut closed = false;
        while url.is_none() && !closed && Instant::now() < deadline {
            if exited_at.is_some_and(|at| at.elapsed() >= DRAIN_AFTER_EXIT) {
                break;
            }
            match self.lines.recv_timeout(POLL) {
                Ok(line) => {
                    url = find_consent_url(&line).and_then(|found| self.scrub_url(found));
                    self.push_tail(line);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if exited_at.is_none() && self.poll_exit().is_some() {
                        exited_at = Some(Instant::now());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => closed = true,
            }
        }
        let exit = if url.is_some() {
            None
        } else {
            self.wait_exit(EXIT_GRACE)
        };
        self.capture(url, exit)
    }

    pub fn wait_exit(&mut self, wait: Duration) -> Option<i32> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(code) = self.poll_exit() {
                return Some(code);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(POLL);
        }
    }

    pub fn tail(&self) -> &[String] {
        &self.tail
    }

    pub fn kill(&mut self) {
        #[cfg(unix)]
        let running = self.poll_exit().is_none();
        let Some(mut child) = self.child.take() else {
            return;
        };
        // The child leads its own process group, so this reaches `npx` -> `node` chains.
        // An unreaped child still owns its pid, which keeps the group id from being reused.
        #[cfg(unix)]
        if running {
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Leaves the child running to finish the browser round trip on its own.
    pub fn detach(mut self) {
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

fn oauth(code: &str, description: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.to_string(),
        description: description.to_string(),
    }
}

pub fn classify(capture: &Capture) -> ProbeOutcome {
    let text = capture.tail.join("\n").to_ascii_lowercase();
    let revoked = text.contains("revoked");
    let expired = text.contains("expired");
    match (&capture.url, capture.exit) {
        (Some(_), _) if revoked => oauth("token_revoked", ""),
        (Some(_), _) if expired => oauth("token_expired", ""),
        (Some(_), _) => oauth("not_authed", ""),
        (None, Some(0)) => ProbeOutcome::Success,
        (None, Some(_)) if text.contains("invalid_grant") => {
            oauth("invalid_grant", if revoked { "revoked" } else { "" })
        }
        (None, Some(_)) if revoked => oauth("token_revoked", ""),
        (None, Some(_)) if expired => oauth("token_expired", ""),
        (None, _) => ProbeOutcome::TransportError,
    }
}

pub struct StdioProbe {
    wait: Duration,
}

impl std::fmt::Debug for StdioProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioProbe")
            .field("wait", &self.wait)
            .finish()
    }
}

impl Default for StdioProbe {
    fn default() -> Self {
        StdioProbe {
            wait: PROBE_URL_WAIT,
        }
    }
}

impl StdioProbe {
    pub fn with_wait(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }
}

impl Probe for StdioProbe {
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let Ok(registry) = super::scan::read_registry() else {
            return ProbeOutcome::TransportError;
        };
        let Some(server) = registry.servers.iter().find(|s| s.id == spec.server) else {
            return oauth("missing_config", "");
        };
        let mut session = match start(server) {
            Ok(session) => session,
            Err(error) if error.fault == LaunchFault::Spawn => return ProbeOutcome::TransportError,
            Err(_) => return oauth("missing_config", ""),
        };
        let capture = session.wait_url(self.wait);
        session.kill();
        classify(&capture)
    }
}

pub fn stdio_registry(registry: &Registry) -> ProbeRegistry {
    let mut reg = ProbeRegistry::new();
    let active = registry.active_profile_id();
    for server in &registry.servers {
        let opted_in = hint(server)
            .and_then(|h| hint_str(h, "kind"))
            .is_some_and(|kind| kind == HINT_KIND);
        let runnable = server.transport == "stdio" && server.command.is_some();
        let reviewed =
            !server.needs_team_enable_review() || registry.is_enabled(&active, &server.id);
        if opted_in && runnable && reviewed {
            reg.register(ProbeSpec::new(&server.id, ProbeKind::Stdio));
        }
    }
    reg
}
