use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::{json, Value};
use toolport_plus_api::memory::ScriptedRun;
use toolport_plus_api::{
    convert, HttpError, HttpResponse, LaunchError, LaunchFault, MemoryHost, MemoryRegistry,
    MemorySecrets, ServerRecord,
};

use super::*;

const T0: i64 = 1_700_000_000;
const CONSENT: &str = "https://accounts.example.invalid/consent?client_id=public-id";

fn server(fields: Value) -> ServerRecord {
    convert(&fields).unwrap()
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("plus-auth-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn google_server(id: &str, profile: Option<&str>) -> ServerRecord {
    let mut env = vec![json!({"key": "GOOGLE_CLIENT_ID", "value": "cid", "secret": false})];
    if let Some(profile) = profile {
        env.push(json!({"key": "GOOGLE_MCP_PROFILE", "value": profile, "secret": false}));
    }
    server(json!({
        "id": id, "name": id, "enabled": true, "transport": "stdio",
        "command": "google-docs-mcp", "env": env,
    }))
}

fn stdio_server(id: &str, kind: Option<&str>) -> ServerRecord {
    let mut fields = json!({
        "id": id, "name": id, "enabled": true, "transport": "stdio", "command": "tool-mcp",
    });
    if let Some(kind) = kind {
        fields["plus"] = json!({"authProbe": {"kind": kind}});
    }
    server(fields)
}

fn remote_server(id: &str) -> ServerRecord {
    server(json!({
        "id": id, "name": id, "enabled": true, "transport": "http",
        "url": "https://remote.example.invalid/mcp",
    }))
}

fn host_with(tag: &str, servers: Vec<ServerRecord>) -> (MemoryHost, std::path::PathBuf) {
    let dir = scratch(tag);
    let host = MemoryHost {
        registry: MemoryRegistry::with_servers(servers),
        secrets: MemorySecrets {
            data_dir: Some(dir.join("data")),
            ..Default::default()
        },
        ..Default::default()
    };
    host.now.set(T0);
    host.secrets
        .set_env("XDG_CONFIG_HOME", dir.join("config").to_str().unwrap());
    (host, dir)
}

fn write_token(dir: &std::path::Path, profile: &str, body: &str) {
    let path = if profile == "default" {
        dir.join("config/google-docs-mcp/token.json")
    } else {
        dir.join(format!("config/google-docs-mcp/{profile}/token.json"))
    };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

const TOKEN_FILE: &str =
    r#"{"refresh_token": "FAKE-refresh", "client_id": "cid", "client_secret": "FAKE-secret"}"#;

fn reply(status: u16, body: Value) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
        truncated: false,
        url: "https://oauth.example.invalid/token".into(),
    })
}

fn probe_one(host: &MemoryHost, server: &str) -> ProbeReport {
    let prober = Prober::new(host)
        .unwrap()
        .with_google_endpoint("http://127.0.0.1:9/token")
        .unwrap()
        .with_stdio_wait(std::time::Duration::from_millis(400));
    prober
        .run(&Selector::One(server.into()), true)
        .unwrap()
        .reports
        .remove(0)
}

fn scripted(stderr: &[&str], exit: Option<i32>) -> ScriptedRun {
    ScriptedRun {
        stderr: stderr.iter().map(|line| line.to_string()).collect(),
        exit,
        ..Default::default()
    }
}

#[test]
fn a_google_refresh_that_succeeds_is_ok_and_sends_no_redirect_following_request() {
    let (host, dir) = host_with("google-ok", vec![google_server("g1", Some("work"))]);
    write_token(&dir, "work", TOKEN_FILE);
    host.http.responses.borrow_mut().push_back(reply(
        200,
        json!({"access_token": "FAKE-access", "expires_in": 3599}),
    ));
    let report = probe_one(&host, "g1");
    assert!(report.ran);
    assert_eq!(report.tracked.state, AuthState::Ok);
    let requests = host.http.requests.borrow();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].follow_redirects);
    assert_eq!(requests[0].url, "http://127.0.0.1:9/token");
}

#[test]
fn a_google_invalid_grant_needs_a_new_login_and_a_revoked_one_says_so() {
    let (host, dir) = host_with("google-grant", vec![google_server("g1", Some("work"))]);
    write_token(&dir, "work", TOKEN_FILE);
    host.http.responses.borrow_mut().push_back(reply(
        400,
        json!({"error": "invalid_grant", "error_description": "Token has been expired or revoked."}),
    ));
    let report = probe_one(&host, "g1");
    assert_eq!(report.tracked.state, AuthState::Revoked);
    assert_eq!(report.tracked.reason, "revoked");
}

