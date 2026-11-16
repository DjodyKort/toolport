//! Refresh-token probe for the Google Workspace servers. It reads the server's own token file,
//! trades the refresh token for an access token and classifies the answer. The request carries
//! the token, so it never follows a redirect.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde_json::Value;
use toolport_plus_api::{Host, HttpBody, HttpRequest, ServerRecord};

use super::probe::{Probe, ProbeKind, ProbeRegistry, ProbeSpec};
use super::types::ProbeOutcome;

pub const DEFAULT_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY_BYTES: usize = 64 * 1024;
const VAULT_CLIENT_ID: &str = "GOOGLE_CLIENT_ID";
const VAULT_CLIENT_SECRET: &str = "GOOGLE_CLIENT_SECRET";
const PROFILE_ENV: &str = "GOOGLE_MCP_PROFILE";

pub struct GoogleRefreshProbe {
    endpoint: String,
    timeout: Duration,
    seen_mtime: Mutex<BTreeMap<PathBuf, SystemTime>>,
}

impl Default for GoogleRefreshProbe {
    fn default() -> Self {
        GoogleRefreshProbe {
            endpoint: DEFAULT_TOKEN_ENDPOINT.to_string(),
            timeout: DEFAULT_TIMEOUT,
            seen_mtime: Mutex::new(BTreeMap::new()),
        }
    }
}

/// https, or plain http to a loopback address for a local test listener.
pub fn endpoint_allowed(endpoint: &str) -> bool {
    let Some((scheme, rest)) = endpoint.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = host.rsplit_once(':').map_or(host, |(name, _)| name);
    match scheme {
        "https" => !host.is_empty() && !authority.contains('@'),
        "http" => host == "127.0.0.1",
        _ => false,
    }
}

pub fn valid_profile(profile: &str) -> bool {
    !profile.is_empty()
        && profile
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn token_path(base: &Path, profile: &str) -> Option<PathBuf> {
    if profile == "default" {
        return Some(base.join("token.json"));
    }
    valid_profile(profile).then(|| base.join(profile).join("token.json"))
}

fn default_config_dir(host: &dyn Host) -> Option<PathBuf> {
    let base = host
        .env_var("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            host.env_var("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(base.join("google-docs-mcp"))
}

fn oauth_failure(code: &str, description: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.to_string(),
        description: description.to_string(),
    }
}

/// An error code from a response body is shown on screen, so only a plain lowercase code passes.
fn sanitize_code(code: &str) -> String {
    let ok = !code.is_empty()
        && code.len() <= 40
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        code.to_string()
    } else {
        "unknown_error".to_string()
    }
}

fn sanitize_description(description: &str) -> &'static str {
    let lower = description.to_ascii_lowercase();
    if lower.contains("revoked") {
        "revoked"
    } else if lower.contains("expired") {
        "expired"
    } else {
        ""
    }
}

struct Credentials {
    refresh_token: String,
    client_id: Option<String>,
    client_secret: Option<String>,
}

fn non_empty(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn read_credentials(path: &Path) -> Result<(Credentials, Option<SystemTime>), ProbeOutcome> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(oauth_failure("no_token_file", ""))
        }
        Err(_) => return Err(oauth_failure("bad_token_file", "")),
    };
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let parsed: Value =
        serde_json::from_slice(&bytes).map_err(|_| oauth_failure("bad_token_file", ""))?;
    let refresh_token =
        non_empty(&parsed, "refresh_token").ok_or_else(|| oauth_failure("bad_token_file", ""))?;
    Ok((
        Credentials {
            refresh_token,
            client_id: non_empty(&parsed, "client_id"),
            client_secret: non_empty(&parsed, "client_secret"),
        },
        mtime,
    ))
}

impl GoogleRefreshProbe {
    pub fn with_endpoint(endpoint: &str) -> Result<Self, String> {
        if !endpoint_allowed(endpoint) {
            return Err("token endpoint must be https or loopback http".to_string());
        }
        Ok(Self {
            endpoint: endpoint.to_string(),
            ..Self::default()
        })
    }

    fn resolve_path(&self, host: &dyn Host, spec: &ProbeSpec) -> Option<PathBuf> {
        let profile = spec.profile.as_deref().unwrap_or("default");
        token_path(&default_config_dir(host)?, profile)
    }

