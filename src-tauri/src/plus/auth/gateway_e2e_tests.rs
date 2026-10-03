use std::collections::HashSet;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::*;
use crate::registry::{Registry, ServerEntry};

const SLACK_BEARER: &str = "FAKE-slack-bearer";

struct Cfg {
    access_ttl: u64,
    issue_refresh: bool,
    static_tokens: Vec<String>,
}

#[derive(Default)]
struct State {
    access: HashSet<String>,
    refresh: HashSet<String>,
    codes: Vec<(String, String)>,
    counter: u32,
    revoked: bool,
    grants: Vec<String>,
    authorize_hits: u32,
    registrations: u32,
    pkce_ok: Vec<bool>,
    mcp_auth: Vec<Option<String>>,
    oauth_paths: Vec<String>,
}

struct Mock {
    origin: String,
    mcp_url: String,
    cfg: Arc<Mutex<Cfg>>,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn json_reply(status: u16, body: Value) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    tiny_http::Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
}

fn header(request: &tiny_http::Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

fn challenge_of(verifier: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Mock {
    fn start(access_ttl: u64, issue_refresh: bool, static_tokens: &[&str]) -> Mock {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let origin = format!("http://127.0.0.1:{port}");
        let mcp_url = format!("{origin}/mcp");
        let cfg = Arc::new(Mutex::new(Cfg {
            access_ttl,
            issue_refresh,
            static_tokens: static_tokens.iter().map(|t| t.to_string()).collect(),
        }));
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let (cfg, state, stop) = (cfg.clone(), state.clone(), stop.clone());
            let (origin, mcp_url) = (origin.clone(), mcp_url.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                        continue;
                    };
                    handle(&cfg, &state, &origin, &mcp_url, request);
                }
            })
        };
        Mock {
            origin,
            mcp_url,
            cfg,
            state,
            stop,
            handle: Some(handle),
        }
    }

    fn set_ttl(&self, ttl: u64) {
        self.cfg.lock().unwrap().access_ttl = ttl;
    }

    fn expire_access_tokens(&self) {
        self.state.lock().unwrap().access.clear();
    }

    fn revoke(&self) {
        self.state.lock().unwrap().revoked = true;
    }

    fn forget_refresh_tokens(&self) {
        self.state.lock().unwrap().refresh.clear();
    }

    fn grants(&self) -> Vec<String> {
        self.state.lock().unwrap().grants.clone()
    }

    fn mcp_auth(&self) -> Vec<Option<String>> {
        self.state.lock().unwrap().mcp_auth.clone()
    }

    fn oauth_hits(&self) -> usize {
        self.state.lock().unwrap().oauth_paths.len()
    }
}

fn issue(cfg: &Cfg, state: &mut State) -> Value {
    state.counter += 1;
    let access = format!("FAKE-access-{}", state.counter);
    state.access.insert(access.clone());
    let mut body = json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": cfg.access_ttl,
    });
    if cfg.issue_refresh {
        let refresh = format!("FAKE-refresh-{}", state.counter);
        state.refresh.insert(refresh.clone());
        body["refresh_token"] = json!(refresh);
    }
    body
}

