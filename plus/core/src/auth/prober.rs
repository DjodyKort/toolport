//! Probe runs: the one core behind `toolportctl auth probe` and the sign-in follow-up. Probes only
//! read credentials; every outcome lands in the 0600 status file. Probes run one after another:
//! the host handle is not shared between threads.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{json, Value};
use toolport_plus_api::{Host, ServerRecord};

use super::google::{google_registry, GoogleRefreshProbe};
use super::machine::step;
use super::probe::{Probe, ProbeKind, ProbeRegistry, ProbeSpec, BACKOFF_CAP_SECS};
use super::stdio::{stdio_registry, StdioProbe};
use super::store::{AuthStore, EdgeEvent, ProbeHintKind, ServerEntry, StatusFile};
use super::surfaces;
use super::types::Tracked;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Scheduled,
    UserForce,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub server: String,
    pub ran: bool,
    pub skipped: Option<&'static str>,
    pub tracked: Tracked,
    pub next_due_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Due,
    All,
    One(String),
}

impl Selector {
    pub fn name(&self) -> &'static str {
        match self {
            Selector::Due => "due",
            Selector::All => "all",
            Selector::One(_) => "server",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeFailure {
    pub server: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbeRun {
    pub reports: Vec<ProbeReport>,
    pub failures: Vec<ProbeFailure>,
}

impl ProbeRun {
    pub fn probed(&self) -> usize {
        self.reports.iter().filter(|r| r.ran).count()
    }

    pub fn servers(&self) -> Vec<String> {
        let reported = self.reports.iter().map(|r| r.server.clone());
        reported
            .chain(self.failures.iter().map(|f| f.server.clone()))
            .collect()
    }
}

pub fn backoff_delay(min_interval: i64, transient_count: u32) -> i64 {
    let shift = transient_count.saturating_sub(1).min(20);
    min_interval
        .saturating_mul(1i64 << shift)
        .min(BACKOFF_CAP_SECS.max(min_interval))
        .max(min_interval)
}

fn gate(status: &StatusFile, spec: &ProbeSpec, now: i64) -> Option<&'static str> {
    if let Some(key) = spec.profile_gate_key() {
        if let Some(last) = status.profiles.get(&key) {
            if now < last + spec.min_interval_secs {
                return Some("profile_interval");
            }
        }
    }
    match status.servers.get(&spec.server) {
        Some(entry) if now < entry.next_due_at => Some("not_due"),
        _ => None,
    }
}

fn fresh_entry(now: i64) -> ServerEntry {
    ServerEntry {
        tracked: Tracked::unknown(now),
        last_probe_at: None,
        next_due_at: 0,
        hint_kind: ProbeHintKind::OAuth,
        token_key: None,
    }
}

/// The probes a registry holds: Google refresh tokens and the stdio servers that opted in.
pub fn probe_registry(host: &dyn Host, servers: &[ServerRecord]) -> ProbeRegistry {
    let mut registry = ProbeRegistry::default();
    let switched_off = |server: &ServerRecord| {
        server
            .fork
            .plus
            .as_ref()
            .and_then(|plus| plus.auth_probe.as_ref())
            .and_then(|probe| probe.kind.as_deref())
            == Some("none")
    };
    let probed: Vec<ServerRecord> = servers
        .iter()
        .filter(|server| !switched_off(server))
        .cloned()
        .collect();
    google_registry(&probed, &mut registry);
    stdio_registry(host, &probed, &mut registry);
    registry
}

pub fn read_servers(host: &dyn Host) -> Result<Vec<ServerRecord>, String> {
    match host.read_view()? {
        Some(view) => view.servers(),
        None => Ok(Vec::new()),
    }
}

pub fn auth_dir(host: &dyn Host) -> Option<PathBuf> {
    host.data_dir().map(|dir| dir.join("auth"))
}

/// The servers whose login state is shown: those that still have a probe. An unreadable registry
/// must not blank the warnings, so the cached servers stand in.
pub fn visible_servers(host: &dyn Host, status: &StatusFile) -> Vec<String> {
    match read_servers(host) {
        Ok(all) => probe_registry(host, &all)
            .iter()
            .map(|spec| spec.server.clone())
            .collect(),
        Err(_) => status.servers.keys().cloned().collect(),
    }
}

/// The `auth` section of `status` and `where_am_i`; read-only and lock-free.
pub fn status_section(host: &dyn Host) -> serde_json::Value {
    let status = auth_dir(host)
        .map(|dir| AuthStore::new(&dir).read_status())
        .unwrap_or_default();
    surfaces::status_summary(&status, host.now_unix(), &visible_servers(host, &status))
}

pub struct Prober<'a> {
    host: &'a dyn Host,
    store: AuthStore,
    registry: ProbeRegistry,
    google: GoogleRefreshProbe,
    stdio: StdioProbe,
}

impl<'a> Prober<'a> {
    pub fn new(host: &'a dyn Host) -> Result<Self, String> {
        let dir = auth_dir(host).ok_or_else(|| "data directory unavailable".to_string())?;
        let registry = probe_registry(host, &read_servers(host)?);
        Ok(Prober {
            host,
            store: AuthStore::new(&dir),
            registry,
            google: GoogleRefreshProbe::default(),
            stdio: StdioProbe::default(),
        })
    }

