use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::google::ClientVault;
use super::http_probes::{http_registry, HttpProbe, Service, ODOO_COOLDOWN_SECS};
use super::*;

const T0: i64 = 5_000_000;
const SLACK_TOKEN: &str = "FAKE-SLACK-TOKEN";
const ODOO_KEY: &str = "FAKE-ODOO-KEY";
const MOODLE_TOKEN: &str = "FAKE-MOODLE-TOKEN";
const STITCH_KEY: &str = "FAKE-STITCH-KEY";
const FIGMA_KEY: &str = "FAKE-FIGMA-KEY";
const MIRO_TOKEN: &str = "FAKE-MIRO-TOKEN";

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "auth-http-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

type Handler = dyn Fn(&str) -> (u16, String, Vec<(String, String)>) + Send + Sync;

struct Mock {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Mock {
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn matching(&self, needle: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.contains(needle))
            .count()
    }
}

fn serve(delay: Duration, handler: Arc<Handler>) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let log = log.clone();
            let handler = handler.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let mut raw = Vec::new();
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    raw.extend_from_slice(&buf[..n]);
                    if n == 0 {
                        break;
                    }
                    let text = String::from_utf8_lossy(&raw).to_string();
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
                let text = String::from_utf8_lossy(&raw).to_string();
                log.lock().unwrap().push(text.clone());
                std::thread::sleep(delay);
                let (status, body, headers) = handler(&text);
                let extra: String = headers
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}\r\n"))
                    .collect();
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    Mock {
        url: format!("http://127.0.0.1:{port}"),
        requests,
    }
}

fn fixed(status: u16, body: &str) -> Mock {
    let body = body.to_string();
    serve(
        Duration::ZERO,
        Arc::new(move |_| (status, body.clone(), vec![])),
    )
}

fn refused_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

struct FakeVault(Vec<(String, String)>);

impl ClientVault for FakeVault {
    fn get(&self, server: &str, key: &str) -> Option<String> {
        let wanted = format!("{server}::{key}");
        self.0
            .iter()
            .find(|(k, _)| *k == wanted)
            .map(|(_, v)| v.clone())
    }
}

fn vault() -> Box<FakeVault> {
    let entries = [
        ("slack", "SLACK_BOT_TOKEN", SLACK_TOKEN),
        ("odoo", "ODOO_API_KEY", ODOO_KEY),
        ("moodle", "MOODLE_TOKEN", MOODLE_TOKEN),
        ("stitch", "STITCH_API_KEY", STITCH_KEY),
        ("framelink", "FIGMA_API_KEY", FIGMA_KEY),
        ("miro", "MIRO_ACCESS_TOKEN", MIRO_TOKEN),
    ];
    Box::new(FakeVault(
        entries
            .iter()
            .map(|(s, k, v)| (format!("{s}::{k}"), v.to_string()))
            .collect(),
    ))
}

fn server_id(service: Service) -> &'static str {
    match service {
        Service::Slack => "slack",
        Service::Odoo => "odoo",
        Service::Moodle => "moodle",
        Service::Stitch => "stitch",
        Service::Framelink => "framelink",
        Service::MiroCommunity => "miro",
    }
}

fn spec(service: Service, url: &str) -> ProbeSpec {
    let mut spec =
        ProbeSpec::new(server_id(service), ProbeKind::Http).with_param("service", service.name());
    if matches!(service, Service::Odoo | Service::Moodle) {
        spec = spec.with_param("base_url", url);
    }
    if service == Service::Odoo {
        spec = spec
            .with_param("db", "fakedb")
            .with_param("user", "fake@example.test");
    }
    spec
}

fn probe(service: Service, url: &str) -> HttpProbe {
    let p = HttpProbe::new()
        .with_vault(vault())
        .with_timeout(Duration::from_secs(5));
    if matches!(service, Service::Odoo | Service::Moodle) {
        p
    } else {
        p.with_base(service, url).unwrap()
    }
}

fn run(service: Service, url: &str) -> ProbeOutcome {
    probe(service, url).run(&spec(service, url))
}

fn oauth(code: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.into(),
        description: String::new(),
    }
}