fn handle(
    cfg: &Mutex<Cfg>,
    state: &Mutex<State>,
    origin: &str,
    mcp_url: &str,
    mut request: tiny_http::Request,
) {
    let parsed = url::Url::parse(&format!("http://mock{}", request.url())).unwrap();
    let path = parsed.path().to_string();
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);
    let cfg = cfg.lock().unwrap();
    let mut st = state.lock().unwrap();
    let response = match path.as_str() {
        "/mcp" => {
            let auth = header(&request, "Authorization");
            st.mcp_auth.push(auth.clone());
            let token = auth.as_deref().and_then(|a| a.strip_prefix("Bearer "));
            let valid = token.is_some_and(|t| {
                cfg.static_tokens.iter().any(|s| s == t) || (st.access.contains(t) && !st.revoked)
            });
            if !valid {
                let challenge = format!("Bearer resource_metadata=\"{origin}/oauth-resource\"");
                json_reply(401, json!({"error": "unauthorized"})).with_header(
                    tiny_http::Header::from_bytes(b"WWW-Authenticate", challenge.as_bytes())
                        .unwrap(),
                )
            } else if request.method() != &tiny_http::Method::Post {
                tiny_http::Response::from_string("").with_status_code(405)
            } else {
                let req: Value = serde_json::from_str(&body).unwrap_or_default();
                match (req.get("id").cloned(), req["method"].as_str()) {
                    (None, _) => tiny_http::Response::from_string("").with_status_code(202),
                    (Some(id), Some("initialize")) => json_reply(
                        200,
                        json!({"jsonrpc": "2.0", "id": id, "result": {
                            "protocolVersion": "2025-06-18",
                            "capabilities": {"tools": {}},
                            "serverInfo": {"name": "mock-remote", "version": "0"}
                        }}),
                    ),
                    (Some(id), Some("tools/list")) => json_reply(
                        200,
                        json!({"jsonrpc": "2.0", "id": id, "result": {"tools": [{
                            "name": "echo",
                            "description": "Echo",
                            "inputSchema": {"type": "object"}
                        }]}}),
                    ),
                    (Some(id), Some("tools/call")) => json_reply(
                        200,
                        json!({"jsonrpc": "2.0", "id": id, "result": {
                            "content": [{"type": "text", "text": req["params"]["arguments"]["text"]}]
                        }}),
                    ),
                    (Some(id), Some("ping")) => {
                        json_reply(200, json!({"jsonrpc": "2.0", "id": id, "result": {}}))
                    }
                    (Some(id), _) => json_reply(
                        200,
                        json!({"jsonrpc": "2.0", "id": id,
                               "error": {"code": -32601, "message": "method not found"}}),
                    ),
                }
            }
        }
        "/oauth-resource" => json_reply(
            200,
            json!({"resource": mcp_url, "authorization_servers": [origin]}),
        ),
        "/.well-known/oauth-authorization-server" => json_reply(
            200,
            json!({
                "issuer": origin,
                "authorization_endpoint": format!("{origin}/authorize"),
                "token_endpoint": format!("{origin}/token"),
                "registration_endpoint": format!("{origin}/register"),
                "code_challenge_methods_supported": ["S256"],
                "grant_types_supported": ["authorization_code", "refresh_token"],
            }),
        ),
        "/register" => {
            st.oauth_paths.push(path.clone());
            st.registrations += 1;
            json_reply(201, json!({"client_id": "FAKE-client-1"}))
        }
        "/authorize" => {
            st.oauth_paths.push(path.clone());
            st.authorize_hits += 1;
            let q: std::collections::HashMap<String, String> = parsed
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            let well_formed = q.get("response_type").map(String::as_str) == Some("code")
                && q.get("code_challenge_method").map(String::as_str) == Some("S256")
                && q.get("resource").map(String::as_str) == Some(mcp_url)
                && q.get("client_id").map(String::as_str) == Some("FAKE-client-1");
            if !well_formed {
                json_reply(400, json!({"error": "invalid_request"}))
            } else {
                let code = format!("FAKE-code-{}", st.authorize_hits);
                st.codes.push((code.clone(), q["code_challenge"].clone()));
                let location = format!(
                    "{}?code={code}&state={}",
                    q["redirect_uri"],
                    urlencoding::encode(&q["state"])
                );
                tiny_http::Response::from_string("")
                    .with_status_code(302)
                    .with_header(
                        tiny_http::Header::from_bytes(b"Location", location.as_bytes()).unwrap(),
                    )
            }
        }
        "/token" => {
            st.oauth_paths.push(path.clone());
            let form: std::collections::HashMap<String, String> =
                url::form_urlencoded::parse(body.as_bytes())
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
            let grant = form.get("grant_type").cloned().unwrap_or_default();
            st.grants.push(grant.clone());
            match grant.as_str() {
                "authorization_code" => {
                    let code = form.get("code").cloned().unwrap_or_default();
                    let verifier = form.get("code_verifier").cloned().unwrap_or_default();
                    let known = st
                        .codes
                        .iter()
                        .find(|(c, _)| *c == code)
                        .map(|(_, challenge)| *challenge == challenge_of(&verifier));
                    st.pkce_ok.push(known == Some(true));
                    if known == Some(true) {
                        json_reply(200, issue(&cfg, &mut st))
                    } else {
                        json_reply(400, json!({"error": "invalid_grant"}))
                    }
                }
                "refresh_token" => {
                    let presented = form.get("refresh_token").cloned().unwrap_or_default();
                    if st.revoked {
                        json_reply(
                            400,
                            json!({"error": "invalid_grant",
                                   "error_description": "Token has been revoked"}),
                        )
                    } else if st.refresh.remove(&presented) {
                        json_reply(200, issue(&cfg, &mut st))
                    } else {
                        json_reply(
                            400,
                            json!({"error": "invalid_grant",
                                   "error_description": "Refresh token is unknown"}),
                        )
                    }
                }
                _ => json_reply(400, json!({"error": "unsupported_grant_type"})),
            }
        }
        _ => tiny_http::Response::from_string("").with_status_code(404),
    };
    let _ = request.respond(response);
}

