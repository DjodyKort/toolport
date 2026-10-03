//! HTTP credential probes (A7): slack, odoo, moodle, stitch, framelink, miro-community.
//!
//! Which probe a registry server gets is decided by [`http_registry`]: an explicit hint
//! `unknownFields.plus.authProbe = {"kind": "<service>"|"none", "tokenKey"?, "baseUrl"?, "db"?,
//! "user"?, "uid"?}` wins; otherwise the service is inferred from the server's env keys
//! (`SLACK_BOT_TOKEN`, `ODOO_URL`+`ODOO_API_KEY`, `MOODLE_URL`+`MOODLE_TOKEN`, `STITCH_API_KEY`,
//! `FIGMA_API_KEY`, `MIRO_ACCESS_TOKEN`). The credential is read only from the vault under
//! `<serverId>::<KEY>`; outcomes carry fixed codes, never tokens or response bodies.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::google::{
    endpoint_allowed, sanitize_code, ClientVault, GoogleRefreshProbe, SecretsVault,
};
use super::probe::{Clock, Probe, ProbeKind, ProbeRegistry, ProbeSpec, SystemClock};
use super::types::ProbeOutcome;
use crate::plus::compression::ledger;

pub const PARAM_SERVICE: &str = "service";
pub const PARAM_TOKEN_KEY: &str = "token_key";
pub const PARAM_BASE_URL: &str = "base_url";
pub const PARAM_DB: &str = "db";
pub const PARAM_USER: &str = "user";
pub const PARAM_UID: &str = "uid";

pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub const ODOO_COOLDOWN_SECS: i64 = 6 * 60 * 60;
const ODOO_MIN_INTERVAL_SECS: i64 = 6 * 60 * 60;
const SLOW_MIN_INTERVAL_SECS: i64 = 30 * 60;
const FAST_MIN_INTERVAL_SECS: i64 = 10 * 60;
const MAX_BODY_BYTES: u64 = 64 * 1024;

const SLACK_BASE: &str = "https://slack.com/api";
const STITCH_BASE: &str = "https://stitch.googleapis.com";
const FIGMA_BASE: &str = "https://api.figma.com";
const MIRO_BASE: &str = "https://api.miro.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Slack,
    Odoo,
    Moodle,
    Stitch,
    Framelink,
    MiroCommunity,
}

impl Service {
    pub fn name(self) -> &'static str {
        match self {
            Service::Slack => "slack",
            Service::Odoo => "odoo",
            Service::Moodle => "moodle",
            Service::Stitch => "stitch",
            Service::Framelink => "framelink",
            Service::MiroCommunity => "miro-community",
        }
    }

    pub fn parse(name: &str) -> Option<Service> {
        [
            Service::Slack,
            Service::Odoo,
            Service::Moodle,
            Service::Stitch,
            Service::Framelink,
            Service::MiroCommunity,
        ]
        .into_iter()
        .find(|s| s.name() == name)
    }

    fn default_token_key(self) -> &'static str {
        match self {
            Service::Slack => "SLACK_BOT_TOKEN",
            Service::Odoo => "ODOO_API_KEY",
            Service::Moodle => "MOODLE_TOKEN",
            Service::Stitch => "STITCH_API_KEY",
            Service::Framelink => "FIGMA_API_KEY",
            Service::MiroCommunity => "MIRO_ACCESS_TOKEN",
        }
    }

    fn min_interval(self) -> i64 {
        match self {
            Service::Odoo => ODOO_MIN_INTERVAL_SECS,
            Service::Slack | Service::Moodle => SLOW_MIN_INTERVAL_SECS,
            Service::Stitch | Service::Framelink | Service::MiroCommunity => FAST_MIN_INTERVAL_SECS,
        }
    }

    fn needs_base_url(self) -> bool {
        matches!(self, Service::Odoo | Service::Moodle)
    }
}

const SLACK_TOKEN_KEYS: [&str; 4] = [
    "SLACK_BOT_TOKEN",
    "SLACK_USER_TOKEN",
    "SLACK_MCP_XOXP_TOKEN",
    "SLACK_MCP_XOXB_TOKEN",
];

