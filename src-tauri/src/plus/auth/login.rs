//! The one-click sign-in fix (B2-03) behind `toolportctl auth login`: the browser flow of the
//! gateway for remote OAuth servers, the server's own `auth` subcommand for stdio ones. Nothing
//! secret is ever returned; a consent URL carries no credential.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{json, Value};

use super::http_probes::{combined_registry, PARAM_TOKEN_KEY};
use super::probe::{Clock, ProbeKind, ProbeSpec};
use super::scan::{self, ProbeRun, Selector};
use super::stdio;
use super::surfaces::{self, AuthRow};
use super::SystemClock;
use crate::plus::args::{flag_or, str_nonempty};
use crate::plus::servers;
use crate::registry::{Registry, ServerEntry};

pub type UrlSink = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginOptions {
    pub open_browser: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginReport {
    pub server: String,
    pub name: String,
    pub flow: &'static str,
    pub consent_url: Option<String>,
    pub signed_in: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginError {
    NotFound(String),
    Unsupported { reason: String, next: String },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Browser,
    Stdio,
    Unsupported { reason: String, next: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteFacts {
    pub oauth_state: bool,
    pub static_token: bool,
    pub detected: String,
}

fn reprobe(id: &str) -> String {
    format!("toolportctl auth probe --server {id} --force")
}

fn unsupported(reason: String, next: String) -> Plan {
    Plan::Unsupported { reason, next }
}

fn app_secret_step(id: &str) -> String {
    format!(
        "enter the new value in the Toolport+ app (server settings, secrets), then run {}",
        reprobe(id)
    )
}

pub fn plan(
    server: &ServerEntry,
    spec: Option<&ProbeSpec>,
    remote_facts: impl FnOnce() -> RemoteFacts,
) -> Plan {
    let (id, name) = (&server.id, &server.name);
    if spec.is_some_and(|s| s.kind == ProbeKind::Http) {
        let key = spec
            .and_then(|s| s.params.get(PARAM_TOKEN_KEY))
            .map_or("<KEY>", String::as_str);
        return unsupported(
            format!("{name} signs in with an API token, so there is no browser sign-in"),
            format!(
                "toolportctl secret set {id} {key} (value on stdin or --value-env <VAR>), then {}",
                reprobe(id)
            ),
        );
    }
    let remote = matches!(server.transport.as_str(), "http" | "sse") && server.url.is_some();
    if remote && server.client_credentials.is_some() {
        return unsupported(
            format!("{name} signs in with client credentials, so there is no browser sign-in"),
            app_secret_step(id),
        );
    }
    if remote {
        let facts = remote_facts();
        if facts.oauth_state {
            return Plan::Browser;
        }
        if facts.static_token {
            return unsupported(
                format!("{name} signs in with a static token, so there is no browser sign-in"),
                app_secret_step(id),
            );
        }
        return match facts.detected.as_str() {
            "token" => unsupported(
                format!("{name} signs in with an API token, so there is no browser sign-in"),
                app_secret_step(id),
            ),
            "none" => unsupported(
                format!("{name} does not ask for a sign-in"),
                format!("toolportctl server info {id} to check its address"),
            ),
            _ => Plan::Browser,
        };
    }
    if server.transport == "stdio" && server.command.is_some() {
        return Plan::Stdio;
    }
    unsupported(
        format!("{name} has no sign-in flow"),
        format!("toolportctl server info {id}"),
    )
}

/// `pub(crate)`: also reused by `prober::gateway_hint_kind` to route a `GatewayState`
/// probe's dead `auth login` hint to `secret set` for API-token remotes (MIG-AUTH-11),
/// so the two surfaces never classify the same remote's auth mechanism differently.
pub(crate) fn remote_facts(server: &ServerEntry) -> RemoteFacts {
    let held = |key: &str| {
        crate::secrets::get_secret_result(&server.id, key)
            .ok()
            .flatten()
            .is_some_and(|value| !value.is_empty())
    };
    let oauth_state = held(crate::remote::OAUTH_STATE_KEY);
    let static_token = !oauth_state && held(crate::secrets::HTTP_AUTH_KEY);
    let detected = if oauth_state || static_token {
        String::new()
    } else {
        crate::vendors::probe_auth(server.url.as_deref().unwrap_or_default()).kind
    };
    RemoteFacts {
        oauth_state,
        static_token,
        detected,
    }
}

fn browser_flow(
    server: &ServerEntry,
    opts: LoginOptions,
    sink: UrlSink,
) -> Result<LoginReport, LoginError> {
    let url = server.url.clone().unwrap_or_default();
    let seen: Arc<Mutex<Option<String>>> = Arc::default();
    let attempt = crate::oauth_controller::start_attempt();
    let observed = Arc::clone(&seen);
    crate::oauth::with_consent_observer(
        move |consent| {
            *observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(consent.to_string());
            sink(consent);
            if opts.open_browser {
                crate::oauth::open_browser(consent);
            }
        },
        || crate::oauth_controller::authenticate(&server.id, &url, &attempt),
    )
    .map_err(LoginError::Failed)?;
    let consent_url = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    Ok(LoginReport {
        server: server.id.clone(),
        name: server.name.clone(),
        flow: "browser",
        consent_url,
        signed_in: true,
        message: format!("Signed in to {}.", server.name),
    })
}

fn stdio_flow(
    server: &ServerEntry,
    opts: LoginOptions,
    sink: UrlSink,
) -> Result<LoginReport, LoginError> {
    let fail = |message: String| LoginError::Failed(message);
    let mut session = stdio::start(server).map_err(|e| fail(e.message))?;
    let capture = session.wait_url(stdio::LOGIN_URL_WAIT);
    let report = |consent_url: Option<String>, message: String| LoginReport {
        server: server.id.clone(),
        name: server.name.clone(),
        flow: "stdio",
        consent_url,
        signed_in: true,
        message,
    };
    let Some(url) = capture.url else {
        return match capture.exit {
            Some(0) => Ok(report(
                None,
                format!("{} reports it is already signed in.", server.name),
            )),
            Some(code) => Err(fail(format!(
                "the auth command of {} exited with {code} without printing a consent URL",
                server.name
            ))),
            None => Err(fail(format!(
                "{} printed no consent URL within {}s",
                server.name,
                stdio::LOGIN_URL_WAIT.as_secs()
            ))),
        };
    };
    sink(&url);
    if opts.open_browser {
        let _ = crate::oauth::open_web_url(&url);
    }
    match session.wait_exit(stdio::LOGIN_FLOW_WAIT) {
        Some(0) => Ok(report(Some(url), format!("Signed in to {}.", server.name))),
        Some(code) => Err(fail(format!(
            "the auth command of {} exited with {code} before the sign-in finished",
            server.name
        ))),
        None => {
            session.kill();
            Err(fail(format!(
                "timed out after {}s waiting for the browser sign-in of {}",
                stdio::LOGIN_FLOW_WAIT.as_secs(),
                server.name
            )))
        }
    }
}

pub fn review_gate(registry: &Registry, server: &ServerEntry) -> Option<Plan> {
    let pending = server.needs_team_enable_review()
        && !registry.is_enabled(&registry.active_profile_id(), &server.id);
    pending.then(|| {
        unsupported(
            format!(
                "{} is a team server that needs consent for its command, address or authentication",
                server.name
            ),
            format!(
                "enable it from Teams after review, then run toolportctl auth login {}",
                server.id
            ),
        )
    })
}

pub fn login(key: &str, opts: LoginOptions, sink: UrlSink) -> Result<LoginReport, LoginError> {
    let registry = super::scan::read_registry().map_err(LoginError::Failed)?;
    let server = servers::find(&registry, key)
        .ok_or_else(|| LoginError::NotFound(format!("unknown server: {key}")))?;
    let probes = combined_registry(&registry);
    let chosen = review_gate(&registry, server)
        .unwrap_or_else(|| plan(server, probes.get(&server.id), || remote_facts(server)));
    match chosen {
        Plan::Browser => browser_flow(server, opts, sink),
        Plan::Stdio => stdio_flow(server, opts, sink),
        Plan::Unsupported { reason, next } => Err(LoginError::Unsupported { reason, next }),
    }
}

/// A forced re-probe of the server just signed in to: what the status cache says afterwards.
pub struct FollowUp {
    pub probe: Value,
    pub rows: Vec<AuthRow>,
    pub run: Option<ProbeRun>,
}

pub fn follow_up(server: &str) -> FollowUp {
    let outcome = scan::default_prober()
        .and_then(|prober| scan::run(&prober, &Selector::One(server.to_string()), true, 1));
    match outcome {
        Ok(run) => {
            let status = surfaces::auth_dir()
                .map(|dir| surfaces::read_status(&dir))
                .unwrap_or_default();
            let rows = surfaces::rows_for(&status, SystemClock.now(), &run.servers());
            FollowUp {
                probe: json!(run.reports.first()),
                rows,
                run: Some(run),
            }
        }
        Err(_) => FollowUp {
            probe: Value::Null,
            rows: Vec::new(),
            run: None,
        },
    }
}

pub fn report_value(report: &LoginReport, follow: &FollowUp) -> Value {
    let mut data = serde_json::to_value(report).unwrap_or_default();
    data["probe"] = follow.probe.clone();
    data["servers"] = json!(follow.rows);
    data
}

impl LoginError {
    /// The text `toolportctl auth login` and the app show for a refused or failed sign-in.
    pub fn message(&self) -> String {
        match self {
            LoginError::NotFound(message) | LoginError::Failed(message) => message.clone(),
            LoginError::Unsupported { reason, next } => format!("{reason}. Next: {next}"),
        }
    }
}

/// `plus.auth.login`: the one-click fix of a `reauth` or `reconsent` row. It blocks until the
/// browser sign-in ends, so the caller runs it off the UI thread (`plus_invoke` already does).
pub fn login_handler(args: Value) -> Result<Value, String> {
    let server = str_nonempty(&args, "server").ok_or_else(|| "server is required".to_string())?;
    let opts = LoginOptions {
        open_browser: flag_or(&args, "openBrowser", true),
    };
    let sink: UrlSink = Arc::new(|_| {});
    let report = login(server, opts, sink).map_err(|error| error.message())?;
    Ok(report_value(&report, &follow_up(&report.server)))
}
