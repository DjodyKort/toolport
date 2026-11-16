use std::collections::BTreeMap;

use toolport_plus_api::Host;

use super::types::ProbeOutcome;

pub const GOOGLE_REFRESH_MIN_INTERVAL_SECS: i64 = 6 * 60 * 60;
pub const STDIO_MIN_INTERVAL_SECS: i64 = 30 * 60;
pub const BACKOFF_CAP_SECS: i64 = 6 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    GoogleRefresh,
    Stdio,
}

impl ProbeKind {
    pub fn default_min_interval(self) -> i64 {
        match self {
            ProbeKind::GoogleRefresh => GOOGLE_REFRESH_MIN_INTERVAL_SECS,
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

    /// Servers of one Google account share a refresh token, so one probe per profile suffices.
    pub fn profile_gate_key(&self) -> Option<String> {
        match (&self.kind, &self.profile) {
            (ProbeKind::GoogleRefresh, Some(profile)) => Some(format!("google:{profile}")),
            _ => None,
        }
    }
}

pub trait Probe {
    fn run(&self, host: &dyn Host, spec: &ProbeSpec) -> ProbeOutcome;
}

#[derive(Debug, Clone, Default)]
pub struct ProbeRegistry {
    specs: BTreeMap<String, ProbeSpec>,
}

impl ProbeRegistry {
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