fn fail(code: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.to_string(),
        description: String::new(),
    }
}

struct Reply {
    status: u16,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

enum Body<'a> {
    Empty,
    Form(&'a [(&'a str, &'a str)]),
    Json(Value),
}

fn status_outcome(status: u16) -> ProbeOutcome {
    match status {
        401 | 403 => fail("unauthorized"),
        _ => ProbeOutcome::HttpStatus { status },
    }
}

fn base_of(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

pub struct HttpProbe {
    bases: BTreeMap<&'static str, String>,
    timeout: Duration,
    vault: Box<dyn ClientVault>,
    clock: Arc<dyn Clock>,
    odoo: Mutex<OdooState>,
}

#[derive(Default)]
struct OdooState {
    uids: BTreeMap<String, i64>,
    cooldown: BTreeMap<String, Cooldown>,
}

struct Cooldown {
    at: i64,
    fingerprint: u64,
    outcome: ProbeOutcome,
}

impl std::fmt::Debug for HttpProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpProbe")
            .field("bases", &self.bases)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Default for HttpProbe {
    fn default() -> Self {
        HttpProbe {
            bases: BTreeMap::new(),
            timeout: HTTP_TIMEOUT,
            vault: Box::new(SecretsVault),
            clock: Arc::new(SystemClock),
            odoo: Mutex::new(OdooState::default()),
        }
    }
}

impl HttpProbe {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base(mut self, service: Service, url: &str) -> Result<Self, String> {
        if !endpoint_allowed(url) {
            return Err("base url must be https or loopback http".to_string());
        }
        self.bases.insert(service.name(), base_of(url));
        Ok(self)
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_vault(mut self, vault: Box<dyn ClientVault>) -> Self {
        self.vault = vault;
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    fn base(&self, service: Service, spec: &ProbeSpec) -> Result<String, ProbeOutcome> {
        let chosen = if service.needs_base_url() {
            spec.params.get(PARAM_BASE_URL).cloned()
        } else {
            self.bases.get(service.name()).cloned().or_else(|| {
                Some(
                    match service {
                        Service::Slack => SLACK_BASE,
                        Service::Stitch => STITCH_BASE,
                        Service::Framelink => FIGMA_BASE,
                        _ => MIRO_BASE,
                    }
                    .to_string(),
                )
            })
        };
        match chosen {
            Some(url) if endpoint_allowed(&url) => Ok(base_of(&url)),
            Some(_) => Err(fail("bad_endpoint")),
            None => Err(fail("missing_config")),
        }
    }

    fn token(&self, service: Service, spec: &ProbeSpec) -> Result<String, ProbeOutcome> {
        let key = spec
            .params
            .get(PARAM_TOKEN_KEY)
            .map(String::as_str)
            .unwrap_or_else(|| service.default_token_key());
        self.vault
            .get(&spec.server, key)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| fail("no_token"))
    }

    fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Body,
    ) -> Result<Reply, ProbeOutcome> {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(self.timeout)
            .build();
        let mut request = agent.request(method, url);
        for (name, value) in headers {
            request = request.set(name, value);
        }
        let result = match body {
            Body::Empty => request.call(),
            Body::Form(pairs) => request.send_form(pairs),
            Body::Json(value) => request.send_json(value),
        };
        let response = match result {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(_)) => return Err(ProbeOutcome::TransportError),
        };
        let status = response.status();
        let mut buf = Vec::new();
        response
            .into_reader()
            .take(MAX_BODY_BYTES)
            .read_to_end(&mut buf)
            .map_err(|_| ProbeOutcome::TransportError)?;
        Ok(Reply { status, body: buf })
    }

    fn slack(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let run = || -> Result<ProbeOutcome, ProbeOutcome> {
            let base = self.base(Service::Slack, spec)?;
            let token = self.token(Service::Slack, spec)?;
            let reply = self.send(
                "POST",
                &format!("{base}/auth.test"),
                &[("Authorization", format!("Bearer {token}"))],
                Body::Empty,
            )?;
            if reply.status == 429 {
                return Ok(ProbeOutcome::HttpStatus { status: 429 });
            }
            let Some(body) = reply.json() else {
                return Ok(non_json(reply.status));
            };
            Ok(match body.get("ok").and_then(Value::as_bool) {
                Some(true) => ProbeOutcome::Success,
                Some(false) => match body.get("error").and_then(Value::as_str) {
                    Some(code) => fail(&sanitize_code(code)),
                    None => ProbeOutcome::TransportError,
                },
                None => ProbeOutcome::TransportError,
            })
        };
        run().unwrap_or_else(|o| o)
    }

    fn moodle(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let run = || -> Result<ProbeOutcome, ProbeOutcome> {
            let base = self.base(Service::Moodle, spec)?;
            let token = self.token(Service::Moodle, spec)?;
            let reply = self.send(
                "POST",
                &format!("{base}/webservice/rest/server.php"),
                &[],
                Body::Form(&[
                    ("wstoken", &token),
                    ("wsfunction", "core_webservice_get_site_info"),
                    ("moodlewsrestformat", "json"),
                ]),
            )?;
            if !(200..300).contains(&reply.status) {
                return Ok(status_outcome(reply.status));
            }
            let Some(body) = reply.json() else {
                return Ok(ProbeOutcome::TransportError);
            };
            if let Some(code) = body.get("errorcode").and_then(Value::as_str) {
                return Ok(fail(&sanitize_code(&code.to_ascii_lowercase())));
            }
            let looks_right = ["sitename", "userid", "username"]
                .iter()
                .any(|k| body.get(k).is_some());
            Ok(if looks_right {
                ProbeOutcome::Success
            } else {
                ProbeOutcome::TransportError
            })
        };
        run().unwrap_or_else(|o| o)
    }

    fn stitch(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let run = || -> Result<ProbeOutcome, ProbeOutcome> {
            let base = self.base(Service::Stitch, spec)?;
            let token = self.token(Service::Stitch, spec)?;
            let reply = self.send(
                "POST",
                &format!("{base}/mcp"),
                &[
                    ("X-Goog-Api-Key", token),
                    ("Accept", "application/json, text/event-stream".to_string()),
                ],
                Body::Json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": {"name": "list_projects", "arguments": {}},
                })),
            )?;
            if !(200..300).contains(&reply.status) {
                return Ok(status_outcome(reply.status));
            }
            let text = String::from_utf8_lossy(&reply.body);
            let trimmed = text.trim_start();
            Ok(match reply.json() {
                Some(body) if body.get("error").is_none() && body.get("result").is_some() => {
                    ProbeOutcome::Success
                }
                None if trimmed.starts_with("event:") || trimmed.starts_with("data:") => {
                    ProbeOutcome::Success
                }
                _ => ProbeOutcome::TransportError,
            })
        };
        run().unwrap_or_else(|o| o)
    }

    fn framelink(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let run = || -> Result<ProbeOutcome, ProbeOutcome> {
            let base = self.base(Service::Framelink, spec)?;
            let token = self.token(Service::Framelink, spec)?;
            let reply = self.send(
                "GET",
                &format!("{base}/v1/me"),
                &[("X-Figma-Token", token)],
                Body::Empty,
            )?;
            if !(200..300).contains(&reply.status) {
                return Ok(status_outcome(reply.status));
            }
            Ok(match reply.json() {
                Some(body) if body.get("id").is_some() => ProbeOutcome::Success,
                _ => ProbeOutcome::TransportError,
            })
        };
        run().unwrap_or_else(|o| o)
    }

    fn miro(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let run = || -> Result<ProbeOutcome, ProbeOutcome> {
            let base = self.base(Service::MiroCommunity, spec)?;
            let token = self.token(Service::MiroCommunity, spec)?;
            let reply = self.send(
                "GET",
                &format!("{base}/v1/oauth-token"),
                &[("Authorization", format!("Bearer {token}"))],
                Body::Empty,
            )?;
            if !(200..300).contains(&reply.status) {
                return Ok(status_outcome(reply.status));
            }
            let Some(body) = reply.json().filter(Value::is_object) else {
                return Ok(ProbeOutcome::TransportError);
            };
            let expires = ["expires_at", "expiresAt"]
                .iter()
                .find_map(|k| body.get(k))
                .and_then(parse_instant);
            Ok(match expires {
                Some(at) => ProbeOutcome::TokenTtl {
                    secs: at - self.clock.now(),
                },
                None => ProbeOutcome::Success,
            })
        };
        run().unwrap_or_else(|o| o)
    }

    fn odoo(&self, spec: &ProbeSpec) -> ProbeOutcome {
        let run = || -> Result<ProbeOutcome, ProbeOutcome> {
            let base = self.base(Service::Odoo, spec)?;
            let key = self.token(Service::Odoo, spec)?;
            let fingerprint = fingerprint(&key);
            let now = self.clock.now();
            if let Some(held) = self.cooled_down(&spec.server, fingerprint, now) {
                return Ok(held);
            }
            let health = self.send("GET", &format!("{base}/web/health"), &[], Body::Empty)?;
            if health.status != 200 {
                return Ok(ProbeOutcome::HttpStatus {
                    status: health.status,
                });
            }
            let db = spec
                .params
                .get(PARAM_DB)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| fail("missing_config"))?;
            let uid = match self.odoo_uid(spec, &base, db, &key)? {
                Some(uid) => uid,
                None => return Ok(self.deny(&spec.server, fingerprint, now)),
            };
            let reply = jsonrpc(
                self,
                &base,
                "object",
                "execute_kw",
                json!([db, uid, key, "res.users", "read", [[uid]], {"fields": ["id"]}]),
            )?;
            Ok(match odoo_result(&reply) {
                OdooReply::Ok(Value::Array(rows)) if !rows.is_empty() => {
                    self.clear_cooldown(&spec.server);
                    ProbeOutcome::Success
                }
                OdooReply::Denied => self.deny(&spec.server, fingerprint, now),
                _ => ProbeOutcome::TransportError,
            })
        };
        run().unwrap_or_else(|o| o)
    }

    fn odoo_uid(
        &self,
        spec: &ProbeSpec,
        base: &str,
        db: &str,
        key: &str,
    ) -> Result<Option<i64>, ProbeOutcome> {
        if let Some(uid) = spec
            .params
            .get(PARAM_UID)
            .and_then(|u| u.parse::<i64>().ok())
        {
            return Ok(Some(uid));
        }
        if let Some(uid) = self
            .odoo
            .lock()
            .ok()
            .and_then(|s| s.uids.get(&spec.server).copied())
        {
            return Ok(Some(uid));
        }
        let user = spec
            .params
            .get(PARAM_USER)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| fail("missing_config"))?;
        let reply = jsonrpc(
            self,
            base,
            "common",
            "authenticate",
            json!([db, user, key, {}]),
        )?;
        match odoo_result(&reply) {
            OdooReply::Ok(Value::Number(n)) if n.as_i64().is_some_and(|u| u > 0) => {
                let uid = n.as_i64().unwrap_or_default();
                if let Ok(mut state) = self.odoo.lock() {
                    state.uids.insert(spec.server.clone(), uid);
                }
                Ok(Some(uid))
            }
            OdooReply::Ok(Value::Bool(false)) | OdooReply::Ok(Value::Null) | OdooReply::Denied => {
                Ok(None)
            }
            _ => Err(ProbeOutcome::TransportError),
        }
    }

    fn cooled_down(&self, server: &str, fingerprint: u64, now: i64) -> Option<ProbeOutcome> {
        let state = self.odoo.lock().ok()?;
        let held = state.cooldown.get(server)?;
        (held.fingerprint == fingerprint && now < held.at + ODOO_COOLDOWN_SECS)
            .then(|| held.outcome.clone())
    }

    fn deny(&self, server: &str, fingerprint: u64, now: i64) -> ProbeOutcome {
        let outcome = fail("access_denied");
        if let Ok(mut state) = self.odoo.lock() {
            state.cooldown.insert(
                server.to_string(),
                Cooldown {
                    at: now,
                    fingerprint,
                    outcome: outcome.clone(),
                },
            );
        }
        outcome
    }

    fn clear_cooldown(&self, server: &str) {
        if let Ok(mut state) = self.odoo.lock() {
            state.cooldown.remove(server);
        }
    }
}