#[test]
fn a_missing_token_file_is_a_login_that_never_happened() {
    let (host, _dir) = host_with("google-nofile", vec![google_server("g1", Some("work"))]);
    let report = probe_one(&host, "g1");
    assert_eq!(report.tracked.state, AuthState::NeedsReauth);
    assert_eq!(report.tracked.reason, "no_token_file");
    assert!(host.http.requests.borrow().is_empty());
}

#[test]
fn a_server_error_or_a_lost_connection_is_not_a_verdict() {
    let (host, dir) = host_with("google-transient", vec![google_server("g1", None)]);
    write_token(&dir, "default", TOKEN_FILE);
    host.http.responses.borrow_mut().extend([
        reply(503, json!({})),
        Err(HttpError::Connect("refused".into())),
    ]);
    let first = probe_one(&host, "g1");
    assert_eq!(first.tracked.state, AuthState::Unknown);
    assert_eq!(first.tracked.transient.map(|run| run.count), Some(1));
    host.now.set(T0 + 60);
    let second = probe_one(&host, "g1");
    assert_eq!(second.tracked.transient.map(|run| run.count), Some(2));
}

#[test]
fn an_error_body_never_puts_its_text_on_screen() {
    let (host, dir) = host_with("google-sanitize", vec![google_server("g1", Some("work"))]);
    write_token(&dir, "work", TOKEN_FILE);
    host.http.responses.borrow_mut().push_back(reply(
        401,
        json!({"error": "FAKE-secret leaked <b>", "error_description": "FAKE-secret"}),
    ));
    let report = probe_one(&host, "g1");
    assert_eq!(report.tracked.transient.map(|run| run.count), Some(1));
    assert!(!format!("{report:?}").contains("FAKE-secret"));
}

#[test]
fn the_token_endpoint_must_be_https_or_loopback() {
    assert!(endpoint_allowed("https://oauth2.googleapis.com/token"));
    assert!(endpoint_allowed("http://127.0.0.1:8080/token"));
    for refused in [
        "http://oauth2.googleapis.com/token",
        "http://localhost/token",
        "https://user@host.example/token",
        "ftp://host.example/",
        "token",
    ] {
        assert!(!endpoint_allowed(refused), "{refused}");
    }
}

#[test]
fn a_stdio_server_that_prints_a_consent_url_needs_a_login_and_is_stopped() {
    let (host, _dir) = host_with("stdio-url", vec![stdio_server("s1", Some("stdio"))]);
    let run = scripted(&["starting", CONSENT], None);
    let killed = Arc::clone(&run.killed);
    host.auth.runs.borrow_mut().push_back(Ok(run));
    let report = probe_one(&host, "s1");
    assert_eq!(report.tracked.state, AuthState::NeedsReauth);
    assert_eq!(report.tracked.reason, "not_authed");
    assert!(killed.load(Ordering::SeqCst));
}

#[test]
fn a_stdio_server_whose_auth_command_exits_cleanly_is_signed_in() {
    let (host, _dir) = host_with("stdio-ok", vec![stdio_server("s1", Some("stdio"))]);
    host.auth
        .runs
        .borrow_mut()
        .push_back(Ok(scripted(&["already signed in"], Some(0))));
    assert_eq!(probe_one(&host, "s1").tracked.state, AuthState::Ok);
}

#[test]
fn a_stdio_server_that_cannot_start_is_transient_and_a_refused_one_is_misconfigured() {
    let (host, _dir) = host_with("stdio-fail", vec![stdio_server("s1", Some("stdio"))]);
    host.auth.runs.borrow_mut().extend([
        Err(LaunchError::new(LaunchFault::Spawn, "no such file")),
        Err(LaunchError::new(LaunchFault::Refused, "screened")),
    ]);
    assert_eq!(
        probe_one(&host, "s1").tracked.transient.map(|r| r.count),
        Some(1)
    );
    let refused = probe_one(&host, "s1");
    assert_eq!(refused.tracked.state, AuthState::Misconfigured);
}