fn tracked(outcome: &ProbeOutcome) -> Tracked {
    step(&Tracked::unknown(T0), outcome, T0 + 1)
}

fn assert_state(outcome: &ProbeOutcome, state: &str, reason: &str) {
    let t = tracked(outcome);
    assert_eq!(
        (t.state.name(), t.reason.as_str()),
        (state, reason),
        "{outcome:?}"
    );
}

fn valid_body(service: Service) -> String {
    match service {
        Service::Slack => json!({"ok": true, "user_id": "U1"}).to_string(),
        Service::Odoo => String::new(),
        Service::Moodle => json!({"sitename": "x", "userid": 3}).to_string(),
        Service::Stitch => {
            json!({"jsonrpc": "2.0", "id": 1, "result": {"content": []}}).to_string()
        }
        Service::Framelink => json!({"id": "1", "email": "a@example.test"}).to_string(),
        Service::MiroCommunity => json!({"type": "oauth_token"}).to_string(),
    }
}

const SIMPLE: [Service; 5] = [
    Service::Slack,
    Service::Moodle,
    Service::Stitch,
    Service::Framelink,
    Service::MiroCommunity,
];

#[test]
fn simple_services_succeed_with_expected_request_shape() {
    for service in SIMPLE {
        let m = fixed(200, &valid_body(service));
        assert_eq!(run(service, &m.url), ProbeOutcome::Success, "{service:?}");
        let req = m.requests.lock().unwrap()[0].clone();
        let (line, token) = match service {
            Service::Slack => ("POST /auth.test ", SLACK_TOKEN),
            Service::Moodle => ("POST /webservice/rest/server.php ", MOODLE_TOKEN),
            Service::Stitch => ("POST /mcp ", STITCH_KEY),
            Service::Framelink => ("GET /v1/me ", FIGMA_KEY),
            _ => ("GET /v1/oauth-token ", MIRO_TOKEN),
        };
        assert!(req.starts_with(line), "{service:?}: {req}");
        assert!(req.contains(token), "{service:?} sends its credential");
    }
    let m = fixed(200, &valid_body(Service::Stitch));
    run(Service::Stitch, &m.url);
    assert!(m.matching("list_projects") == 1);
    let m = fixed(200, &valid_body(Service::Moodle));
    run(Service::Moodle, &m.url);
    assert!(m.matching("core_webservice_get_site_info") == 1);
}