struct PathGuard(Option<std::ffi::OsString>);

impl Drop for PathGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn with_world(test: impl FnOnce()) {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let bin = std::env::temp_dir().join(format!("gateway-e2e-bin-{}", std::process::id()));
        std::fs::create_dir_all(&bin).unwrap();
        let script = bin.join("xdg-open");
        std::fs::write(
            &script,
            "#!/bin/sh\ncurl -s -o /dev/null -L --max-time 10 \"$1\" &\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old = std::env::var_os("PATH");
        let _restore = PathGuard(old.clone());
        let mut paths = vec![bin.clone()];
        paths.extend(std::env::split_paths(&old.unwrap_or_default()));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        test();
        let _ = std::fs::remove_dir_all(&bin);
    });
}

fn remote(id: &str, url: &str) -> ServerEntry {
    ServerEntry {
        id: id.into(),
        name: id.into(),
        transport: "http".into(),
        command: None,
        args: vec![],
        env: vec![],
        url: Some(url.into()),
        source: Some("imported:mcpm".into()),
        disabled_tools: vec![],
        cwd: None,
        client_credentials: None,
        request_timeout_ms: None,
        initialize_timeout_ms: None,
        launch: None,
        unknown_fields: serde_json::Map::new(),
    }
}

fn sign_in(server: &ServerEntry) {
    let attempt = crate::oauth_controller::start_attempt();
    crate::oauth_controller::authenticate_with(
        &server.id,
        server.url.as_deref().unwrap(),
        &attempt,
        || Ok(()),
    )
    .unwrap();
}

fn vaulted(server: &str, key: &str) -> Option<String> {
    crate::secrets::get_secret(server, key)
}

fn tracked(outcome: &ProbeOutcome) -> Tracked {
    step(&Tracked::unknown(0), outcome, 0)
}

fn probe(server: &str) -> ProbeOutcome {
    GatewayStateProbe::default().run(&ProbeSpec::new(server, ProbeKind::GatewayState))
}

fn files_text(dir: &Path, out: &mut String) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_text(&path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
}

fn assert_not_on_disk(tokens: &[&str]) {
    let mut text = String::new();
    files_text(&crate::registry::conduit_dir().unwrap(), &mut text);
    for token in tokens {
        assert!(!text.contains(token), "{token} found in the data dir");
    }
}

#[test]
fn interactive_connect_uses_pkce_and_vaults_the_tokens() {
    with_world(|| {
        let mock = Mock::start(30 * 86_400, true, &[]);
        let server = remote("figma", &mock.mcp_url);
        sign_in(&server);
        let st = mock.state.lock().unwrap();
        assert_eq!(st.authorize_hits, 1);
        assert_eq!(st.registrations, 1);
        assert_eq!(st.pkce_ok, [true]);
        drop(st);
        assert_eq!(mock.grants(), ["authorization_code"]);
        assert_eq!(
            vaulted("figma", "__http_auth__").as_deref(),
            Some("FAKE-access-1")
        );
        let state: Value =
            serde_json::from_str(&vaulted("figma", "__oauth_state__").unwrap()).unwrap();
        assert_eq!(state["client_id"], "FAKE-client-1");
        assert_eq!(state["refresh_token"], "FAKE-refresh-1");
        assert_eq!(state["issuer"], mock.origin);
        assert_eq!(state["resource"], mock.mcp_url);
        assert!(state["expires_at"].as_u64().unwrap() > 0);

        let mut ds = crate::remote::connect_remote(&server).unwrap();
        assert_eq!(ds.tools.len(), 1);
        let reply = ds.call("echo", json!({"text": "FAKE-ping"})).unwrap();
        assert_eq!(reply["content"][0]["text"], "FAKE-ping");
        let sent: Vec<Option<String>> = mock
            .mcp_auth()
            .into_iter()
            .filter(|a| a.is_some())
            .collect();
        assert!(!sent.is_empty());
        assert!(sent
            .iter()
            .all(|a| a.as_deref() == Some("Bearer FAKE-access-1")));
        assert_eq!(mock.grants(), ["authorization_code"]);
        assert_not_on_disk(&["FAKE-access-1", "FAKE-refresh-1"]);

        let outcome = probe("figma");
        assert!(matches!(outcome, ProbeOutcome::TokenTtl { secs } if secs > 29 * 86_400));
        assert_eq!(tracked(&outcome).state, AuthState::Ok);
    });
}

