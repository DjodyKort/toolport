use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::atomic::{AtomicI64, Ordering};
#[cfg(test)]
use std::sync::Mutex;

use super::types::ProbeOutcome;

pub const GOOGLE_REFRESH_MIN_INTERVAL_SECS: i64 = 6 * 60 * 60;
pub const HTTP_MIN_INTERVAL_SECS: i64 = 10 * 60;
pub const STDIO_MIN_INTERVAL_SECS: i64 = 30 * 60;
pub const BACKOFF_CAP_SECS: i64 = 6 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    GoogleRefresh,
    Http,
    GatewayState,
    Stdio,
}

impl ProbeKind {
    pub fn default_min_interval(self) -> i64 {
        match self {
            ProbeKind::GoogleRefresh => GOOGLE_REFRESH_MIN_INTERVAL_SECS,
            ProbeKind::Http | ProbeKind::GatewayState => HTTP_MIN_INTERVAL_SECS,
            ProbeKind::Stdio => STDIO_MIN_INTERVAL_SECS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSpec {
    pub server: String,
    pub kind: ProbeKind,
    pub profile: Option<String>,
    pub params: BTreeMap<String, String>,
    pub min_interval_secs: i64,
}

impl ProbeSpec {
    pub fn new(server: &str, kind: ProbeKind) -> Self {
        ProbeSpec {
            server: server.to_string(),
            kind,
            profile: None,
            params: BTreeMap::new(),
            min_interval_secs: kind.default_min_interval(),
        }
    }

    pub fn with_profile(mut self, profile: &str) -> Self {
        self.profile = Some(profile.to_string());
        self
    }

    pub fn with_param(mut self, key: &str, value: &str) -> Self {
        self.params.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_min_interval(mut self, secs: i64) -> Self {
        self.min_interval_secs = secs;
        self
    }

    pub fn profile_gate_key(&self) -> Option<String> {
        match (&self.kind, &self.profile) {
            (ProbeKind::GoogleRefresh, Some(profile)) => Some(format!("google:{profile}")),
            _ => None,
        }
    }
}

pub trait Probe: Send + Sync {
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        crate::plus::skills::clock::now_unix_secs()
    }
}

#[cfg(test)]
pub struct FakeClock(AtomicI64);

#[cfg(test)]
impl FakeClock {
    pub fn new(now: i64) -> Self {
        FakeClock(AtomicI64::new(now))
    }

    pub fn advance(&self, secs: i64) {
        self.0.fetch_add(secs, Ordering::SeqCst);
    }
}

#[cfg(test)]
impl Clock for FakeClock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProbeRegistry {
    specs: BTreeMap<String, ProbeSpec>,
}

impl ProbeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, spec: ProbeSpec) {
        self.specs.insert(spec.server.clone(), spec);
    }

    pub fn get(&self, server: &str) -> Option<&ProbeSpec> {
        self.specs.get(server)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ProbeSpec> {
        self.specs.values()
    }
}

#[cfg(test)]
pub struct MockProbe {
    script: Mutex<Vec<ProbeOutcome>>,
    fallback: ProbeOutcome,
    calls: Mutex<Vec<String>>,
    on_run: Option<Box<dyn Fn() + Send + Sync>>,
}

#[cfg(test)]
impl MockProbe {
    pub fn always(outcome: ProbeOutcome) -> Self {
        MockProbe {
            script: Mutex::new(Vec::new()),
            fallback: outcome,
            calls: Mutex::new(Vec::new()),
            on_run: None,
        }
    }

    pub fn scripted(mut outcomes: Vec<ProbeOutcome>) -> Self {
        outcomes.reverse();
        let mut probe = MockProbe::always(ProbeOutcome::Success);
        probe.script = Mutex::new(outcomes);
        probe
    }

    pub fn on_run(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_run = Some(Box::new(hook));
        self
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    pub fn called_servers(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl Probe for MockProbe {
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome {
        self.calls.lock().unwrap().push(spec.server.clone());
        if let Some(hook) = &self.on_run {
            hook();
        }
        self.script
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| self.fallback.clone())
    }
}