fn non_json(status: u16) -> ProbeOutcome {
    if (200..300).contains(&status) {
        ProbeOutcome::TransportError
    } else {
        status_outcome(status)
    }
}

fn fingerprint(secret: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    secret.hash(&mut hasher);
    hasher.finish()
}

fn jsonrpc(
    probe: &HttpProbe,
    base: &str,
    service: &str,
    method: &str,
    args: Value,
) -> Result<Reply, ProbeOutcome> {
    let reply = probe.send(
        "POST",
        &format!("{base}/jsonrpc"),
        &[],
        Body::Json(json!({
            "jsonrpc": "2.0",
            "method": "call",
            "id": 1,
            "params": {"service": service, "method": method, "args": args},
        })),
    )?;
    if reply.status != 200 {
        return Err(ProbeOutcome::HttpStatus {
            status: reply.status,
        });
    }
    Ok(reply)
}

enum OdooReply {
    Ok(Value),
    Denied,
    Other,
}

fn odoo_result(reply: &Reply) -> OdooReply {
    let Some(body) = reply.json() else {
        return OdooReply::Other;
    };
    if let Some(error) = body.get("error") {
        let name = error
            .pointer("/data/name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let message = error.get("message").and_then(Value::as_str).unwrap_or("");
        let denied =
            name.contains("AccessDenied") || message.to_ascii_lowercase().contains("access denied");
        return if denied {
            OdooReply::Denied
        } else {
            OdooReply::Other
        };
    }
    match body.get("result") {
        Some(result) => OdooReply::Ok(result.clone()),
        None => OdooReply::Other,
    }
}

fn parse_instant(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n
            .as_i64()
            .map(|v| if v > 100_000_000_000 { v / 1000 } else { v }),
        Value::String(s) => parse_rfc3339(s),
        _ => None,
    }
}

