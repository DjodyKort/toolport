use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde_json::json;

use super::google::{ClientVault, GoogleRefreshProbe, PARAM_CONFIG_DIR};
use super::*;

const T0: i64 = 3_000_000;
const REFRESH: &str = "FAKE-REFRESH-TOKEN";
const SECRET: &str = "FAKE-CLIENT-SECRET";

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "auth-google-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Mock {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

fn mock(status: u16, body: &str, delay: Duration) -> Mock {
    mock_raw(status, body, delay, &[])
}

fn mock_raw(status: u16, body: &str, delay: Duration, headers: &[(&str, &str)]) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    let body = body.to_string();
    let extra: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}\r\n"))
        .collect();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 8192];
            let mut raw = Vec::new();
            loop {
                let n = stream.read(&mut buf).unwrap_or(0);
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw).to_string();
                if n == 0 {
                    break;
                }
                if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if rest.len() >= len {
                        break;
                    }
                }
            }
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&raw).to_string());
            std::thread::sleep(delay);
            let response = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Mock {
        url: format!("http://127.0.0.1:{port}/token"),
        requests,
    }
}

struct FakeVault(Vec<(&'static str, &'static str)>);

impl ClientVault for FakeVault {
    fn get(&self, server: &str, key: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(k, _)| *k == format!("{server}::{key}"))
            .map(|(_, v)| v.to_string())
    }
}

fn no_vault() -> Box<FakeVault> {
    Box::new(FakeVault(vec![]))
}

fn write_token(dir: &std::path::Path, profile: &str, body: serde_json::Value) -> PathBuf {
    let path = dir.join(profile).join("token.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body.to_string()).unwrap();
    path
}

fn full_token() -> serde_json::Value {
    json!({"type": "authorized_user", "client_id": "fake-id", "client_secret": SECRET, "refresh_token": REFRESH})
}

fn spec(dir: &std::path::Path) -> ProbeSpec {
    ProbeSpec::new("google-docs-mcp", ProbeKind::GoogleRefresh)
        .with_profile("work")
        .with_param(PARAM_CONFIG_DIR, dir.to_str().unwrap())
}

fn probe_for(m: &Mock) -> GoogleRefreshProbe {
    GoogleRefreshProbe::new()
        .with_endpoint(&m.url)
        .unwrap()
        .with_vault(no_vault())
        .with_timeout(Duration::from_secs(5))
}

fn oauth(code: &str, description: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.into(),
        description: description.into(),
    }
}

fn state_after(outcome: &ProbeOutcome) -> Tracked {
    step(&Tracked::unknown(T0), outcome, T0 + 1)
}

#[test]
fn success_posts_refresh_grant_and_records_ttl() {
    let dir = scratch("ok");
    write_token(&dir, "work", full_token());
    let m = mock(
        200,
        r#"{"access_token":"FAKE-ACCESS","expires_in":3599,"token_type":"Bearer"}"#,
        Duration::ZERO,
    );
    let probe = probe_for(&m);
    assert_eq!(probe.run(&spec(&dir)), ProbeOutcome::Success);
    assert_eq!(probe.last_ttl("google-docs-mcp"), Some(3599));
    let requests = m.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /token"));
    assert!(requests[0].contains("grant_type=refresh_token"));
    assert!(requests[0].contains("client_id=fake-id"));
    assert_eq!(state_after(&ProbeOutcome::Success).state, AuthState::Ok);
}

#[test]
fn invalid_grant_expired_needs_reauth_and_revoked_is_revoked() {
    let dir = scratch("grant");
    write_token(&dir, "work", full_token());
    let m = mock(
        400,
        r#"{"error":"invalid_grant","error_description":"Token has been expired."}"#,
        Duration::ZERO,
    );
    let out = probe_for(&m).run(&spec(&dir));
    assert_eq!(out, oauth("invalid_grant", "expired"));
    let t = state_after(&out);
    assert_eq!(
        (t.state, t.reason.as_str()),
        (AuthState::NeedsReauth, "invalid_grant")
    );

    let m = mock(
        400,
        r#"{"error":"invalid_grant","error_description":"Token has been revoked."}"#,
        Duration::ZERO,
    );
    let out = probe_for(&m).run(&spec(&dir));
    let t = state_after(&out);
    assert_eq!(
        (t.state, t.reason.as_str()),
        (AuthState::Revoked, "revoked")
    );

    let m = mock(
        400,
        r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
        Duration::ZERO,
    );
    let t = state_after(&probe_for(&m).run(&spec(&dir)));
    assert_eq!(t.state, AuthState::NeedsReauth);
}

