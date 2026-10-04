//! HTTP credential probes (A7): slack, odoo, moodle, stitch, framelink, miro-community.
//!
//! Which probe a registry server gets is decided by [`http_registry`]: an explicit hint
//! `unknownFields.plus.authProbe = {"kind": "<service>"|"none", "tokenKey"?, "baseUrl"?, "db"?,
//! "user"?, "uid"?}` wins; otherwise the service is inferred from the server's env keys
//! (`SLACK_BOT_TOKEN`, `ODOO_URL`+`ODOO_API_KEY`, `MOODLE_URL`+`MOODLE_TOKEN`, `STITCH_API_KEY`,
//! `FIGMA_API_KEY`, `MIRO_ACCESS_TOKEN`). The credential is read only from the vault under
//! `<serverId>::<KEY>`; outcomes carry fixed codes, never tokens or response bodies.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::google::{
    endpoint_allowed, sanitize_code, ClientVault, GoogleRefreshProbe, SecretsVault,
};
use super::probe::{Clock, Probe, ProbeKind, ProbeRegistry, ProbeSpec, SystemClock};
use super::types::ProbeOutcome;
use super::{agent, read_capped};
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

enum BaseUrl {
    Fixed(&'static str),
    Config { url_env: &'static str },
}

type RunFn = fn(&HttpProbe, &ProbeSpec) -> Probed;

struct ServiceDef {
    service: Service,
    name: &'static str,
    token_keys: &'static [&'static str],
    base: BaseUrl,
    min_interval: i64,
    params: &'static [(&'static str, &'static str, &'static str)],
    run: RunFn,
}

impl ServiceDef {
    fn default_token_key(&self) -> &'static str {
        self.token_keys[0]
    }

    fn url_env(&self) -> Option<&'static str> {
        match self.base {
            BaseUrl::Config { url_env } => Some(url_env),
            BaseUrl::Fixed(_) => None,
        }
    }
}

const SLACK_TOKEN_KEYS: [&str; 4] = [
    "SLACK_BOT_TOKEN",
    "SLACK_USER_TOKEN",
    "SLACK_MCP_XOXP_TOKEN",
    "SLACK_MCP_XOXB_TOKEN",
];

const ODOO_PARAMS: [(&str, &str, &str); 3] = [
    (PARAM_DB, "db", "ODOO_DB"),
    (PARAM_USER, "user", "ODOO_USER"),
    (PARAM_UID, "uid", "ODOO_UID"),
];

/// In the order `infer` tries them, which is also the order of the `Service` variants.
static SERVICES: [ServiceDef; 6] = [
    ServiceDef {
        service: Service::Slack,
        name: "slack",
        token_keys: &SLACK_TOKEN_KEYS,
        base: BaseUrl::Fixed(SLACK_BASE),
        min_interval: SLOW_MIN_INTERVAL_SECS,
        params: &[],
        run: HttpProbe::slack,
    },
    ServiceDef {
        service: Service::Odoo,
        name: "odoo",
        token_keys: &["ODOO_API_KEY"],
        base: BaseUrl::Config {
            url_env: "ODOO_URL",
        },
        min_interval: ODOO_MIN_INTERVAL_SECS,
        params: &ODOO_PARAMS,
        run: HttpProbe::odoo,
    },
    ServiceDef {
        service: Service::Moodle,
        name: "moodle",
        token_keys: &["MOODLE_TOKEN"],
        base: BaseUrl::Config {
            url_env: "MOODLE_URL",
        },
        min_interval: SLOW_MIN_INTERVAL_SECS,
        params: &[],
        run: HttpProbe::moodle,
    },
    ServiceDef {
        service: Service::Stitch,
        name: "stitch",
        token_keys: &["STITCH_API_KEY"],
        base: BaseUrl::Fixed(STITCH_BASE),
        min_interval: FAST_MIN_INTERVAL_SECS,
        params: &[],
        run: HttpProbe::stitch,
    },
    ServiceDef {
        service: Service::Framelink,
        name: "framelink",
        token_keys: &["FIGMA_API_KEY"],
        base: BaseUrl::Fixed(FIGMA_BASE),
        min_interval: FAST_MIN_INTERVAL_SECS,
        params: &[],
        run: HttpProbe::framelink,
    },
    ServiceDef {
        service: Service::MiroCommunity,
        name: "miro-community",
        token_keys: &["MIRO_ACCESS_TOKEN"],
        base: BaseUrl::Fixed(MIRO_BASE),
        min_interval: FAST_MIN_INTERVAL_SECS,
        params: &[],
        run: HttpProbe::miro,
    },
];

