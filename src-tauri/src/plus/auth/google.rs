use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::probe::{Probe, ProbeSpec};
use super::types::ProbeOutcome;
use super::{agent, read_capped};

pub const DEFAULT_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const PARAM_CONFIG_DIR: &str = "config_dir";
const VAULT_CLIENT_ID: &str = "GOOGLE_CLIENT_ID";
const VAULT_CLIENT_SECRET: &str = "GOOGLE_CLIENT_SECRET";

pub trait ClientVault: Send + Sync {
    fn get(&self, server: &str, key: &str) -> Option<String>;
}

pub struct SecretsVault;

impl ClientVault for SecretsVault {
    fn get(&self, server: &str, key: &str) -> Option<String> {
        crate::secrets::get_secret(server, key).filter(|v| !v.is_empty())
    }
}

pub struct GoogleRefreshProbe {
    endpoint: String,
    timeout: Duration,
    vault: Box<dyn ClientVault>,
    seen_mtime: Mutex<BTreeMap<PathBuf, SystemTime>>,
    ttl: Mutex<BTreeMap<String, i64>>,
}

impl std::fmt::Debug for GoogleRefreshProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleRefreshProbe")
            .field("endpoint", &self.endpoint)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Default for GoogleRefreshProbe {
    fn default() -> Self {
        GoogleRefreshProbe {
            endpoint: DEFAULT_TOKEN_ENDPOINT.to_string(),
            timeout: DEFAULT_TIMEOUT,
            vault: Box::new(SecretsVault),
            seen_mtime: Mutex::new(BTreeMap::new()),
            ttl: Mutex::new(BTreeMap::new()),
        }
    }
}

pub(super) fn endpoint_allowed(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => matches!(url.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback()),
        _ => false,
    }
}

pub fn default_config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))?;
    Some(base.join("google-docs-mcp"))
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

fn oauth_failure(code: &str, description: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.to_string(),
        description: description.to_string(),
    }
}

pub(super) fn sanitize_code(code: &str) -> String {
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
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn read_credentials(path: &Path) -> Result<(Credentials, Option<SystemTime>), ProbeOutcome> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
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
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn with_endpoint(mut self, endpoint: &str) -> Result<Self, String> {
        if !endpoint_allowed(endpoint) {
            return Err("token endpoint must be https or loopback http".to_string());
        }
        self.endpoint = endpoint.to_string();
        Ok(self)
    }

    #[cfg(test)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[cfg(test)]
    pub fn with_vault(mut self, vault: Box<dyn ClientVault>) -> Self {
        self.vault = vault;
        self
    }

    #[cfg(test)]
    pub fn last_ttl(&self, server: &str) -> Option<i64> {
        self.ttl.lock().ok()?.get(server).copied()
    }

    fn resolve_path(&self, spec: &ProbeSpec) -> Option<PathBuf> {
        let profile = spec.profile.as_deref().unwrap_or("default");
        let base = match spec.params.get(PARAM_CONFIG_DIR) {
            Some(dir) => PathBuf::from(dir),
            None => default_config_dir()?,
        };
        token_path(&base, profile)
    }

    fn exchange(&self, spec: &ProbeSpec, creds: Credentials) -> ProbeOutcome {
        let client_id = creds
            .client_id
            .or_else(|| self.vault.get(&spec.server, VAULT_CLIENT_ID));
        let client_secret = creds
            .client_secret
            .or_else(|| self.vault.get(&spec.server, VAULT_CLIENT_SECRET));
        let (Some(client_id), Some(client_secret)) = (client_id, client_secret) else {
            return oauth_failure("invalid_client", "");
        };
        let result = agent(self.timeout).post(&self.endpoint).send_form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &creds.refresh_token),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
        ]);
        match result {
            Ok(response) => self.on_success(spec, response),
            Err(ureq::Error::Status(status, response)) => on_status(status, response),
            Err(ureq::Error::Transport(_)) => ProbeOutcome::TransportError,
        }
    }

    fn on_success(&self, spec: &ProbeSpec, response: ureq::Response) -> ProbeOutcome {
        let Some(body) = read_json(response) else {
            return ProbeOutcome::TransportError;
        };
        if non_empty(&body, "access_token").is_none() {
            return ProbeOutcome::TransportError;
        }
        if let Some(secs) = body.get("expires_in").and_then(Value::as_i64) {
            if let Ok(mut ttl) = self.ttl.lock() {
                ttl.insert(spec.server.clone(), secs);
            }
        }
        ProbeOutcome::Success
    }
}

fn read_json(response: ureq::Response) -> Option<Value> {
    serde_json::from_slice(&read_capped(response).ok()?).ok()
}

fn on_status(status: u16, response: ureq::Response) -> ProbeOutcome {
    if status >= 500 || !(400..500).contains(&status) {
        return ProbeOutcome::HttpStatus { status };
    }
    let Some(body) = read_json(response) else {
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
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let Some(path) = self.resolve_path(spec) else {
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
        self.exchange(spec, creds)
    }
}

pub fn google_registry(registry: &crate::registry::Registry) -> super::ProbeRegistry {
    let mut reg = super::ProbeRegistry::new();
    for server in &registry.servers {
        let profile_env = server.env.iter().find(|e| e.key == "GOOGLE_MCP_PROFILE");
        let profile = profile_env
            .filter(|e| !e.secret)
            .and_then(|e| e.value.as_deref())
            .filter(|p| valid_profile(p));
        // No GOOGLE_MCP_PROFILE at all (not just unusable) means the default
        // account; google-docs-mcp's own client id/secret env keys identify it
        // as a google server even then.
        let is_default_account = profile_env.is_none()
            && server
                .env
                .iter()
                .any(|e| e.key == VAULT_CLIENT_ID || e.key == VAULT_CLIENT_SECRET);
        if let Some(profile) = profile {
            reg.register(
                ProbeSpec::new(&server.id, super::ProbeKind::GoogleRefresh).with_profile(profile),
            );
        } else if is_default_account {
            reg.register(
                ProbeSpec::new(&server.id, super::ProbeKind::GoogleRefresh)
                    .with_profile("default"),
            );
        }
    }
    reg
}
