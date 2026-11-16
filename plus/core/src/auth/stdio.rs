//! Sign-in capture for stdio servers. A server that authenticates out of band prints a consent
//! URL on stderr when started as `<command> <args> auth`. This is the one capture behind the
//! stdio probe, `toolportctl auth login` and the `servers_auth` tool; upstream starts the process.

use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use toolport_plus_api::{AuthChild, Host, LaunchError, LaunchFault, ServerRecord};

use super::probe::{Probe, ProbeKind, ProbeRegistry, ProbeSpec};
use super::types::ProbeOutcome;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub url: Option<String>,
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub tail: Vec<String>,
}

pub struct Session {
    child: Option<Box<dyn AuthChild>>,
    lines: Receiver<String>,
    tail: Vec<String>,
    exit: Option<i32>,
}

/// The first http(s) URL in a line, unless it carries credentials in its authority.
pub fn find_consent_url(line: &str) -> Option<String> {
    let start = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| line.find(scheme))
        .min()?;
    let token = line[start..].split_whitespace().next()?;
    let url = token.trim_end_matches([',', '.', ')', '"', '\'']);
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (!authority.is_empty() && !authority.contains('@')).then(|| url.to_string())
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

pub fn start(host: &dyn Host, server_id: &str) -> Result<Session, LaunchError> {
    let mut child = host.start_auth(server_id)?;
    let (queue, lines) = sync_channel(QUEUE_LINES);
    match child.take_stderr() {
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
    })
}

impl Session {
    fn push_tail(&mut self, line: String) {
        let Some(child) = &self.child else { return };
        if child.hides_output() {
            return;
        }
        self.tail.push(child.scrub(line));
        if self.tail.len() > TAIL_LINES {
            self.tail.remove(0);
        }
    }

    fn poll_exit(&mut self) -> Option<i32> {
        if self.exit.is_none() {
            self.exit = self.child.as_mut()?.try_exit();
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
                    url = find_consent_url(&line)
                        .and_then(|found| self.child.as_ref()?.scrub_url(found));
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

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.kill();
        }
    }

    /// Leaves the child running to finish the browser round trip on its own.
    pub fn detach(mut self) {
        if let Some(child) = self.child.take() {
            child.detach();
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

#[derive(Debug, Clone, Copy)]
pub struct StdioProbe {
    pub wait: Duration,
}

impl Default for StdioProbe {
    fn default() -> Self {
        StdioProbe {
            wait: PROBE_URL_WAIT,
        }
    }
}

impl Probe for StdioProbe {
    fn run(&self, host: &dyn Host, spec: &ProbeSpec) -> ProbeOutcome {
        let mut session = match start(host, &spec.server) {
            Ok(session) => session,
            Err(error) if error.fault == LaunchFault::Spawn => return ProbeOutcome::TransportError,
            Err(_) => return oauth("missing_config", ""),
        };
        let capture = session.wait_url(self.wait);
        session.kill();
        classify(&capture)
    }
}

/// Opted in with `plus.authProbe.kind = "stdio"`: a stdio server's own `auth` subcommand is the
/// probe, which only a server that has one can answer.
pub fn is_opted_in(server: &ServerRecord) -> bool {
    server
        .fork
        .plus
        .as_ref()
        .and_then(|plus| plus.auth_probe.as_ref())
        .and_then(|probe| probe.kind.as_deref())
        == Some(HINT_KIND)
}

pub fn stdio_registry(host: &dyn Host, servers: &[ServerRecord], registry: &mut ProbeRegistry) {
    for server in servers {
        let runnable =
            server.text("transport") == Some("stdio") && server.text("command").is_some();
        let reviewed = !host.awaits_review(&server.id).unwrap_or(true);
        if is_opted_in(server) && runnable && reviewed {
            registry.register(ProbeSpec::new(&server.id, ProbeKind::Stdio));
        }
    }
}