    /// Points the Google probe at another token endpoint; only https and loopback http pass.
    pub fn with_google_endpoint(mut self, endpoint: &str) -> Result<Self, String> {
        self.google = GoogleRefreshProbe::with_endpoint(endpoint)?;
        Ok(self)
    }

    pub fn with_stdio_wait(mut self, wait: std::time::Duration) -> Self {
        self.stdio = StdioProbe { wait };
        self
    }

    pub fn has_probe(&self, server: &str) -> bool {
        self.registry.get(server).is_some()
    }

    pub fn registered(&self) -> Vec<String> {
        let mut claimed = BTreeSet::new();
        self.registry
            .iter()
            .filter(|spec| {
                spec.profile_gate_key()
                    .is_none_or(|key| claimed.insert(key))
            })
            .map(|spec| spec.server.clone())
            .collect()
    }

    pub fn probed_servers(&self) -> Vec<String> {
        self.registry
            .iter()
            .map(|spec| spec.server.clone())
            .collect()
    }

    /// A cache entry outlives its server unless someone removes it: this is that someone. It
    /// touches nothing on disk when there is no cache yet or nothing in it is stale.
    pub fn prune_unregistered(&self) -> Result<bool, String> {
        if !self.store.status_path().exists() {
            return Ok(false);
        }
        let guard = self.store.lock()?;
        let mut status = guard.load_status();
        if !status.prune(&self.registry) {
            return Ok(false);
        }
        guard.save_status(&status)?;
        Ok(true)
    }

    fn due(&self) -> Vec<String> {
        if self.registry.iter().next().is_none() {
            return Vec::new();
        }
        let status = self.store.read_status();
        let now = self.host.now_unix();
        let mut claimed = BTreeSet::new();
        let mut due = Vec::new();
        for spec in self.registry.iter() {
            if gate(&status, spec, now).is_some() {
                continue;
            }
            if let Some(key) = spec.profile_gate_key() {
                if !claimed.insert(key) {
                    continue;
                }
            }
            due.push(spec.server.clone());
        }
        due
    }

    fn run_probe(&self, spec: &ProbeSpec) -> super::types::ProbeOutcome {
        match spec.kind {
            ProbeKind::GoogleRefresh => self.google.run(self.host, spec),
            ProbeKind::Stdio => self.stdio.run(self.host, spec),
        }
    }

    pub fn request(&self, server: &str, trigger: Trigger) -> Result<ProbeReport, String> {
        let spec = self
            .registry
            .get(server)
            .ok_or_else(|| format!("no probe registered for {server}"))?;
        self.execute(spec, trigger)
    }

