use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

use super::cache::{AuthStore, EdgeEvent, ServerEntry, StatusFile};
use super::flight::SingleFlight;
use super::issues::compute_issues;
use super::machine::step;
use super::probe::{Clock, Probe, ProbeRegistry, ProbeSpec, BACKOFF_CAP_SECS};
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

pub fn backoff_delay(min_interval: i64, transient_count: u32) -> i64 {
    let shift = transient_count.saturating_sub(1).min(20);
    let delay = min_interval.saturating_mul(1i64 << shift);
    delay
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

pub fn probe_due(status: &StatusFile, registry: &ProbeRegistry, now: i64) -> Vec<String> {
    let mut claimed = std::collections::BTreeSet::new();
    let mut due = Vec::new();
    for spec in registry.iter() {
        if gate(status, spec, now).is_some() {
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

pub fn probe_all(registry: &ProbeRegistry) -> Vec<String> {
    let mut claimed = std::collections::BTreeSet::new();
    registry
        .iter()
        .filter(|spec| {
            spec.profile_gate_key()
                .is_none_or(|key| claimed.insert(key))
        })
        .map(|spec| spec.server.clone())
        .collect()
}

pub struct AuthProber {
    store: AuthStore,
    registry: ProbeRegistry,
    probe: Arc<dyn Probe>,
    clock: Arc<dyn Clock>,
    flight: SingleFlight<Result<ProbeReport, String>>,
}

impl AuthProber {
    pub fn new(
        store: AuthStore,
        registry: ProbeRegistry,
        probe: Arc<dyn Probe>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        AuthProber {
            store,
            registry,
            probe,
            clock,
            flight: SingleFlight::new(),
        }
    }

    pub fn store(&self) -> &AuthStore {
        &self.store
    }

    pub fn waiters(&self, server: &str) -> usize {
        self.flight.waiters(server)
    }

    pub fn has_probe(&self, server: &str) -> bool {
        self.registry.get(server).is_some()
    }

    pub fn registered(&self) -> Vec<String> {
        probe_all(&self.registry)
    }

    pub fn due(&self) -> Result<Vec<String>, String> {
        if self.registry.iter().next().is_none() {
            return Ok(Vec::new());
        }
        let status = self.store.lock()?.load_status();
        Ok(probe_due(&status, &self.registry, self.clock.now()))
    }

    pub fn request(&self, server: &str, trigger: Trigger) -> Result<ProbeReport, String> {
        let spec = self
            .registry
            .get(server)
            .ok_or_else(|| format!("no probe registered for {server}"))?;
        self.flight.run(server, || self.execute(spec, trigger))
    }

    fn execute(&self, spec: &ProbeSpec, trigger: Trigger) -> Result<ProbeReport, String> {
        let now = self.clock.now();
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

        let outcome = self.probe.run(spec);

        let finished = self.clock.now();
        let guard = self.store.lock()?;
        let mut status = guard.load_status();
        let prev = status
            .servers
            .get(&spec.server)
            .map(|e| e.tracked.clone())
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
}

fn fresh_entry(now: i64) -> ServerEntry {
    ServerEntry {
        tracked: Tracked::unknown(now),
        last_probe_at: None,
        next_due_at: 0,
    }
}

pub fn status_value(dir: &Path) -> Result<Value, String> {
    let status = AuthStore::new(dir).lock()?.load_status();
    let issues = compute_issues(&status.tracked_map());
    Ok(json!({
        "issues": issues,
        "servers": status.servers,
    }))
}

pub fn status_handler(_args: Value) -> Result<Value, String> {
    let dir = crate::registry::conduit_dir()
        .ok_or_else(|| "data directory unavailable".to_string())?
        .join("auth");
    status_value(&dir)
}