#[test]
fn slack_error_codes_classify_on_the_code() {
    let cases = [
        ("invalid_auth", "needs_reauth", "invalid_auth"),
        ("token_expired", "needs_reauth", "token_expired"),
        ("not_authed", "needs_reauth", "not_authed"),
        ("token_revoked", "revoked", "token_revoked"),
        ("account_inactive", "revoked", "account_inactive"),
    ];
    for (code, state, reason) in cases {
        let m = fixed(200, &json!({"ok": false, "error": code}).to_string());
        assert_state(&run(Service::Slack, &m.url), state, reason);
    }
    for code in ["ratelimited", "fatal_error", "Weird Code!"] {
        let m = fixed(200, &json!({"ok": false, "error": code}).to_string());
        assert_state(&run(Service::Slack, &m.url), "unknown", "unknown");
    }
    let m = fixed(429, "{}");
    assert_eq!(
        run(Service::Slack, &m.url),
        ProbeOutcome::HttpStatus { status: 429 }
    );
    let m = fixed(401, r#"{"ok":false,"error":"invalid_auth"}"#);
    assert_state(&run(Service::Slack, &m.url), "needs_reauth", "invalid_auth");
    let m = fixed(401, "nope");
    assert_state(&run(Service::Slack, &m.url), "needs_reauth", "unauthorized");
}

#[test]
fn moodle_error_codes() {
    let m = fixed(
        200,
        r#"{"exception":"moodle_exception","errorcode":"invalidtoken","message":"x"}"#,
    );
    assert_state(
        &run(Service::Moodle, &m.url),
        "needs_reauth",
        "invalidtoken",
    );
    let m = fixed(200, r#"{"exception":"x","errorcode":"accessexception"}"#);
    assert_state(
        &run(Service::Moodle, &m.url),
        "misconfigured",
        "accessexception",
    );
    let m = fixed(200, r#"{"errorcode":"somethingelse"}"#);
    assert_state(&run(Service::Moodle, &m.url), "unknown", "unknown");
    let m = fixed(200, r#"{"unrelated":true}"#);
    assert_eq!(run(Service::Moodle, &m.url), ProbeOutcome::TransportError);
}

#[test]
fn stitch_auth_failures_and_sse() {
    for status in [401, 403] {
        let m = fixed(status, "{}");
        assert_state(
            &run(Service::Stitch, &m.url),
            "needs_reauth",
            "unauthorized",
        );
    }
    let m = fixed(200, "event: message\ndata: {\"result\":{}}\n\n");
    assert_eq!(run(Service::Stitch, &m.url), ProbeOutcome::Success);
    let m = fixed(
        200,
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"x"}}"#,
    );
    assert_eq!(run(Service::Stitch, &m.url), ProbeOutcome::TransportError);
}

#[test]
fn framelink_and_miro_status_mapping() {
    for service in [Service::Framelink, Service::MiroCommunity] {
        let m = fixed(401, "{}");
        assert_state(&run(service, &m.url), "needs_reauth", "unauthorized");
    }
    let m = fixed(403, r#"{"err":"Invalid token"}"#);
    assert_state(
        &run(Service::Framelink, &m.url),
        "needs_reauth",
        "unauthorized",
    );
    let m = fixed(404, "{}");
    assert_eq!(
        run(Service::Framelink, &m.url),
        ProbeOutcome::HttpStatus { status: 404 }
    );
}

#[test]
fn miro_expiry_drives_expiring_then_reauth() {
    let clock = Arc::new(FakeClock::new(T0));
    let at = |secs: i64| {
        let m = fixed(200, &json!({"expires_at": T0 + secs}).to_string());
        HttpProbe::new()
            .with_vault(vault())
            .with_clock(clock.clone())
            .with_base(Service::MiroCommunity, &m.url)
            .unwrap()
            .run(&spec(Service::MiroCommunity, &m.url))
    };
    let soon = at(3600);
    assert_eq!(soon, ProbeOutcome::TokenTtl { secs: 3600 });
    assert_state(&soon, "expiring", "token_expiring");
    assert_state(&at(30 * 24 * 3600), "ok", "ok");
    assert_state(&at(-5), "needs_reauth", "token_expired");

    let m = fixed(200, r#"{"expiresAt":"2026-01-01T00:00:00Z"}"#);
    let p = probe(Service::MiroCommunity, &m.url);
    let out = p.run(&spec(Service::MiroCommunity, &m.url));
    assert_state(&out, "needs_reauth", "token_expired");

    let m = fixed(200, r#"{"expires_at":"2001-09-09T03:46:40+02:00"}"#);
    let out = run(Service::MiroCommunity, &m.url);
    assert!(
        matches!(out, ProbeOutcome::TokenTtl { secs } if secs < 0),
        "{out:?}"
    );
}

#[test]
fn transient_failures_for_every_service() {
    for service in SIMPLE.into_iter().chain([Service::Odoo]) {
        let m = fixed(500, "boom");
        let out = run(service, &m.url);
        assert_eq!(out, ProbeOutcome::HttpStatus { status: 500 }, "{service:?}");
        assert_state(&out, "unknown", "unknown");

        let out = run(service, &refused_url());
        assert_eq!(out, ProbeOutcome::TransportError, "{service:?} refused");

        let m = fixed(200, "<html>not json");
        let out = run(service, &m.url);
        if service != Service::Odoo {
            assert_ne!(out, ProbeOutcome::Success, "{service:?} malformed");
        }
        assert_state(&out, "unknown", "unknown");
    }
}

#[test]
fn timeouts_are_transient() {
    for service in [Service::Slack, Service::Odoo, Service::Framelink] {
        let m = serve(
            Duration::from_millis(1500),
            Arc::new(|_| (200, "{}".to_string(), vec![])),
        );
        let p = HttpProbe::new()
            .with_vault(vault())
            .with_timeout(Duration::from_millis(300));
        let p = if service == Service::Odoo {
            p
        } else {
            p.with_base(service, &m.url).unwrap()
        };
        let started = std::time::Instant::now();
        assert_eq!(p.run(&spec(service, &m.url)), ProbeOutcome::TransportError);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

#[test]
fn redirects_are_not_followed() {
    for service in SIMPLE.into_iter().chain([Service::Odoo]) {
        let target = fixed(200, &valid_body(service));
        let location = format!("{}/stolen", target.url);
        let hop = serve(
            Duration::ZERO,
            Arc::new(move |_| {
                (
                    302,
                    String::new(),
                    vec![("Location".into(), location.clone())],
                )
            }),
        );
        let out = run(service, &hop.url);
        assert_ne!(out, ProbeOutcome::Success, "{service:?}");
        assert_state(&out, "unknown", "unknown");
        assert_eq!(target.count(), 0, "{service:?} followed the redirect");
        assert_eq!(hop.count(), 1);
    }
}

#[test]
fn missing_credential_needs_reauth_without_a_request() {
    for service in SIMPLE.into_iter().chain([Service::Odoo]) {
        let m = fixed(200, "{}");
        let p = HttpProbe::new().with_vault(Box::new(FakeVault(vec![])));
        let p = if matches!(service, Service::Odoo | Service::Moodle) {
            p
        } else {
            p.with_base(service, &m.url).unwrap()
        };
        let out = p.run(&spec(service, &m.url));
        assert_state(&out, "needs_reauth", "no_token");
        assert_eq!(m.count(), 0);
    }
}

#[test]
fn endpoint_rules() {
    assert!(HttpProbe::new()
        .with_base(Service::Slack, "http://slack.com/api")
        .is_err());
    assert!(HttpProbe::new()
        .with_base(Service::Slack, "http://localhost:1")
        .is_err());
    assert!(HttpProbe::new()
        .with_base(Service::Slack, "https://slack.com/api")
        .is_ok());
    assert!(HttpProbe::new()
        .with_base(Service::Slack, "http://127.0.0.1:9")
        .is_ok());
    let m = fixed(200, "{}");
    let p = HttpProbe::new().with_vault(vault());
    let mut bad = spec(Service::Odoo, "http://example.test");
    bad.params
        .insert("base_url".into(), "http://example.test".into());
    assert_state(&p.run(&bad), "misconfigured", "bad_endpoint");
    assert_eq!(m.count(), 0);
}

fn odoo_mock(auth_ok: bool, read_ok: bool) -> Mock {
    serve(
        Duration::ZERO,
        Arc::new(move |req| {
            if req.starts_with("GET /web/health") {
                return (200, r#"{"status":"pass"}"#.into(), vec![]);
            }
            let denied = json!({"jsonrpc":"2.0","id":1,"error":{"code":200,"message":"Odoo Server Error","data":{"name":"odoo.exceptions.AccessDenied","message":"Access Denied"}}});
            if req.contains("\"authenticate\"") {
                let body = if auth_ok {
                    json!({"jsonrpc":"2.0","id":1,"result":7})
                } else {
                    json!({"jsonrpc":"2.0","id":1,"result":false})
                };
                return (200, body.to_string(), vec![]);
            }
            let body = if read_ok {
                json!({"jsonrpc":"2.0","id":1,"result":[{"id":7}]})
            } else {
                denied
            };
            (200, body.to_string(), vec![])
        }),
    )
}

fn odoo_probe(clock: Arc<FakeClock>) -> HttpProbe {
    HttpProbe::new()
        .with_vault(vault())
        .with_timeout(Duration::from_secs(5))
        .with_clock(clock)
}

#[test]
fn odoo_checks_health_then_reads_the_user_record() {
    let m = odoo_mock(true, true);
    let p = odoo_probe(Arc::new(FakeClock::new(T0)));
    assert_eq!(p.run(&spec(Service::Odoo, &m.url)), ProbeOutcome::Success);
    let log = m.requests.lock().unwrap().clone();
    assert!(log[0].starts_with("GET /web/health"));
    assert!(log
        .iter()
        .any(|r| r.contains("\"res.users\"") && r.contains("\"read\"")));
    assert!(log.iter().any(|r| r.contains(ODOO_KEY)));
    assert_eq!(m.matching("\"authenticate\""), 1);

    p.run(&spec(Service::Odoo, &m.url));
    assert_eq!(m.matching("\"authenticate\""), 1, "uid is remembered");

    let m = odoo_mock(true, true);
    let with_uid = spec(Service::Odoo, &m.url).with_param("uid", "7");
    assert_eq!(
        odoo_probe(Arc::new(FakeClock::new(T0))).run(&with_uid),
        ProbeOutcome::Success
    );
    assert_eq!(m.matching("\"authenticate\""), 0);
}

#[test]
fn odoo_down_is_transient_and_skips_the_credential_calls() {
    let m = fixed(502, "bad gateway");
    let out = odoo_probe(Arc::new(FakeClock::new(T0))).run(&spec(Service::Odoo, &m.url));
    assert_eq!(out, ProbeOutcome::HttpStatus { status: 502 });
    assert_eq!(m.count(), 1);
}

#[test]
fn odoo_access_denied_cools_down_for_six_hours() {
    let clock = Arc::new(FakeClock::new(T0));
    let m = odoo_mock(true, false);
    let p = odoo_probe(clock.clone());
    let s = spec(Service::Odoo, &m.url).with_param("uid", "7");
    assert_state(&p.run(&s), "needs_reauth", "access_denied");
    let after_first = m.count();
    for _ in 0..3 {
        clock.advance(3600);
        assert_state(&p.run(&s), "needs_reauth", "access_denied");
    }
    assert_eq!(m.count(), after_first, "no traffic during the cooldown");
    clock.advance(ODOO_COOLDOWN_SECS);
    p.run(&s);
    assert!(m.count() > after_first, "retried after the cooldown");
}

#[test]
fn odoo_failed_authenticate_is_not_looped() {
    let clock = Arc::new(FakeClock::new(T0));
    let m = odoo_mock(false, true);
    let p = odoo_probe(clock.clone());
    for _ in 0..5 {
        assert_state(
            &p.run(&spec(Service::Odoo, &m.url)),
            "needs_reauth",
            "access_denied",
        );
        clock.advance(60);
    }
    assert_eq!(m.matching("\"authenticate\""), 1);
}

#[test]
fn odoo_new_credential_bypasses_the_cooldown() {
    let clock = Arc::new(FakeClock::new(T0));
    let m = odoo_mock(true, false);
    let s = spec(Service::Odoo, &m.url).with_param("uid", "7");
    let p = odoo_probe(clock.clone());
    p.run(&s);
    let before = m.count();
    let rotated = HttpProbe::new()
        .with_vault(Box::new(FakeVault(vec![(
            "odoo::ODOO_API_KEY".into(),
            "FAKE-ODOO-KEY-ROTATED".into(),
        )])))
        .with_clock(clock.clone());
    rotated.run(&s);
    assert!(m.count() > before);
    assert_eq!(
        probe_cooldown_probe(&p, &s),
        oauth("access_denied"),
        "original credential still cooled down"
    );
}

fn probe_cooldown_probe(p: &HttpProbe, s: &ProbeSpec) -> ProbeOutcome {
    p.run(s)
}

#[test]
fn odoo_success_clears_the_cooldown() {
    let clock = Arc::new(FakeClock::new(T0));
    let denied = Arc::new(Mutex::new(true));
    let flag = denied.clone();
    let m = serve(
        Duration::ZERO,
        Arc::new(move |req| {
            if req.starts_with("GET /web/health") {
                return (200, "{}".into(), vec![]);
            }
            if *flag.lock().unwrap() {
                let body = json!({"error":{"message":"Odoo Server Error","data":{"name":"odoo.exceptions.AccessDenied"}}});
                return (200, body.to_string(), vec![]);
            }
            (200, json!({"result":[{"id":7}]}).to_string(), vec![])
        }),
    );
    let s = spec(Service::Odoo, &m.url).with_param("uid", "7");
    let p = odoo_probe(clock.clone());
    assert_state(&p.run(&s), "needs_reauth", "access_denied");
    *denied.lock().unwrap() = false;
    clock.advance(ODOO_COOLDOWN_SECS + 1);
    assert_eq!(p.run(&s), ProbeOutcome::Success);
    *denied.lock().unwrap() = true;
    assert_state(&p.run(&s), "needs_reauth", "access_denied");
}

#[test]
fn odoo_missing_config_is_misconfigured() {
    let m = odoo_mock(true, true);
    let mut s = spec(Service::Odoo, &m.url);
    s.params.remove("db");
    assert_state(
        &odoo_probe(Arc::new(FakeClock::new(T0))).run(&s),
        "misconfigured",
        "missing_config",
    );
    let mut s = spec(Service::Odoo, &m.url);
    s.params.remove("user");
    assert_state(
        &odoo_probe(Arc::new(FakeClock::new(T0))).run(&s),
        "misconfigured",
        "missing_config",
    );
}

fn entry(
    id: &str,
    env: &[(&str, Option<&str>, bool)],
    extra: serde_json::Value,
) -> crate::registry::ServerEntry {
    let env: Vec<_> = env
        .iter()
        .map(|(k, v, s)| json!({"key": k, "value": v, "secret": s}))
        .collect();
    let mut value = json!({"id": id, "name": id, "transport": "stdio", "env": env});
    if let (Some(obj), Some(more)) = (value.as_object_mut(), extra.as_object()) {
        obj.extend(more.clone());
    }
    let mut server: crate::registry::ServerEntry = serde_json::from_value(value).unwrap();
    server.id = id.to_string();
    server
}

#[test]
fn registry_infers_services_and_intervals() {
    let mut reg = crate::registry::Registry::default();
    reg.servers = vec![
        entry("slack", &[("SLACK_BOT_TOKEN", None, true)], json!({})),
        entry(
            "codeforward-odoo",
            &[
                ("ODOO_URL", Some("https://odoo.example.test"), false),
                ("ODOO_DB", Some("db1"), false),
                ("ODOO_USER", Some("me@example.test"), false),
                ("ODOO_API_KEY", None, true),
            ],
            json!({}),
        ),
        entry(
            "moodle-mcp",
            &[
                ("MOODLE_URL", Some("https://moodle.example.test"), false),
                ("MOODLE_TOKEN", None, true),
            ],
            json!({}),
        ),
        entry("stitch", &[("STITCH_API_KEY", None, true)], json!({})),
        entry("framelink", &[("FIGMA_API_KEY", None, true)], json!({})),
        entry(
            "miro-community",
            &[("MIRO_ACCESS_TOKEN", None, true)],
            json!({}),
        ),
        entry("anna", &[("ANNAS_SECRET_KEY", None, true)], json!({})),
        entry("odoo-no-url", &[("ODOO_API_KEY", None, true)], json!({})),
    ];
    let probes = http_registry(&reg);
    assert_eq!(probes.iter().count(), 6);
    let get = |id: &str| probes.get(id).unwrap().clone();
    assert_eq!(get("slack").params["service"], "slack");
    assert_eq!(get("slack").min_interval_secs, 30 * 60);
    let odoo = get("codeforward-odoo");
    assert_eq!(odoo.min_interval_secs, 6 * 3600);
    assert_eq!(odoo.params["base_url"], "https://odoo.example.test");
    assert_eq!(odoo.params["db"], "db1");
    assert_eq!(odoo.params["user"], "me@example.test");
    assert_eq!(get("moodle-mcp").params["token_key"], "MOODLE_TOKEN");
    assert_eq!(get("stitch").min_interval_secs, 10 * 60);
    assert_eq!(get("framelink").params["token_key"], "FIGMA_API_KEY");
    assert_eq!(get("miro-community").params["service"], "miro-community");
    assert!(probes.get("anna").is_none());
    assert!(probes.get("odoo-no-url").is_none());
}

#[test]
fn registry_hint_overrides_inference() {
    let mut reg = crate::registry::Registry::default();
    reg.servers = vec![
        entry(
            "custom-slack",
            &[("MY_TOKEN", None, true)],
            json!({"plus": {"authProbe": {"kind": "slack", "tokenKey": "MY_TOKEN"}}}),
        ),
        entry(
            "silenced",
            &[("SLACK_BOT_TOKEN", None, true)],
            json!({"plus": {"authProbe": {"kind": "none"}}}),
        ),
        entry(
            "hinted-odoo",
            &[("ODOO_API_KEY", None, true)],
            json!({"plus": {"authProbe": {"kind": "odoo", "baseUrl": "https://o.example.test", "db": "d", "uid": "9"}}}),
        ),
        entry(
            "bad-kind",
            &[("SLACK_BOT_TOKEN", None, true)],
            json!({"plus": {"authProbe": {"kind": "bogus"}}}),
        ),
    ];
    let probes = http_registry(&reg);
    assert_eq!(
        probes.get("custom-slack").unwrap().params["token_key"],
        "MY_TOKEN"
    );
    assert!(probes.get("silenced").is_none());
    let odoo = probes.get("hinted-odoo").unwrap();
    assert_eq!(odoo.params["base_url"], "https://o.example.test");
    assert_eq!(odoo.params["uid"], "9");
    assert!(probes.get("bad-kind").is_none());
}

#[test]
fn combined_registry_keeps_google_servers_on_the_refresh_probe() {
    let mut reg = crate::registry::Registry::default();
    reg.servers = vec![
        entry(
            "google-docs-mcp",
            &[
                ("GOOGLE_MCP_PROFILE", Some("work"), false),
                ("SLACK_BOT_TOKEN", None, true),
            ],
            json!({}),
        ),
        entry("slack", &[("SLACK_BOT_TOKEN", None, true)], json!({})),
    ];
    let probes = super::combined_registry(&reg);
    assert_eq!(
        probes.get("google-docs-mcp").unwrap().kind,
        ProbeKind::GoogleRefresh
    );
    assert_eq!(probes.get("slack").unwrap().kind, ProbeKind::Http);
}

#[test]
fn composite_routes_by_kind() {
    let composite = CompositeProbe::default();
    let out = composite.run(&ProbeSpec::new("x", ProbeKind::Http));
    assert_state(&out, "misconfigured", "missing_config");
    assert_eq!(
        composite.run(&ProbeSpec::new("x", ProbeKind::GatewayState)),
        ProbeOutcome::TransportError
    );
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
fn credentials_and_bodies_never_reach_outcomes_debug_or_cache() {
    let cache = scratch("leak-cache");
    let planted = [
        SLACK_TOKEN,
        ODOO_KEY,
        MOODLE_TOKEN,
        STITCH_KEY,
        FIGMA_KEY,
        MIRO_TOKEN,
        "FAKE-BODY-SECRET",
    ];
    let echo = format!(
        "{{\"ok\":false,\"error\":\"invalid_auth FAKE-BODY-SECRET {SLACK_TOKEN}\",\"errorcode\":\"invalidtoken\",\"message\":\"{MOODLE_TOKEN} FAKE-BODY-SECRET\",\"token\":\"{MIRO_TOKEN}\",\"id\":\"{FIGMA_KEY}\"}}"
    );
    let mut seen = String::new();
    for service in SIMPLE.into_iter().chain([Service::Odoo]) {
        for (status, body) in [
            (200, echo.clone()),
            (401, echo.clone()),
            (500, echo.clone()),
            (200, format!("garbage {echo}")),
        ] {
            let m = fixed(status, &body);
            let p = Arc::new(probe(service, &m.url));
            let mut reg = ProbeRegistry::new();
            reg.register(spec(service, &m.url));
            let prober = AuthProber::new(
                AuthStore::new(&cache),
                reg,
                p.clone(),
                Arc::new(FakeClock::new(T0)),
            );
            let report = prober
                .request(server_id(service), Trigger::UserForce)
                .unwrap();
            let direct = p.run(&spec(service, &m.url));
            seen.push_str(&format!("{report:?}{p:?}{direct:?}"));
            seen.push_str(&serde_json::to_string(&report).unwrap());
        }
    }
    seen.push_str(&tree_text(&cache));
    for secret in planted {
        assert!(!seen.contains(secret), "{secret} leaked");
    }
}