#[test]
fn client_errors_are_misconfigured() {
    let dir = scratch("client");
    write_token(&dir, "work", full_token());
    for code in ["invalid_client", "deleted_client", "unauthorized_client"] {
        let m = mock(401, &json!({"error": code}).to_string(), Duration::ZERO);
        let t = state_after(&probe_for(&m).run(&spec(&dir)));
        assert_eq!(
            (t.state, t.reason.as_str()),
            (AuthState::Misconfigured, code)
        );
    }
}

#[test]
fn hostile_error_code_is_not_echoed() {
    let dir = scratch("hostile");
    write_token(&dir, "work", full_token());
    let m = mock(
        400,
        &json!({"error": format!("x {REFRESH}")}).to_string(),
        Duration::ZERO,
    );
    let out = probe_for(&m).run(&spec(&dir));
    assert_eq!(out, oauth("unknown_error", ""));
    assert_eq!(state_after(&out).state, AuthState::Unknown);
}

#[test]
fn server_errors_and_refused_connections_are_transient() {
    let dir = scratch("transient");
    write_token(&dir, "work", full_token());
    let m = mock(503, "unavailable", Duration::ZERO);
    let out = probe_for(&m).run(&spec(&dir));
    assert_eq!(out, ProbeOutcome::HttpStatus { status: 503 });
    assert!(matches!(classify(&out), Classification::Transient));

    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let probe = GoogleRefreshProbe::new()
        .with_endpoint(&format!("http://127.0.0.1:{port}/token"))
        .unwrap()
        .with_vault(no_vault());
    assert_eq!(probe.run(&spec(&dir)), ProbeOutcome::TransportError);
}

