//! Keeps the OTLP receiver running inside a long-lived Toolport process (MIG-OBS-3).
//!
//! The gateway starts [`spawn`] once. Every couple of seconds the host reads `obs/otel.json` and
//! starts, restarts or stops the receiver to match it. Several gateways can run at once (one per
//! client session); the first to bind the port serves, the others retry and take over when it
//! exits.

use super::otel_setup::{clear_host_status, load_config, save_host_status, HostStatus};
use super::receiver::{Receiver, StartError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_secs(10);

pub struct Host {
    dir: PathBuf,
    active: Option<(u16, Receiver)>,
    wanted: Option<u16>,
    retry_at: Option<Instant>,
    last_note: String,
}

impl Host {
    pub fn new(dir: &Path) -> Host {
        Host {
            dir: dir.to_path_buf(),
            active: None,
            wanted: None,
            retry_at: None,
            last_note: String::new(),
        }
    }

    pub fn port(&self) -> Option<u16> {
        self.active.as_ref().map(|(port, _)| *port)
    }

    pub fn tick(&mut self, now: Instant, log: &dyn Fn(&str)) {
        let config = load_config(&self.dir);
        let want = config.enabled.then_some(config.port);
        if self.wanted != want {
            if let Some((port, receiver)) = self.active.take() {
                drop(receiver);
                clear_host_status(&self.dir, std::process::id());
                log(&format!("otel receiver stopped (was 127.0.0.1:{port})"));
            }
            self.wanted = want;
            self.retry_at = None;
            self.last_note.clear();
        }
        let Some(port) = want else {
            return;
        };
        if self.active.is_some() || self.retry_at.is_some_and(|at| now < at) {
            return;
        }
        match Receiver::start(&self.dir, port) {
            Ok(receiver) => {
                self.active = Some((port, receiver));
                self.last_note.clear();
                save_host_status(
                    &self.dir,
                    &HostStatus {
                        port,
                        pid: std::process::id(),
                        state: "listening".into(),
                        message: String::new(),
                    },
                );
                log(&format!("otel receiver listening on 127.0.0.1:{port}"));
            }
            Err(error) => {
                self.retry_at = Some(now + RETRY);
                self.note_failure(port, &error, log);
            }
        }
    }

    fn note_failure(&mut self, port: u16, error: &StartError, log: &dyn Fn(&str)) {
        let message = match error {
            StartError::InUse { ours: true } => {
                format!("otel receiver port {port} is served by another Toolport process")
            }
            StartError::InUse { ours: false } => {
                let message = format!(
                    "otel receiver cannot start: port {port} is used by another program; pick a free one with `toolportctl obs otel enable --port <n>`"
                );
                save_host_status(
                    &self.dir,
                    &HostStatus {
                        port,
                        pid: std::process::id(),
                        state: "error".into(),
                        message: message.clone(),
                    },
                );
                message
            }
            StartError::Failed(why) => {
                let message = format!("otel receiver cannot start on port {port}: {why}");
                save_host_status(
                    &self.dir,
                    &HostStatus {
                        port,
                        pid: std::process::id(),
                        state: "error".into(),
                        message: message.clone(),
                    },
                );
                message
            }
        };
        if message != self.last_note {
            log(&message);
            self.last_note = message;
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if self.active.is_some() {
            clear_host_status(&self.dir, std::process::id());
        }
    }
}

/// Runs the host for the life of the process. A missing data directory is a no-op.
pub fn spawn() {
    let Some(dir) = crate::registry::conduit_dir().map(|d| d.join("obs")) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("otel-receiver-host".into())
        .spawn(move || {
            let mut host = Host::new(&dir);
            loop {
                host.tick(Instant::now(), &crate::gatewaylog::append);
                std::thread::sleep(POLL);
            }
        });
}