#[test]
fn only_stdio_and_google_servers_get_a_probe() {
    let (host, _dir) = host_with(
        "registry",
        vec![
            remote_server("remote"),
            stdio_server("plain", None),
            stdio_server("opted", Some("stdio")),
            stdio_server("off", Some("none")),
            google_server("g-profile", Some("work")),
            google_server("g-default", None),
            server(
                json!({"id": "g-bad", "name": "g-bad", "transport": "stdio", "command": "x",
                "env": [{"key": "GOOGLE_MCP_PROFILE", "value": "../x", "secret": false}]}),
            ),
        ],
    );
    let registry = probe_registry(&host, &read_servers(&host).unwrap());
    let ids: Vec<_> = registry.iter().map(|spec| spec.server.as_str()).collect();
    assert_eq!(ids, ["g-default", "g-profile", "opted"]);
    assert_eq!(
        registry
            .get("g-default")
            .unwrap()
            .profile_gate_key()
            .as_deref(),
        Some("google:default")
    );
}

#[test]
fn a_team_server_waiting_for_review_is_never_started_by_a_probe() {
    let (host, _dir) = host_with("review", vec![stdio_server("s1", Some("stdio"))]);
    host.auth.awaiting_review.borrow_mut().insert("s1".into());
    let prober = Prober::new(&host).unwrap();
    assert!(!prober.has_probe("s1"));
    assert!(host.auth.started.borrow().is_empty());
}

#[test]
fn servers_of_one_google_account_share_one_scheduled_probe() {
    let (host, dir) = host_with(
        "profile-gate",
        vec![
            google_server("g1", Some("work")),
            google_server("g2", Some("work")),
        ],
    );
    write_token(&dir, "work", TOKEN_FILE);
    host.http
        .responses
        .borrow_mut()
        .push_back(reply(200, json!({"access_token": "FAKE-access"})));
    let prober = Prober::new(&host)
        .unwrap()
        .with_google_endpoint("http://127.0.0.1:9/t")
        .unwrap();
    let first = prober.run(&Selector::Due, false).unwrap();
    assert_eq!(first.probed(), 1);
    let again = prober.run(&Selector::Due, false).unwrap();
    assert_eq!(again.probed(), 0);
    assert_eq!(host.http.requests.borrow().len(), 1);
}

#[cfg(unix)]
#[test]
fn the_status_file_is_private_and_a_removed_server_is_pruned_from_it() {
    use std::os::unix::fs::PermissionsExt;
    let (host, dir) = host_with("store", vec![stdio_server("s1", Some("stdio"))]);
    host.auth
        .runs
        .borrow_mut()
        .push_back(Ok(scripted(&[], Some(0))));
    probe_one(&host, "s1");
    let status = dir.join("data/auth/status.json");
    let mode = std::fs::metadata(&status).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(AuthStore::new(&dir.join("data/auth"))
        .read_status()
        .servers
        .contains_key("s1"));
    host.registry.servers.borrow_mut().clear();
    Prober::new(&host)
        .unwrap()
        .run(&Selector::Due, false)
        .unwrap();
    assert!(AuthStore::new(&dir.join("data/auth"))
        .read_status()
        .servers
        .is_empty());
}

#[test]
fn a_status_file_from_the_old_line_stays_readable() {
    let (_host, dir) = host_with("compat", vec![]);
    let auth_dir = dir.join("data/auth");
    std::fs::create_dir_all(&auth_dir).unwrap();
    std::fs::write(
        auth_dir.join("status.json"),
        r#"{"version":1,"servers":{"s1":{"tracked":{"state":"needs_reauth","reason":"no_token","since":5,"transient":null},"lastProbeAt":7,"nextDueAt":9,"hintKind":"api_token","tokenKey":"K"}},"profiles":{"google:work":3}}"#,
    )
    .unwrap();
    let status = AuthStore::new(&auth_dir).read_status();
    assert_eq!(status.servers["s1"].tracked.state, AuthState::NeedsReauth);
    assert_eq!(status.profiles["google:work"], 3);
}