#[test]
fn slow_response_times_out_as_transport_error() {
    let dir = scratch("slow");
    write_token(&dir, "work", full_token());
    let m = mock(200, r#"{"access_token":"a"}"#, Duration::from_secs(3));
    let probe = probe_for(&m).with_timeout(Duration::from_millis(300));
    let started = std::time::Instant::now();
    assert_eq!(probe.run(&spec(&dir)), ProbeOutcome::TransportError);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn malformed_or_incomplete_responses_are_transient() {
    let dir = scratch("malformed");
    write_token(&dir, "work", full_token());
    for (status, body) in [(200, "not json"), (200, "{}"), (400, "<html>"), (400, "{}")] {
        let m = mock(status, body, Duration::ZERO);
        let out = probe_for(&m).run(&spec(&dir));
        assert!(
            matches!(classify(&out), Classification::Transient),
            "{status} {body}: {out:?}"
        );
    }
}

#[test]
fn redirects_are_not_followed() {
    let dir = scratch("redirect");
    write_token(&dir, "work", full_token());
    let target = mock(200, r#"{"access_token":"a"}"#, Duration::ZERO);
    let m = mock_raw(302, "", Duration::ZERO, &[("location", &target.url)]);
    let out = probe_for(&m).run(&spec(&dir));
    assert!(
        matches!(classify(&out), Classification::Transient),
        "{out:?}"
    );
    assert!(target.requests.lock().unwrap().is_empty());
}

#[test]
fn token_file_variants() {
    let dir = scratch("files");
    let m = mock(200, r#"{"access_token":"a"}"#, Duration::ZERO);
    let probe = probe_for(&m);

    let out = probe.run(&spec(&dir));
    assert_eq!(out, oauth("no_token_file", ""));
    let t = state_after(&out);
    assert_eq!(
        (t.state, t.reason.as_str()),
        (AuthState::NeedsReauth, "no_token_file")
    );

    let path = write_token(&dir, "work", json!({}));
    std::fs::write(&path, "{not json").unwrap();
    let out = probe.run(&spec(&dir));
    assert_eq!(out, oauth("bad_token_file", ""));
    assert_eq!(state_after(&out).reason, "bad_token_file");

    std::fs::write(&path, r#"{"client_id":"x"}"#).unwrap();
    assert_eq!(probe.run(&spec(&dir)), oauth("bad_token_file", ""));

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(probe.run(&spec(&dir)), oauth("bad_token_file", ""));
    assert!(m.requests.lock().unwrap().is_empty());
}

#[test]
fn profile_paths_and_traversal() {
    let dir = scratch("paths");
    let m = mock(200, r#"{"access_token":"a"}"#, Duration::ZERO);
    let probe = probe_for(&m);
    let evil = ProbeSpec::new("g", ProbeKind::GoogleRefresh)
        .with_profile("../x")
        .with_param(PARAM_CONFIG_DIR, dir.to_str().unwrap());
    assert_eq!(probe.run(&evil), oauth("bad_token_file", ""));

    std::fs::write(dir.join("token.json"), full_token().to_string()).unwrap();
    let default = ProbeSpec::new("g", ProbeKind::GoogleRefresh)
        .with_profile("default")
        .with_param(PARAM_CONFIG_DIR, dir.to_str().unwrap());
    assert_eq!(probe.run(&default), ProbeOutcome::Success);
}

#[test]
fn token_file_change_short_circuits_without_exchange() {
    let dir = scratch("mtime");
    let path = write_token(&dir, "work", full_token());
    let m = mock(200, r#"{"access_token":"a"}"#, Duration::ZERO);
    let probe = probe_for(&m);
    assert_eq!(probe.run(&spec(&dir)), ProbeOutcome::Success);
    assert_eq!(probe.run(&spec(&dir)), ProbeOutcome::Success);
    assert_eq!(m.requests.lock().unwrap().len(), 2);

    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(120))
        .unwrap();
    drop(file);
    assert_eq!(probe.run(&spec(&dir)), ProbeOutcome::FileChanged);
    assert_eq!(m.requests.lock().unwrap().len(), 2);
    assert_eq!(probe.run(&spec(&dir)), ProbeOutcome::Success);

    let stuck = Tracked {
        state: AuthState::NeedsReauth,
        reason: "invalid_grant".into(),
        since: T0,
        transient: None,
    };
    assert_eq!(
        step(&stuck, &ProbeOutcome::FileChanged, T0 + 5).state,
        AuthState::Ok
    );
}

#[test]
fn client_credentials_fall_back_to_vault_only_when_file_lacks_them() {
    let dir = scratch("vault");
    write_token(&dir, "work", json!({"refresh_token": REFRESH}));
    let m = mock(200, r#"{"access_token":"a"}"#, Duration::ZERO);
    let probe = GoogleRefreshProbe::new()
        .with_endpoint(&m.url)
        .unwrap()
        .with_vault(Box::new(FakeVault(vec![
            ("google-docs-mcp::GOOGLE_CLIENT_ID", "vault-id"),
            ("google-docs-mcp::GOOGLE_CLIENT_SECRET", "vault-secret"),
        ])));
    assert_eq!(probe.run(&spec(&dir)), ProbeOutcome::Success);
    assert!(m.requests.lock().unwrap()[0].contains("client_id=vault-id"));

    let m2 = mock(200, r#"{"access_token":"a"}"#, Duration::ZERO);
    let nothing = probe_for(&m2);
    let out = nothing.run(&spec(&dir));
    assert_eq!(out, oauth("invalid_client", ""));
    assert_eq!(state_after(&out).state, AuthState::Misconfigured);
    assert!(m2.requests.lock().unwrap().is_empty());
}

#[test]
fn endpoint_must_be_https_or_loopback_http() {
    assert!(GoogleRefreshProbe::new()
        .with_endpoint("http://example.com/token")
        .is_err());
    assert!(GoogleRefreshProbe::new()
        .with_endpoint("http://localhost/token")
        .is_err());
    assert!(GoogleRefreshProbe::new()
        .with_endpoint("ftp://127.0.0.1/token")
        .is_err());
    assert!(GoogleRefreshProbe::new().with_endpoint("nonsense").is_err());
    assert!(GoogleRefreshProbe::new()
        .with_endpoint("https://oauth2.googleapis.com/token")
        .is_ok());
    assert!(GoogleRefreshProbe::new()
        .with_endpoint("http://127.0.0.1:9/token")
        .is_ok());
}

fn tree_text(dir: &std::path::Path) -> String {
    let mut out = String::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.push_str(&tree_text(&path));
        } else {
            out.push_str(&String::from_utf8_lossy(&std::fs::read(&path).unwrap()));
        }
    }
    out
}

#[test]
fn secrets_never_reach_outcomes_debug_or_cache() {
    let dir = scratch("leak");
    let cache = scratch("leak-cache");
    write_token(&dir, "work", full_token());
    let echo = json!({
        "error": "invalid_grant",
        "error_description": format!("revoked {REFRESH} {SECRET}"),
        "refresh_token": REFRESH,
        "client_secret": SECRET,
    })
    .to_string();
    let bodies = [
        (400, echo.clone()),
        (
            200,
            json!({"access_token": "FAKE-ACCESS", "refresh_token": REFRESH}).to_string(),
        ),
        (500, echo),
        (200, format!("garbage {REFRESH} {SECRET}")),
    ];
    let mut seen = String::new();
    for (status, body) in bodies {
        let m = mock(status, &body, Duration::ZERO);
        let probe = Arc::new(probe_for(&m));
        let prober = AuthProber::new(
            AuthStore::new(&cache),
            {
                let mut reg = ProbeRegistry::new();
                reg.register(spec(&dir));
                reg
            },
            probe.clone(),
            Arc::new(FakeClock::new(T0)),
        );
        let report = prober
            .request("google-docs-mcp", Trigger::UserForce)
            .unwrap();
        seen.push_str(&format!("{report:?}{probe:?}{:?}", probe.run(&spec(&dir))));
        seen.push_str(&serde_json::to_string(&report).unwrap());
    }
    let missing = scratch("leak-missing");
    let m = mock(200, "{}", Duration::ZERO);
    seen.push_str(&format!("{:?}", probe_for(&m).run(&spec(&missing))));
    seen.push_str(&tree_text(&cache));
    for planted in [REFRESH, SECRET, "FAKE-ACCESS"] {
        assert!(!seen.contains(planted), "{planted} leaked");
    }
}

#[test]
fn prober_gates_google_profile_to_one_exchange_per_interval() {
    let dir = scratch("gate");
    let cache = scratch("gate-cache");
    write_token(&dir, "work", full_token());
    let m = mock(
        200,
        r#"{"access_token":"a","expires_in":3600}"#,
        Duration::ZERO,
    );
    let clock = Arc::new(FakeClock::new(T0));
    let mut reg = ProbeRegistry::new();
    reg.register(spec(&dir));
    let prober = AuthProber::new(
        AuthStore::new(&cache),
        reg,
        Arc::new(probe_for(&m)),
        clock.clone(),
    );
    assert!(
        prober
            .request("google-docs-mcp", Trigger::Scheduled)
            .unwrap()
            .ran
    );
    clock.advance(3600);
    let skipped = prober
        .request("google-docs-mcp", Trigger::Scheduled)
        .unwrap();
    assert!(!skipped.ran);
    assert_eq!(m.requests.lock().unwrap().len(), 1);
    clock.advance(6 * 3600);
    assert!(
        prober
            .request("google-docs-mcp", Trigger::Scheduled)
            .unwrap()
            .ran
    );
    assert_eq!(m.requests.lock().unwrap().len(), 2);
}

#[test]
fn google_registry_reads_plain_profile_env() {
    let mut reg = crate::registry::Registry::default();
    let mut server = |id: &str, key: &str, value: Option<&str>, secret: bool| {
        let mut entry: crate::registry::ServerEntry = serde_json::from_value(json!({
            "id": id, "name": id, "transport": "stdio",
            "env": [{"key": key, "value": value, "secret": secret}],
        }))
        .unwrap();
        entry.id = id.to_string();
        reg.servers.push(entry);
    };
    server(
        "google-docs-cf",
        "GOOGLE_MCP_PROFILE",
        Some("codeforward"),
        false,
    );
    server("other", "ODOO_URL", Some("x"), false);
    server("bad", "GOOGLE_MCP_PROFILE", Some("../x"), false);
    server("secretive", "GOOGLE_MCP_PROFILE", None, true);
    let registry = super::google::google_registry(&reg);
    let spec = registry.get("google-docs-cf").unwrap();
    assert_eq!(spec.profile.as_deref(), Some("codeforward"));
    assert_eq!(
        spec.profile_gate_key().as_deref(),
        Some("google:codeforward")
    );
    assert_eq!(registry.iter().count(), 1);
}

#[test]
fn probe_handler_validates_arguments() {
    let _lock = crate::registry::data_dir_test_lock();
    let dir = scratch("handler");
    let _guard = crate::registry::DataDirOverride::set(&dir);
    assert!(crate::plus::dispatch("plus.auth.probe", json!({})).is_err());
    let err = crate::plus::dispatch("plus.auth.probe", json!({"server": "nope", "force": true}))
        .unwrap_err();
    assert!(err.contains("no probe registered"), "{err}");
}