impl Service {
    fn def(self) -> &'static ServiceDef {
        &SERVICES[self as usize]
    }

    pub fn name(self) -> &'static str {
        self.def().name
    }

    pub fn parse(name: &str) -> Option<Service> {
        SERVICES.iter().find(|d| d.name == name).map(|d| d.service)
    }
}

type Probed = Result<ProbeOutcome, ProbeOutcome>;

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

    fn ok_status(&self) -> Result<(), ProbeOutcome> {
        if (200..300).contains(&self.status) {
            Ok(())
        } else {
            Err(status_outcome(self.status))
        }
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
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn with_base(mut self, service: Service, url: &str) -> Result<Self, String> {
        if !endpoint_allowed(url) {
            return Err("base url must be https or loopback http".to_string());
        }
        self.bases.insert(service.name(), base_of(url));
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
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    fn base(&self, service: Service, spec: &ProbeSpec) -> Result<String, ProbeOutcome> {
        let chosen = match service.def().base {
            BaseUrl::Config { .. } => spec.params.get(PARAM_BASE_URL).cloned(),
            BaseUrl::Fixed(default) => Some(
                self.bases
                    .get(service.name())
                    .cloned()
                    .unwrap_or_else(|| default.to_string()),
            ),
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
            .unwrap_or_else(|| service.def().default_token_key());
        self.vault
            .get(&spec.server, key)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| fail("no_token"))
    }

    fn creds(
        &self,
        service: Service,
        spec: &ProbeSpec,
    ) -> Result<(String, String), ProbeOutcome> {
        Ok((self.base(service, spec)?, self.token(service, spec)?))
    }

    fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Body,
    ) -> Result<Reply, ProbeOutcome> {
        let mut request = agent(self.timeout).request(method, url);
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
        let body = read_capped(response).map_err(|_| ProbeOutcome::TransportError)?;
        Ok(Reply { status, body })
    }

    fn slack(&self, spec: &ProbeSpec) -> Probed {
        let (base, token) = self.creds(Service::Slack, spec)?;
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
    }

    fn moodle(&self, spec: &ProbeSpec) -> Probed {
        let (base, token) = self.creds(Service::Moodle, spec)?;
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
        reply.ok_status()?;
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
    }

    fn stitch(&self, spec: &ProbeSpec) -> Probed {
        let (base, token) = self.creds(Service::Stitch, spec)?;
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
        reply.ok_status()?;
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
    }

    fn framelink(&self, spec: &ProbeSpec) -> Probed {
        let (base, token) = self.creds(Service::Framelink, spec)?;
        let reply = self.send(
            "GET",
            &format!("{base}/v1/me"),
            &[("X-Figma-Token", token)],
            Body::Empty,
        )?;
        reply.ok_status()?;
        Ok(match reply.json() {
            Some(body) if body.get("id").is_some() => ProbeOutcome::Success,
            _ => ProbeOutcome::TransportError,
        })
    }

    fn miro(&self, spec: &ProbeSpec) -> Probed {
        let (base, token) = self.creds(Service::MiroCommunity, spec)?;
        let reply = self.send(
            "GET",
            &format!("{base}/v1/oauth-token"),
            &[("Authorization", format!("Bearer {token}"))],
            Body::Empty,
        )?;
        reply.ok_status()?;
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
    }

    fn odoo(&self, spec: &ProbeSpec) -> Probed {
        let (base, key) = self.creds(Service::Odoo, spec)?;
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
        let probed = match spec
            .params
            .get(PARAM_SERVICE)
            .and_then(|s| Service::parse(s))
        {
            Some(service) => (service.def().run)(self, spec),
            None => Err(fail("missing_config")),
        };
        probed.unwrap_or_else(|outcome| outcome)
    }
}

#[derive(Debug, Default)]
pub struct CompositeProbe {
    google: GoogleRefreshProbe,
    http: HttpProbe,
    gateway: super::gateway_state::GatewayStateProbe,
    stdio: super::stdio::StdioProbe,
}