fn parse_rfc3339(text: &str) -> Option<i64> {
    let tail = text.get(19..)?;
    let zone = tail.strip_prefix('.').map_or(tail, |frac| {
        frac.trim_start_matches(|c: char| c.is_ascii_digit())
    });
    if zone.is_empty() {
        return None;
    }
    ledger::parse_ts(text).map(|ms| ms.div_euclid(1000))
}

impl Probe for HttpProbe {
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome {
        match spec
            .params
            .get(PARAM_SERVICE)
            .and_then(|s| Service::parse(s))
        {
            Some(Service::Slack) => self.slack(spec),
            Some(Service::Odoo) => self.odoo(spec),
            Some(Service::Moodle) => self.moodle(spec),
            Some(Service::Stitch) => self.stitch(spec),
            Some(Service::Framelink) => self.framelink(spec),
            Some(Service::MiroCommunity) => self.miro(spec),
            None => fail("missing_config"),
        }
    }
}

#[derive(Default)]
pub struct CompositeProbe {
    google: GoogleRefreshProbe,
    http: HttpProbe,
    gateway: super::gateway_state::GatewayStateProbe,
}

impl std::fmt::Debug for CompositeProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeProbe")
            .field("google", &self.google)
            .field("http", &self.http)
            .field("gateway", &self.gateway)
            .finish()
    }
}