#[test]
fn token_is_refreshed_before_it_expires() {
    with_world(|| {
        let mock = Mock::start(30, true, &[]);
        let server = remote("clickup", &mock.mcp_url);
        sign_in(&server);
        mock.set_ttl(3600);
        let before = mock.mcp_auth().len();

        let mut ds = crate::remote::connect_remote(&server).unwrap();
        assert_eq!(ds.tools.len(), 1);
        ds.call("echo", json!({"text": "FAKE-ping"})).unwrap();

        assert_eq!(mock.grants(), ["authorization_code", "refresh_token"]);
        let used: Vec<Option<String>> = mock
            .mcp_auth()
            .split_off(before)
            .into_iter()
            .filter(Option::is_some)
            .collect();
        assert!(used
            .iter()
            .all(|a| a.as_deref() == Some("Bearer FAKE-access-2")));
        assert_eq!(
            vaulted("clickup", "__http_auth__").as_deref(),
            Some("FAKE-access-2")
        );
        let state: Value =
            serde_json::from_str(&vaulted("clickup", "__oauth_state__").unwrap()).unwrap();
        assert_eq!(state["refresh_token"], "FAKE-refresh-2");
        assert_not_on_disk(&["FAKE-access-2", "FAKE-refresh-2"]);
    });
}

#[test]
fn a_rejected_token_triggers_one_reactive_refresh() {
    with_world(|| {
        let mock = Mock::start(3600, true, &[]);
        let server = remote("miro", &mock.mcp_url);
        sign_in(&server);
        mock.expire_access_tokens();
        let before = mock.mcp_auth().len();

        let mut ds = crate::remote::connect_remote(&server).unwrap();
        ds.call("echo", json!({"text": "FAKE-ping"})).unwrap();

        assert_eq!(mock.grants(), ["authorization_code", "refresh_token"]);
        let used: Vec<Option<String>> = mock
            .mcp_auth()
            .split_off(before)
            .into_iter()
            .filter(Option::is_some)
            .collect();
        assert_eq!(
            used.first().unwrap().as_deref(),
            Some("Bearer FAKE-access-1")
        );
        assert!(used[1..]
            .iter()
            .all(|a| a.as_deref() == Some("Bearer FAKE-access-2")));
    });
}

#[test]
fn revocation_surfaces_as_revoked_and_connect_reports_an_auth_error() {
    with_world(|| {
        let mock = Mock::start(30, true, &[]);
        let server = remote("figma", &mock.mcp_url);
        sign_in(&server);
        mock.revoke();

        let outcome = probe("figma");
        assert_eq!(
            outcome,
            ProbeOutcome::OauthError {
                code: "invalid_grant".into(),
                description: "revoked".into()
            }
        );
        let state = tracked(&outcome);
        assert_eq!(state.state, AuthState::Revoked);
        assert_eq!(state.reason, "revoked");
        assert!(state.state.is_issue());

        let error = crate::remote::connect_remote(&server).err().unwrap();
        assert!(crate::remote::is_auth_error(&error), "{error}");
        assert!(!error.contains("FAKE-access"));
        assert!(!error.contains("FAKE-refresh"));
    });
}