impl Probe for CompositeProbe {
    fn run(&self, spec: &ProbeSpec) -> ProbeOutcome {
        match spec.kind {
            ProbeKind::GoogleRefresh => self.google.run(spec),
            ProbeKind::Http => self.http.run(spec),
            ProbeKind::GatewayState => self.gateway.run(spec),
            ProbeKind::Stdio => self.stdio.run(spec),
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
    SERVICES.iter().find_map(|def| {
        let key = def.token_keys.iter().find(|k| has_env(server, k))?;
        def.url_env()
            .is_none_or(|env| has_env(server, env))
            .then(|| (def.service, key.to_string()))
    })
}

pub(super) fn hint(
    server: &crate::registry::ServerEntry,
) -> Option<&serde_json::Map<String, Value>> {
    crate::plus::tags::object(&server.unknown_fields, &["plus", "authProbe"])
}

pub(super) fn hint_str<'a>(hint: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
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
                        .unwrap_or(service.def().default_token_key());
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
        let def = service.def();
        let base_url = def.url_env().and_then(|env| {
            plain_env(server, env)
                .map(str::to_string)
                .or_else(|| from_hint("baseUrl"))
        });
        if def.url_env().is_some() && base_url.is_none() {
            continue;
        }
        let mut spec = ProbeSpec::new(&server.id, ProbeKind::Http)
            .with_param(PARAM_SERVICE, def.name)
            .with_param(PARAM_TOKEN_KEY, &token_key)
            .with_min_interval(def.min_interval);
        if let Some(url) = base_url {
            spec = spec.with_param(PARAM_BASE_URL, &url);
        }
        for &(param, hint_key, env_key) in def.params {
            let value =
                from_hint(hint_key).or_else(|| plain_env(server, env_key).map(str::to_string));
            if let Some(value) = value {
                spec = spec.with_param(param, &value);
            }
        }
        reg.register(spec);
    }
    reg
}

pub fn combined_registry(registry: &crate::registry::Registry) -> ProbeRegistry {
    let mut reg = super::stdio::stdio_registry(registry);
    merge_missing(&mut reg, &super::google::google_registry(registry));
    merge_missing(&mut reg, &http_registry(registry));
    merge_missing(&mut reg, &super::gateway_state::gateway_registry(registry));
    reg
}

fn merge_missing(reg: &mut ProbeRegistry, extra: &ProbeRegistry) {
    for spec in extra.iter() {
        if reg.get(&spec.server).is_none() {
            reg.register(spec.clone());
        }
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;
    use serde_json::json;

    fn server_with(keys: &[&str]) -> crate::registry::ServerEntry {
        let env: Vec<_> = keys
            .iter()
            .map(|k| json!({"key": k, "value": "https://x.example.test", "secret": false}))
            .collect();
        serde_json::from_value(json!({
            "id": "s", "name": "s", "transport": "stdio", "env": env,
        }))
        .unwrap()
    }

    #[test]
    fn every_service_has_one_row_at_its_variant_index() {
        for (i, def) in SERVICES.iter().enumerate() {
            assert_eq!(def.service as usize, i, "{}", def.name);
            assert_eq!(def.service.name(), def.name);
            assert_eq!(Service::parse(def.name), Some(def.service));
            assert!(!def.token_keys.is_empty(), "{}", def.name);
        }
        let mut names: Vec<_> = SERVICES.iter().map(|d| d.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SERVICES.len());
        assert_eq!(Service::parse("bogus"), None);
    }

    #[test]
    fn each_service_is_inferred_from_every_one_of_its_token_keys() {
        for def in &SERVICES {
            for key in def.token_keys {
                let mut keys = vec![*key];
                keys.extend(def.url_env());
                assert_eq!(
                    infer(&server_with(&keys)),
                    Some((def.service, key.to_string())),
                    "{} via {key}",
                    def.name
                );
            }
            if let Some(url_env) = def.url_env() {
                assert_eq!(infer(&server_with(&[def.token_keys[0]])), None, "{url_env}");
            }
        }
    }

    #[test]
    fn slack_keys_win_over_other_services_and_the_first_key_is_the_default() {
        let server = server_with(&["MIRO_ACCESS_TOKEN", "SLACK_MCP_XOXB_TOKEN"]);
        assert_eq!(
            infer(&server),
            Some((Service::Slack, "SLACK_MCP_XOXB_TOKEN".to_string()))
        );
        assert_eq!(Service::Slack.def().default_token_key(), "SLACK_BOT_TOKEN");
    }

    #[test]
    fn only_odoo_and_moodle_take_a_configured_base_url() {
        for def in &SERVICES {
            let configured = matches!(def.service, Service::Odoo | Service::Moodle);
            assert_eq!(def.url_env().is_some(), configured, "{}", def.name);
            assert_eq!(def.params.is_empty(), def.service != Service::Odoo);
        }
    }
}