impl Probe for CompositeProbe {
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome {
        match spec.kind {
            ProbeKind::GoogleRefresh => self.google.run(spec),
            ProbeKind::Http => self.http.run(spec),
            ProbeKind::GatewayState => self.gateway.run(spec),
        }
    }
}

fn plain_env<'a>(server: &'a crate::registry::ServerEntry, key: &str) -> Option<&'a str> {
    server
        .env
        .iter()
        .find(|e| e.key == key && !e.secret)
        .and_then(|e| e.value.as_deref())
        .filter(|v| !v.is_empty())
}

fn has_env(server: &crate::registry::ServerEntry, key: &str) -> bool {
    server.env.iter().any(|e| e.key == key)
}

fn infer(server: &crate::registry::ServerEntry) -> Option<(Service, String)> {
    if let Some(key) = SLACK_TOKEN_KEYS.iter().find(|k| has_env(server, k)) {
        return Some((Service::Slack, key.to_string()));
    }
    let candidates = [
        (Service::Odoo, "ODOO_API_KEY", Some("ODOO_URL")),
        (Service::Moodle, "MOODLE_TOKEN", Some("MOODLE_URL")),
        (Service::Stitch, "STITCH_API_KEY", None),
        (Service::Framelink, "FIGMA_API_KEY", None),
        (Service::MiroCommunity, "MIRO_ACCESS_TOKEN", None),
    ];
    candidates.into_iter().find_map(|(service, key, needs)| {
        (has_env(server, key) && needs.is_none_or(|n| has_env(server, n)))
            .then(|| (service, key.to_string()))
    })
}