#[test]
fn an_unknown_refresh_token_needs_a_new_login() {
    with_world(|| {
        let mock = Mock::start(30, true, &[]);
        let server = remote("figma", &mock.mcp_url);
        sign_in(&server);
        mock.forget_refresh_tokens();
        let state = tracked(&probe("figma"));
        assert_eq!(state.state, AuthState::NeedsReauth);
        assert_eq!(state.reason, "invalid_grant");
    });
}

#[test]
fn expiry_without_a_refresh_token_walks_expiring_then_needs_reauth() {
    with_world(|| {
        let mock = Mock::start(1800, false, &[]);
        let server = remote("clickup", &mock.mcp_url);
        sign_in(&server);

        let outcome = probe("clickup");
        assert!(matches!(outcome, ProbeOutcome::TokenTtl { secs } if (1..=1800).contains(&secs)));
        assert!(matches!(
            tracked(&outcome).state,
            AuthState::Expiring { .. }
        ));
        assert_eq!(mock.grants(), ["authorization_code"]);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        crate::remote::store_oauth_state(
            "clickup",
            Some(mock.origin.clone()),
            &format!("{}/token", mock.origin),
            "FAKE-client-1",
            None,
            Some(mock.mcp_url.clone()),
            None,
            now - 3600,
            Some(now - 10),
        )
        .unwrap();
        let state = tracked(&probe("clickup"));
        assert_eq!(state.state, AuthState::NeedsReauth);
        assert_eq!(state.reason, "token_expired");

        let error = crate::remote::connect_remote(&server).err().unwrap();
        assert!(crate::remote::is_auth_error(&error), "{error}");
        assert_eq!(mock.grants(), ["authorization_code"]);
    });
}

#[test]
fn slack_static_bearer_is_sent_and_never_leaks() {
    with_world(|| {
        let mock = Mock::start(3600, true, &[SLACK_BEARER]);
        let server = remote("slack", &mock.mcp_url);
        crate::secrets::set_secret("slack", "__http_auth__", SLACK_BEARER).unwrap();

        let mut ds = crate::remote::connect_remote(&server).unwrap();
        assert_eq!(ds.tools.len(), 1);
        ds.call("echo", json!({"text": "FAKE-ping"})).unwrap();
        let sent = mock.mcp_auth();
        assert!(!sent.is_empty());
        assert!(sent
            .iter()
            .all(|a| a.as_deref() == Some("Bearer FAKE-slack-bearer")));
        assert_eq!(mock.oauth_hits(), 0);
        assert!(mock.grants().is_empty());

        let outcome = probe("slack");
        assert_eq!(outcome, ProbeOutcome::Success);
        assert_eq!(tracked(&outcome).state, AuthState::Ok);
        assert!(!format!("{outcome:?}{:?}", GatewayStateProbe::default()).contains(SLACK_BEARER));
        assert_not_on_disk(&[SLACK_BEARER]);

        crate::secrets::set_secret("slack", "__http_auth__", "FAKE-wrong-bearer").unwrap();
        let error = crate::remote::connect_remote(&server).err().unwrap();
        assert!(crate::remote::is_auth_error(&error), "{error}");
        assert!(!error.contains("FAKE-wrong-bearer"));
        assert!(!error.contains(SLACK_BEARER));
        assert_eq!(mock.oauth_hits(), 0);
    });
}

#[test]
fn a_remote_without_any_vaulted_token_needs_a_login() {
    with_world(|| {
        let state = tracked(&probe("figma"));
        assert_eq!(state.state, AuthState::NeedsReauth);
        assert_eq!(state.reason, "no_token");
    });
}

#[test]
fn only_remote_servers_get_a_gateway_state_probe() {
    let mut registry = Registry::default();
    let mut local = remote("local-tool", "http://127.0.0.1:1/mcp");
    local.transport = "stdio".into();
    local.url = None;
    local.command = Some("tool".into());
    registry.servers = vec![
        remote("figma", "https://mcp.example.test/mcp"),
        remote("slack", "https://mcp.example.test/slack"),
        local,
    ];
    let probes = gateway_registry(&registry);
    let ids: Vec<&str> = probes.iter().map(|s| s.server.as_str()).collect();
    assert_eq!(ids, ["figma", "slack"]);
    assert!(probes.iter().all(|s| s.kind == ProbeKind::GatewayState));
    let combined = combined_registry(&registry);
    assert_eq!(combined.get("figma").unwrap().kind, ProbeKind::GatewayState);
}
