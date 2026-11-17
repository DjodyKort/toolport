//! The sign-in behind `toolportctl auth login` and the `servers_auth` tool. A remote server signs
//! in through upstream's own OAuth flow, the one the Sign in button of the app runs. A stdio
//! server, Google's included, signs in through its own `auth` subcommand, which prints the
//! consent URL. A consent URL carries no credential; nothing secret is ever returned.

use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use serde_json::{json, Value};
use toolport_plus_api::{ConsentSink, Host, LaunchError, ServerRecord};

use super::prober::{read_servers, Prober, Selector};
use super::stdio;
use super::surfaces::{self, AuthRow};
use crate::servers;

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
    Launch(LaunchError),
}

impl LoginError {
    /// The text a surface shows for a refused or failed sign-in.
    pub fn message(&self) -> String {
        match self {
            LoginError::NotFound(message) | LoginError::Failed(message) => message.clone(),
            LoginError::Launch(error) => error.message.clone(),
            LoginError::Unsupported { reason, next } => format!("{reason}. Next: {next}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Remote,
    Stdio,
    Unsupported { reason: String, next: String },
}

fn unsupported(reason: String, next: String) -> Plan {
    Plan::Unsupported { reason, next }
}

fn is_remote(server: &ServerRecord) -> bool {
    matches!(server.text("transport"), Some("http" | "sse")) && server.text("url").is_some()
}

pub fn plan(server: &ServerRecord) -> Plan {
    let (id, name) = (&server.id, &server.name);
    if is_remote(server) {
        if server
            .extra
            .get("clientCredentials")
            .is_some_and(|value| !value.is_null())
        {
            return unsupported(
                format!("{name} signs in with client credentials, so there is no browser sign-in"),
                "set its client secret in the app: Servers, Secrets".to_string(),
            );
        }
        return Plan::Remote;
    }
    if server.text("transport") == Some("stdio") && server.text("command").is_some() {
        return Plan::Stdio;
    }
    unsupported(
        format!("{name} has no sign-in flow"),
        format!("toolportctl server info {id}"),
    )
}

pub fn review_gate(host: &dyn Host, server: &ServerRecord) -> Result<Option<Plan>, String> {
    Ok(host.awaits_review(&server.id)?.then(|| {
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
    }))
}

fn find_server(host: &dyn Host, key: &str) -> Result<ServerRecord, LoginError> {
    let all = read_servers(host).map_err(LoginError::Failed)?;
    servers::find(&all, key)
        .cloned()
        .ok_or_else(|| LoginError::NotFound(format!("unknown server: {key}")))
}

fn remote_flow(
    host: &dyn Host,
    server: &ServerRecord,
    opts: LoginOptions,
    sink: UrlSink,
) -> Result<LoginReport, LoginError> {
    let consent = Arc::new(Mutex::new(None::<String>));
    let on_consent: Option<ConsentSink> = (!opts.open_browser).then(|| {
        let consent = Arc::clone(&consent);
        Arc::new(move |url: &str| {
            *consent.lock().unwrap_or_else(PoisonError::into_inner) = Some(url.to_string());
            sink(url);
        }) as ConsentSink
    });
    host.sign_in_remote(&server.id, on_consent)
        .map_err(LoginError::Failed)?;
    let consent_url = consent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
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
    host: &dyn Host,
    server: &ServerRecord,
    opts: LoginOptions,
    sink: UrlSink,
) -> Result<LoginReport, LoginError> {
    let fail = |message: String| LoginError::Failed(message);
    let mut session = stdio::start(host, &server.id).map_err(|e| fail(e.message))?;
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
        let _ = host.open_url(&url);
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

pub fn login(
    host: &dyn Host,
    key: &str,
    opts: LoginOptions,
    sink: UrlSink,
) -> Result<LoginReport, LoginError> {
    let server = find_server(host, key)?;
    let chosen = match review_gate(host, &server).map_err(LoginError::Failed)? {
        Some(gate) => gate,
        None => plan(&server),
    };
    match chosen {
        Plan::Remote => remote_flow(host, &server, opts, sink),
        Plan::Stdio => stdio_flow(host, &server, opts, sink),
        Plan::Unsupported { reason, next } => Err(LoginError::Unsupported { reason, next }),
    }
}

/// What `servers_auth` returns: the consent URL of a stdio server, with its `auth` command left
/// running to catch the redirect. A remote server goes through the same flow as `auth login`.
pub fn start_sign_in(host: &dyn Host, key: &str) -> Result<Value, LoginError> {
    let server = find_server(host, key)?;
    if let Some(Plan::Unsupported { reason, next }) =
        review_gate(host, &server).map_err(LoginError::Failed)?
    {
        return Err(LoginError::Unsupported { reason, next });
    }
    if is_remote(&server) {
        let report = remote_flow(
            host,
            &server,
            LoginOptions { open_browser: true },
            Arc::new(|_| {}),
        )?;
        return Ok(json!({
            "name": server.name,
            "started": true,
            "flow": report.flow,
            "signedIn": report.signed_in,
        }));
    }
    let mut session = stdio::start(host, &server.id).map_err(LoginError::Launch)?;
    let capture = session.wait_url(AUTH_WAIT);
    let signing_in = capture.url.is_some();
    if signing_in {
        session.detach();
    } else {
        session.kill();
    }
    Ok(json!({
        "name": server.name,
        "started": true,
        "authUrl": capture.url,
        "exited": capture.exit.is_some(),
        "timedOut": capture.timed_out,
        "stderrTail": capture.tail,
        "hint": signing_in.then_some("Open authUrl in a browser; the server's local callback saves the token on completion."),
    }))
}

const AUTH_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// A forced re-probe of the server just signed in to: what the status file says afterwards.
pub struct FollowUp {
    pub probe: Value,
    pub rows: Vec<AuthRow>,
}

pub fn follow_up(host: &dyn Host, server: &str) -> FollowUp {
    let empty = FollowUp {
        probe: Value::Null,
        rows: Vec::new(),
    };
    let Ok(prober) = Prober::new(host) else {
        return empty;
    };
    if !prober.has_probe(server) {
        return empty;
    }
    match prober.run(&Selector::One(server.to_string()), true) {
        Ok(run) => FollowUp {
            probe: json!(run.reports.first()),
            rows: surfaces::rows_for(&prober.status(), host.now_unix(), &run.servers()),
        },
        Err(_) => empty,
    }
}

pub fn report_value(report: &LoginReport, follow: &FollowUp) -> Value {
    let mut data = serde_json::to_value(report).unwrap_or_default();
    data["probe"] = follow.probe.clone();
    data["servers"] = json!(follow.rows);
    data
}