fn hint(server: &crate::registry::ServerEntry) -> Option<&serde_json::Map<String, Value>> {
    server
        .unknown_fields
        .get("plus")?
        .get("authProbe")?
        .as_object()
}

fn hint_str<'a>(hint: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    hint.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
}

pub fn http_registry(registry: &crate::registry::Registry) -> ProbeRegistry {
    let mut reg = ProbeRegistry::new();
    for server in &registry.servers {
        let hint = hint(server);
        let chosen = match hint.and_then(|h| hint_str(h, "kind")) {
            Some("none") => continue,
            Some(kind) => match Service::parse(kind) {
                Some(service) => {
                    let key = hint
                        .and_then(|h| hint_str(h, "tokenKey"))
                        .unwrap_or(service.default_token_key());
                    Some((service, key.to_string()))
                }
                None => None,
            },
            None => infer(server),
        };
        let Some((service, token_key)) = chosen else {
            continue;
        };
        let from_hint = |key: &str| hint.and_then(|h| hint_str(h, key)).map(str::to_string);
        let url_env = match service {
            Service::Odoo => Some("ODOO_URL"),
            Service::Moodle => Some("MOODLE_URL"),
            _ => None,
        };
        let base_url = url_env
            .and_then(|k| plain_env(server, k).map(str::to_string))
            .or_else(|| from_hint("baseUrl").filter(|_| service.needs_base_url()));
        if service.needs_base_url() && base_url.is_none() {
            continue;
        }
        let mut spec = ProbeSpec::new(&server.id, ProbeKind::Http)
            .with_param(PARAM_SERVICE, service.name())
            .with_param(PARAM_TOKEN_KEY, &token_key)
            .with_min_interval(service.min_interval());
        if let Some(url) = base_url {
            spec = spec.with_param(PARAM_BASE_URL, &url);
        }
        if service == Service::Odoo {
            let pairs = [
                (PARAM_DB, "db", "ODOO_DB"),
                (PARAM_USER, "user", "ODOO_USER"),
                (PARAM_UID, "uid", "ODOO_UID"),
            ];
            for (param, hint_key, env_key) in pairs {
                let value =
                    from_hint(hint_key).or_else(|| plain_env(server, env_key).map(str::to_string));
                if let Some(value) = value {
                    spec = spec.with_param(param, &value);
                }
            }
        }
        reg.register(spec);
    }
    reg
}

pub fn combined_registry(registry: &crate::registry::Registry) -> ProbeRegistry {
    let mut reg = super::google::google_registry(registry);
    for spec in http_registry(registry).iter() {
        if reg.get(&spec.server).is_none() {
            reg.register(spec.clone());
        }
    }
    for spec in super::gateway_state::gateway_registry(registry).iter() {
        if reg.get(&spec.server).is_none() {
            reg.register(spec.clone());
        }
    }
    reg
}
