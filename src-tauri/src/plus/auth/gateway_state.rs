use std::sync::Arc;

use serde::Deserialize;

use super::google::sanitize_code;
use super::probe::{Clock, Probe, ProbeKind, ProbeRegistry, ProbeSpec, SystemClock};
use super::types::ProbeOutcome;

const REFRESH_WINDOW_SECS: i64 = 60;

#[derive(Deserialize)]
struct VaultedState {
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
}

pub struct GatewayStateProbe {
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for GatewayStateProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayStateProbe").finish_non_exhaustive()
    }
}

impl Default for GatewayStateProbe {
    fn default() -> Self {
        GatewayStateProbe {
            clock: Arc::new(SystemClock),
        }
    }
}

fn fail(code: &str, description: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.to_string(),
        description: description.to_string(),
    }
}

fn read_state(server: &str) -> Result<Option<VaultedState>, ProbeOutcome> {
    let blob = crate::secrets::get_secret_result(server, crate::remote::OAUTH_STATE_KEY)
        .map_err(|_| ProbeOutcome::TransportError)?;
    match blob {
        None => Ok(None),
        Some(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|_| fail("bad_token_file", "")),
    }
}

fn refresh_failure(error: &str) -> ProbeOutcome {
    let lower = error.to_ascii_lowercase();
    let code = ["invalid_grant", "invalid_client", "unauthorized_client"]
        .into_iter()
        .find(|code| lower.contains(code));
    match code {
        Some(code) => fail(
            &sanitize_code(code),
            if lower.contains("revoked") {
                "revoked"
            } else {
                ""
            },
        ),
        None if lower.contains("needs authentication") => fail("invalid_auth", ""),
        None => ProbeOutcome::TransportError,
    }
}

impl GatewayStateProbe {
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    fn ttl(&self, expires_at: u64) -> i64 {
        i64::try_from(expires_at)
            .unwrap_or(i64::MAX)
            .saturating_sub(self.clock.now())
    }
}

impl Probe for GatewayStateProbe {
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let server = spec.server.as_str();
        match crate::secrets::get_secret_result(server, crate::secrets::HTTP_AUTH_KEY) {
            Ok(Some(token)) if !token.is_empty() => {}
            Ok(_) => return fail("no_token", ""),
            Err(_) => return ProbeOutcome::TransportError,
        }
        let state = match read_state(server) {
            Ok(Some(state)) => state,
            Ok(None) => return ProbeOutcome::Success,
            Err(outcome) => return outcome,
        };
        let Some(expires_at) = state.expires_at else {
            return ProbeOutcome::Success;
        };
        let ttl = self.ttl(expires_at);
        if ttl > REFRESH_WINDOW_SECS || state.refresh_token.is_none() {
            return ProbeOutcome::TokenTtl { secs: ttl };
        }
        if let Err(error) = crate::remote::refresh_token(server) {
            return refresh_failure(&error);
        }
        match read_state(server) {
            Ok(Some(VaultedState {
                expires_at: Some(fresh),
                ..
            })) => ProbeOutcome::TokenTtl {
                secs: self.ttl(fresh),
            },
            Ok(_) => ProbeOutcome::Success,
            Err(outcome) => outcome,
        }
    }
}

pub fn gateway_registry(registry: &crate::registry::Registry) -> ProbeRegistry {
    let mut reg = ProbeRegistry::new();
    for server in &registry.servers {
        let remote = matches!(server.transport.as_str(), "http" | "sse");
        if remote && server.url.is_some() && server.client_credentials.is_none() {
            reg.register(ProbeSpec::new(&server.id, ProbeKind::GatewayState));
        }
    }
    reg
}