#[test]
fn the_statusline_and_the_hook_only_count_servers_that_still_have_a_probe() {
    let (host, dir) = host_with("surfaces", vec![stdio_server("s1", Some("stdio"))]);
    host.auth
        .runs
        .borrow_mut()
        .push_back(Ok(scripted(&[CONSENT], None)));
    probe_one(&host, "s1");
    let status = AuthStore::new(&dir.join("data/auth")).read_status();
    let line = statusline(&status, T0, &["s1".to_string()]);
    assert_eq!(line["auth"]["needs_reauth"], 1);
    assert_eq!(line["auth"]["worst"], json!(["s1"]));
    assert_eq!(line["auth"]["text"], "auth: 1 need re-auth [s1]");
    let hooked = hook(&status, T0, &["s1".to_string()]);
    assert_eq!(
        hooked["hookSpecificOutput"]["hookEventName"],
        "SessionStart"
    );
    assert!(hooked["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("s1: needs_reauth (Sign in to s1 again)"));
    assert_eq!(statusline(&status, T0, &[])["auth"]["text"], "auth ok (0)");
}

#[test]
fn a_consent_url_with_credentials_in_its_authority_is_not_taken() {
    assert_eq!(
        find_consent_url("open (https://a.example/auth?x=1), then wait").as_deref(),
        Some("https://a.example/auth?x=1")
    );
    assert_eq!(find_consent_url("https://user:pw@a.example/auth"), None);
    assert_eq!(find_consent_url("no link here"), None);
}

fn no_sink() -> UrlSink {
    Arc::new(|_| {})
}

#[test]
fn a_stdio_login_prints_the_link_opens_it_and_waits_for_the_command_to_end() {
    let (host, _dir) = host_with("login-stdio", vec![stdio_server("s1", None)]);
    host.auth
        .runs
        .borrow_mut()
        .push_back(Ok(scripted(&[CONSENT], Some(0))));
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink: UrlSink = {
        let seen = Arc::clone(&seen);
        Arc::new(move |url| seen.lock().unwrap().push(url.to_string()))
    };
    let report = login(&host, "s1", LoginOptions { open_browser: true }, sink).unwrap();
    assert_eq!(report.flow, "stdio");
    assert_eq!(report.consent_url.as_deref(), Some(CONSENT));
    assert_eq!(*seen.lock().unwrap(), [CONSENT]);
    assert_eq!(*host.auth.opened.borrow(), [CONSENT]);
}

#[test]
fn a_login_with_no_open_leaves_the_browser_alone() {
    let (host, _dir) = host_with("login-no-open", vec![stdio_server("s1", None)]);
    host.auth
        .runs
        .borrow_mut()
        .push_back(Ok(scripted(&[CONSENT], Some(0))));
    login(
        &host,
        "s1",
        LoginOptions {
            open_browser: false,
        },
        no_sink(),
    )
    .unwrap();
    assert!(host.auth.opened.borrow().is_empty());
}

#[test]
fn an_auth_command_that_exits_without_a_link_is_reported_with_its_code() {
    let (host, _dir) = host_with("login-exit", vec![stdio_server("s1", None)]);
    host.auth
        .runs
        .borrow_mut()
        .push_back(Ok(scripted(&["boom"], Some(3))));
    let error = login(&host, "s1", LoginOptions { open_browser: true }, no_sink()).unwrap_err();
    assert_eq!(
        error.message(),
        "the auth command of s1 exited with 3 without printing a consent URL"
    );
}

#[test]
fn a_remote_login_runs_upstreams_own_flow() {
    let (host, _dir) = host_with("login-remote", vec![remote_server("r1")]);
    let report = login(&host, "r1", LoginOptions { open_browser: true }, no_sink()).unwrap();
    assert_eq!(report.flow, "browser");
    assert_eq!(*host.auth.remote_signins.borrow(), ["r1"]);
    assert!(host.auth.started.borrow().is_empty());
}

#[test]
fn a_remote_login_opens_the_browser_itself_and_asks_for_no_link() {
    let (host, _dir) = host_with("login-remote-open", vec![remote_server("r1")]);
    *host.auth.remote_consent.borrow_mut() = Some(CONSENT.to_string());
    let report = login(&host, "r1", LoginOptions { open_browser: true }, no_sink()).unwrap();
    assert_eq!(*host.auth.remote_sinks.borrow(), [false]);
    assert_eq!(report.consent_url, None);
}

#[test]
fn a_remote_login_with_no_open_prints_the_consent_url_and_leaves_the_browser_alone() {
    let (host, _dir) = host_with("login-remote-no-open", vec![remote_server("r1")]);
    *host.auth.remote_consent.borrow_mut() = Some(CONSENT.to_string());
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink: UrlSink = {
        let seen = Arc::clone(&seen);
        Arc::new(move |url| seen.lock().unwrap().push(url.to_string()))
    };
    let report = login(
        &host,
        "r1",
        LoginOptions {
            open_browser: false,
        },
        sink,
    )
    .unwrap();
    assert_eq!(*host.auth.remote_sinks.borrow(), [true]);
    assert_eq!(*seen.lock().unwrap(), [CONSENT]);
    assert_eq!(report.consent_url.as_deref(), Some(CONSENT));
    assert!(report.signed_in);
    assert!(host.auth.opened.borrow().is_empty());
}

#[test]
fn a_failed_remote_login_with_no_open_reports_the_failure_not_a_link() {
    let (host, _dir) = host_with("login-remote-no-open-fail", vec![remote_server("r1")]);
    *host.auth.remote_result.borrow_mut() = Some(Err("the sign-in timed out".into()));
    let error = login(
        &host,
        "r1",
        LoginOptions {
            open_browser: false,
        },
        no_sink(),
    )
    .unwrap_err();
    assert_eq!(error.message(), "the sign-in timed out");
}

#[test]
fn a_remote_server_with_client_credentials_has_no_browser_sign_in() {
    let mut remote = remote_server("r1");
    remote
        .extra
        .insert("clientCredentials".into(), json!({"clientId": "x"}));
    let (host, _dir) = host_with("login-cc", vec![remote]);
    let error = login(&host, "r1", LoginOptions { open_browser: true }, no_sink()).unwrap_err();
    assert!(matches!(error, LoginError::Unsupported { .. }));
}

#[test]
fn a_server_waiting_for_review_does_not_sign_in() {
    let (host, _dir) = host_with("login-review", vec![stdio_server("s1", None)]);
    host.auth.awaiting_review.borrow_mut().insert("s1".into());
    let error = login(&host, "s1", LoginOptions { open_browser: true }, no_sink()).unwrap_err();
    assert!(error.message().contains("needs consent"));
    assert!(host.auth.started.borrow().is_empty());
}

#[test]
fn the_sign_in_tool_leaves_the_auth_command_running_and_returns_the_link() {
    let (host, _dir) = host_with("tool", vec![stdio_server("s1", None)]);
    let run = scripted(&[CONSENT], None);
    let (killed, detached) = (Arc::clone(&run.killed), Arc::clone(&run.detached));
    host.auth.runs.borrow_mut().push_back(Ok(run));
    let out = start_sign_in(&host, "s1").unwrap();
    assert_eq!(out["authUrl"], CONSENT);
    assert_eq!(out["started"], true);
    assert!(detached.load(Ordering::SeqCst));
    assert!(!killed.load(Ordering::SeqCst));
}

#[test]
fn the_sign_in_tool_keeps_launch_secrets_out_of_the_link_and_the_tail() {
    let (host, _dir) = host_with("tool-secret", vec![stdio_server("s1", None)]);
    let mut run = scripted(&["token FAKE-launch-value"], Some(1));
    run.secret = Some("FAKE-launch-value".into());
    host.auth.runs.borrow_mut().push_back(Ok(run));
    let out = start_sign_in(&host, "s1").unwrap();
    assert!(!out.to_string().contains("FAKE-launch-value"));
    assert_eq!(out["stderrTail"], json!(["token ***"]));
    assert_eq!(out["authUrl"], Value::Null);
}

#[test]
fn a_follow_up_probe_runs_only_for_a_server_that_has_one() {
    let (host, _dir) = host_with(
        "follow",
        vec![
            stdio_server("plain", None),
            stdio_server("s1", Some("stdio")),
        ],
    );
    assert!(follow_up(&host, "plain").rows.is_empty());
    host.auth
        .runs
        .borrow_mut()
        .push_back(Ok(scripted(&[], Some(0))));
    let follow = follow_up(&host, "s1");
    assert_eq!(follow.rows.len(), 1);
    assert_eq!(follow.probe["tracked"]["state"], "ok");
}

#[test]
fn status_and_doctor_name_the_logins_that_need_attention_without_failing_the_check() {
    use crate::status::{doctor, snapshot, status};
    let (host, _dir) = host_with("sections", vec![stdio_server("s1", Some("stdio"))]);
    host.secrets.set_env("HOME", "/nonexistent");
    let quiet = doctor(&host, &snapshot(&host));
    let check = quiet.checks.iter().find(|c| c.name == "logins").unwrap();
    assert_eq!(check.detail, "no stdio or Google logins checked yet");
    host.auth
        .runs
        .borrow_mut()
        .push_back(Ok(scripted(&[CONSENT], None)));
    probe_one(&host, "s1");
    let section = status(&host, &snapshot(&host))["auth"].clone();
    assert_eq!(section["counts"]["needs_reauth"], 1);
    assert_eq!(section["servers"][0]["server"], "s1");
    let report = doctor(&host, &snapshot(&host));
    let check = report.checks.iter().find(|c| c.name == "logins").unwrap();
    assert!(check.detail.starts_with("1 of 1 logins need attention: s1"));
    assert_eq!(check.status, crate::health::Health::Warn);
    let _ = report;
}