    fn vault_value(host: &dyn Host, server: &str, key: &str) -> Option<String> {
        host.read_vault_secret(server, key)
            .ok()
            .flatten()
            .filter(|value| !value.is_empty())
    }

    fn exchange(&self, host: &dyn Host, spec: &ProbeSpec, creds: Credentials) -> ProbeOutcome {
        let client_id = creds
            .client_id
            .or_else(|| Self::vault_value(host, &spec.server, VAULT_CLIENT_ID));
        let client_secret = creds
            .client_secret
            .or_else(|| Self::vault_value(host, &spec.server, VAULT_CLIENT_SECRET));
        let (Some(client_id), Some(client_secret)) = (client_id, client_secret) else {
            return oauth_failure("invalid_client", "");
        };
        let request = HttpRequest::post(
            &self.endpoint,
            HttpBody::Form(vec![
                ("grant_type".into(), "refresh_token".into()),
                ("refresh_token".into(), creds.refresh_token),
                ("client_id".into(), client_id),
                ("client_secret".into(), client_secret),
            ]),
        )
        .timeout(self.timeout)
        .max_body_bytes(MAX_BODY_BYTES)
        .no_redirects();
        let Ok(response) = host.send(&request) else {
            return ProbeOutcome::TransportError;
        };
        if response.is_success() {
            return Self::on_success(response.json());
        }
        on_status(response.status, response.json())
    }

    fn on_success(body: Option<Value>) -> ProbeOutcome {
        let Some(body) = body else {
            return ProbeOutcome::TransportError;
        };
        if non_empty(&body, "access_token").is_none() {
            return ProbeOutcome::TransportError;
        }
        ProbeOutcome::Success
    }
}

fn on_status(status: u16, body: Option<Value>) -> ProbeOutcome {
    if !(400..500).contains(&status) {
        return ProbeOutcome::HttpStatus { status };
    }
    let Some(body) = body else {
        return ProbeOutcome::HttpStatus { status };
    };
    match body.get("error").and_then(Value::as_str) {
        Some(code) => {
            let description = body
                .get("error_description")
                .and_then(Value::as_str)
                .unwrap_or("");
            oauth_failure(&sanitize_code(code), sanitize_description(description))
        }
        None => ProbeOutcome::HttpStatus { status },
    }
}

impl Probe for GoogleRefreshProbe {
    fn run(&self, host: &dyn Host, spec: &ProbeSpec) -> ProbeOutcome {
        let Some(path) = self.resolve_path(host, spec) else {
            return oauth_failure("bad_token_file", "");
        };
        let (creds, mtime) = match read_credentials(&path) {
            Ok(read) => read,
            Err(outcome) => return outcome,
        };
        let changed = match (mtime, self.seen_mtime.lock()) {
            (Some(now), Ok(mut seen)) => seen.insert(path, now).is_some_and(|prev| prev != now),
            _ => false,
        };
        if changed {
            return ProbeOutcome::FileChanged;
        }
        self.exchange(host, spec, creds)
    }
}

/// A server is a Google one when it names its profile, or, without a profile variable, when it
/// carries the client id or secret variable of google-docs-mcp, which means the default account.
pub fn google_registry(servers: &[ServerRecord], registry: &mut ProbeRegistry) {
    for server in servers {
        let Ok(env) = server.env() else { continue };
        let profile_env = env.iter().find(|entry| entry.key == PROFILE_ENV);
        let profile = profile_env
            .filter(|entry| !entry.secret)
            .and_then(|entry| entry.value.as_deref())
            .filter(|profile| valid_profile(profile));
        let is_default_account = profile_env.is_none()
            && env
                .iter()
                .any(|entry| entry.key == VAULT_CLIENT_ID || entry.key == VAULT_CLIENT_SECRET);
        if let Some(profile) = profile {
            registry.register(
                ProbeSpec::new(&server.id, ProbeKind::GoogleRefresh).with_profile(profile),
            );
        } else if is_default_account {
            registry.register(
                ProbeSpec::new(&server.id, ProbeKind::GoogleRefresh).with_profile("default"),
            );
        }
    }
}