    fn execute(&self, spec: &ProbeSpec, trigger: Trigger) -> Result<ProbeReport, String> {
        let now = self.host.now_unix();
        {
            let guard = self.store.lock()?;
            let mut status = guard.load_status();
            if trigger == Trigger::Scheduled {
                if let Some(reason) = gate(&status, spec, now) {
                    let entry = status
                        .servers
                        .get(&spec.server)
                        .cloned()
                        .unwrap_or_else(|| fresh_entry(now));
                    return Ok(ProbeReport {
                        server: spec.server.clone(),
                        ran: false,
                        skipped: Some(reason),
                        tracked: entry.tracked,
                        next_due_at: entry.next_due_at,
                    });
                }
            }
            let entry = status
                .servers
                .entry(spec.server.clone())
                .or_insert_with(|| fresh_entry(now));
            entry.last_probe_at = Some(now);
            entry.next_due_at = now + spec.min_interval_secs;
            if let Some(key) = spec.profile_gate_key() {
                status.profiles.insert(key, now);
            }
            guard.save_status(&status)?;
        }

        let outcome = self.run_probe(spec);

        let finished = self.host.now_unix();
        let guard = self.store.lock()?;
        let mut status = guard.load_status();
        let prev = status
            .servers
            .get(&spec.server)
            .map(|entry| entry.tracked.clone())
            .unwrap_or_else(|| Tracked::unknown(finished));
        let next = step(&prev, &outcome, finished);
        let delay = match next.transient {
            Some(run) => backoff_delay(spec.min_interval_secs, run.count),
            None => spec.min_interval_secs,
        };
        let entry = ServerEntry {
            tracked: next.clone(),
            last_probe_at: Some(finished),
            next_due_at: finished + delay,
            hint_kind: ProbeHintKind::OAuth,
            token_key: None,
        };
        status.servers.insert(spec.server.clone(), entry.clone());
        guard.save_status(&status)?;
        if prev.state.name() != next.state.name() {
            guard.append_events(&[EdgeEvent {
                ts: finished,
                server: spec.server.clone(),
                from: prev.state.name().to_string(),
                to: next.state.name().to_string(),
                reason: next.reason.clone(),
            }])?;
        }
        Ok(ProbeReport {
            server: spec.server.clone(),
            ran: true,
            skipped: None,
            tracked: next,
            next_due_at: entry.next_due_at,
        })
    }

    /// `Due` runs what the file says is due, `All` forces every registered probe, `One` is a
    /// single request that fails as a whole. A registry without probes touches nothing on disk.
    pub fn run(&self, selector: &Selector, force: bool) -> Result<ProbeRun, String> {
        if force || *selector == Selector::All {
            return self.run_probes(selector, force);
        }
        let mut result = Ok(ProbeRun::default());
        self.host
            .unattended(&mut || result = self.run_probes(selector, force));
        result
    }

    fn run_probes(&self, selector: &Selector, force: bool) -> Result<ProbeRun, String> {
        self.prune_unregistered()?;
        let trigger = if force || *selector == Selector::All {
            Trigger::UserForce
        } else {
            Trigger::Scheduled
        };
        let targets = match selector {
            Selector::One(server) => {
                return Ok(ProbeRun {
                    reports: vec![self.request(server, trigger)?],
                    failures: Vec::new(),
                })
            }
            Selector::Due => self.due(),
            Selector::All => self.registered(),
        };
        let mut run = ProbeRun::default();
        for server in targets {
            match self.request(&server, trigger) {
                Ok(report) => run.reports.push(report),
                Err(error) => run.failures.push(ProbeFailure { server, error }),
            }
        }
        Ok(run)
    }

    pub fn status(&self) -> StatusFile {
        self.store.read_status()
    }
}

pub fn report_value(run: &ProbeRun, mode: &str, status: &StatusFile, now: i64) -> Value {
    let rows = surfaces::rows_for(status, now, &run.servers());
    json!({
        "mode": mode,
        "probes": run.reports,
        "failures": run.failures,
        "counts": surfaces::counts(&rows),
        "servers": rows,
    })
}
