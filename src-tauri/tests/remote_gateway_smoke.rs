//! Gateway smoke with a remote (http) server behind a mock OAuth authorization
//! server (MIG-HRD-1), next to a local stdio server.
//!
//! The mock is one loopback HTTP server that is both the OAuth AS (discovery,
//! dynamic registration, authorize, token) and the protected MCP endpoint. A
//! real sign-in runs in this process (PKCE, loopback callback, `curl` standing
//! in for the browser); the real gateway binary then lists and calls the remote
//! tools with the vaulted token. `MIG-MCP-6` covers the imported stdio registry
//! only, this file adds the remote half.

#![cfg(unix)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use conduit_lib::registry;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[path = "common/exec.rs"]
mod exec_fixture;

const SERVER_ID: &str = "remote-mock";
const LOCAL_ID: &str = "alpha-mock";
const LOCAL_TOOLS: &[&str] = &[
    "echo",
    "add",
    "echo_meta",
    "grow",
    "die",
    "legacy_elicitation",
    "mrtr_confirm",
    "progress_ping",
    "pwd",
];
const REMOTE_TOOLS: &[&str] = &["echo", "reverse"];
const SECRET_KEY: &str = "abababababababababababababababababababababababababababababababab";

struct Cfg {
    access_ttl: u64,
    issue_refresh: bool,
}

#[derive(Default)]
struct State {
    access: HashSet<String>,
    refresh: HashSet<String>,
    codes: Vec<(String, String)>,
    counter: u32,
    revoked: bool,
    grants: Vec<String>,
    registrations: u32,
    pkce_ok: Vec<bool>,
    mcp_auth: Vec<Option<String>>,
    mcp_methods: Vec<String>,
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
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Mock {
    fn start(access_ttl: u64, issue_refresh: bool) -> Mock {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let origin = format!("http://127.0.0.1:{port}");
        let mcp_url = format!("{origin}/mcp");
        let cfg = Arc::new(Mutex::new(Cfg {
            access_ttl,
            issue_refresh,
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

    fn revoke(&self) {
        self.state.lock().unwrap().revoked = true;
    }

    fn grants(&self) -> Vec<String> {
        self.state.lock().unwrap().grants.clone()
    }

    fn mcp_auth(&self) -> Vec<Option<String>> {
        self.state.lock().unwrap().mcp_auth.clone()
    }

    fn methods(&self) -> Vec<String> {
        self.state.lock().unwrap().mcp_methods.clone()
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

fn mcp_reply(req: &Value) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let text = req["params"]["arguments"]["text"].as_str().unwrap_or("");
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
            json!({"jsonrpc": "2.0", "id": id, "result": {"tools": [
                {"name": "echo", "description": "Echo the text",
                 "inputSchema": {"type": "object",
                                 "properties": {"text": {"type": "string"}}}},
                {"name": "reverse", "description": "Reverse the text",
                 "inputSchema": {"type": "object",
                                 "properties": {"text": {"type": "string"}}}}
            ]}}),
        ),
        (Some(id), Some("tools/call")) => {
            let reply = match req["params"]["name"].as_str() {
                Some("reverse") => text.chars().rev().collect::<String>(),
                _ => text.to_string(),
            };
            json_reply(
                200,
                json!({"jsonrpc": "2.0", "id": id, "result": {
                    "content": [{"type": "text", "text": reply}]
                }}),
            )
        }
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
            let valid = token.is_some_and(|t| st.access.contains(t) && !st.revoked);
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
                if let Some(method) = req["method"].as_str() {
                    st.mcp_methods.push(method.to_string());
                }
                mcp_reply(&req)
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
            st.registrations += 1;
            json_reply(201, json!({"client_id": "FAKE-client-1"}))
        }
        "/authorize" => {
            let q: HashMap<String, String> = parsed
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
                let code = format!("FAKE-code-{}", st.codes.len() + 1);
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
            let form: HashMap<String, String> = url::form_urlencoded::parse(body.as_bytes())
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
                    if !st.revoked && st.refresh.remove(&presented) {
                        json_reply(200, issue(&cfg, &mut st))
                    } else {
                        json_reply(400, json!({"error": "invalid_grant"}))
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

struct World {
    base: PathBuf,
    data: PathBuf,
    home: PathBuf,
    _override: registry::DataDirOverride,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl World {
    fn new(tag: &str) -> Self {
        let lock = registry::data_dir_test_lock();
        std::env::set_var("TOOLPORT_SECRET_KEY", SECRET_KEY);
        let base =
            std::env::temp_dir().join(format!("remote-gateway-smoke-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("data");
        let home = base.join("home");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let guard = registry::DataDirOverride::set(&data);
        Self {
            base,
            data,
            home,
            _override: guard,
            _lock: lock,
        }
    }

    fn write_registry(&self, mcp_url: &str) {
        registry::update(|reg| {
            reg.set_lazy_discovery(false);
            reg.servers.push(
                serde_json::from_value(json!({
                    "id": LOCAL_ID, "name": LOCAL_ID, "transport": "stdio",
                    "command": env!("CARGO_BIN_EXE_mock-mcp-server"), "args": []
                }))
                .unwrap(),
            );
            reg.servers.push(
                serde_json::from_value(json!({
                    "id": SERVER_ID, "name": SERVER_ID, "transport": "http", "url": mcp_url
                }))
                .unwrap(),
            );
            let active = reg.active_profile_id.clone();
            for p in reg
                .profiles
                .iter_mut()
                .filter(|p| Some(&p.id) == active.as_ref())
            {
                p.enabled_server_ids = vec![LOCAL_ID.to_string(), SERVER_ID.to_string()];
            }
            Ok(())
        })
        .expect("write the registry");
        let reg: Value = serde_json::from_str(
            &std::fs::read_to_string(self.data.join("registry.json")).unwrap(),
        )
        .unwrap();
        let enabled = reg["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["enabledServerIds"] == json!([LOCAL_ID, SERVER_ID]));
        assert!(enabled, "the active profile enables both servers: {reg}");
    }

    fn sign_in(&self, mcp_url: &str) {
        let bin = self.base.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["xdg-open", "open"] {
            let opener = bin.join(name);
            exec_fixture::write_executable(
                &opener,
                "#!/bin/sh\ncurl -s -o /dev/null -L --max-time 10 \"$1\" &\n",
            );
        }
        let old = std::env::var_os("PATH");
        let _restore = PathGuard(old.clone());
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&old.unwrap_or_default()));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());

        let result = conduit_lib::oauth::authenticate(mcp_url).expect("interactive sign-in");
        conduit_lib::remote::store_oauth_state(
            SERVER_ID,
            Some(result.issuer),
            &result.token_endpoint,
            &result.client_id,
            result.refresh_token,
            Some(mcp_url.to_string()),
            result.scope,
            result.issued_at,
            result.expires_at,
        )
        .expect("store the oauth state");
        conduit_lib::secrets::set_secret(
            SERVER_ID,
            conduit_lib::secrets::HTTP_AUTH_KEY,
            &result.access_token,
        )
        .expect("vault the access token");
    }

    fn assert_tokens_not_on_disk(&self, tokens: &[&str]) {
        let mut text = String::new();
        collect_text(&self.data, &mut text);
        for token in tokens {
            assert!(!text.contains(token), "{token} found in the data dir");
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn collect_text(dir: &Path, out: &mut String) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_text(&path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
}

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    dir: PathBuf,
    next_id: i64,
}

impl Client {
    fn start(world: &World) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_toolport-gateway"))
            .arg("--stdio-adapter")
            .env("HOME", &world.home)
            .env("TOOLPORT_DATA_DIR", &world.data)
            .env("TOOLPORT_REGISTRY", world.data.join("registry.json"))
            .env("TOOLPORT_SECRET_KEY", SECRET_KEY)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the adapter");
        let stdin = child.stdin.take().expect("adapter stdin");
        let stdout = child.stdout.take().expect("adapter stdout");
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            dir: world.data.clone(),
            next_id: 0,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{message}").expect("write request");
        self.stdin.flush().expect("flush request");
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .unwrap_or_else(|e| panic!("no answer to {method}: {e}"));
            let value: Value = serde_json::from_str(&line).expect("json line");
            if value["id"] == id {
                return value;
            }
        }
    }

    fn handshake(&mut self) -> Value {
        let init = self.request(
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "remote-smoke", "version": "1"}}),
        );
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .expect("write notification");
        self.stdin.flush().expect("flush notification");
        init
    }

    fn tool_names(&mut self) -> BTreeSet<String> {
        let list = self.request("tools/list", json!({}));
        list["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("tools array: {list}"))
            .iter()
            .filter_map(|t| t["name"].as_str())
            .filter(|n| !n.starts_with("toolport_"))
            .map(String::from)
            .collect()
    }

    fn call(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": args}))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !(name.starts_with("daemon-") && name.ends_with(".json")) {
                    continue;
                }
                let pid = std::fs::read_to_string(entry.path())
                    .ok()
                    .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                    .and_then(|v| v["pid"].as_u64());
                if let Some(pid) = pid {
                    let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
                }
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn expected_names() -> BTreeSet<String> {
    let local = LOCAL_TOOLS.iter().map(|t| format!("alpha_mock__{t}"));
    let remote = REMOTE_TOOLS.iter().map(|t| format!("remote_mock__{t}"));
    local.chain(remote).collect()
}

fn text_of(reply: &Value) -> &str {
    reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content in {reply}"))
}

fn bearer_values(mock: &Mock) -> Vec<String> {
    mock.mcp_auth().into_iter().flatten().collect()
}

#[test]
fn gateway_lists_and_calls_remote_tools_behind_the_oauth_server() {
    let world = World::new("list");
    let mock = Mock::start(30 * 86_400, true);
    world.write_registry(&mock.mcp_url);
    world.sign_in(&mock.mcp_url);
    {
        let st = mock.state.lock().unwrap();
        assert_eq!(st.registrations, 1);
        assert_eq!(st.pkce_ok, [true]);
    }
    assert_eq!(mock.grants(), ["authorization_code"]);

    let mut client = Client::start(&world);
    client.handshake();
    assert_eq!(client.tool_names(), expected_names());

    let echoed = client.call("remote_mock__echo", json!({"text": "FAKE-ping"}));
    assert_eq!(text_of(&echoed), "FAKE-ping", "{echoed}");
    let reversed = client.call("remote_mock__reverse", json!({"text": "abc"}));
    assert_eq!(text_of(&reversed), "cba", "{reversed}");
    let local = client.call("alpha_mock__echo", json!({"text": "FAKE-local"}));
    assert_eq!(text_of(&local), "FAKE-local", "{local}");

    let methods = mock.methods();
    for expected in ["initialize", "tools/list", "tools/call"] {
        assert!(methods.iter().any(|m| m == expected), "{methods:?}");
    }
    let bearers = bearer_values(&mock);
    assert!(!bearers.is_empty());
    assert!(
        bearers.iter().all(|a| a == "Bearer FAKE-access-1"),
        "{bearers:?}"
    );
    assert_eq!(
        mock.grants(),
        ["authorization_code"],
        "a fresh token needs no refresh"
    );
    world.assert_tokens_not_on_disk(&["FAKE-access-1", "FAKE-refresh-1"]);
}

#[test]
fn gateway_refreshes_an_expiring_token_against_the_mock_server() {
    let world = World::new("refresh");
    let mock = Mock::start(30, true);
    world.write_registry(&mock.mcp_url);
    world.sign_in(&mock.mcp_url);
    mock.set_ttl(3600);
    let before = mock.mcp_auth().len();

    let mut client = Client::start(&world);
    client.handshake();
    assert_eq!(client.tool_names(), expected_names());
    let reply = client.call("remote_mock__echo", json!({"text": "FAKE-ping"}));
    assert_eq!(text_of(&reply), "FAKE-ping", "{reply}");

    assert_eq!(mock.grants(), ["authorization_code", "refresh_token"]);
    let used: Vec<String> = mock
        .mcp_auth()
        .split_off(before)
        .into_iter()
        .flatten()
        .collect();
    assert!(!used.is_empty());
    assert!(
        used.iter().all(|a| a == "Bearer FAKE-access-2"),
        "the gateway presents only the refreshed token: {used:?}"
    );
    world.assert_tokens_not_on_disk(&["FAKE-access-1", "FAKE-access-2", "FAKE-refresh-2"]);
}

#[test]
fn a_revoked_remote_drops_out_while_the_local_tools_keep_working() {
    let world = World::new("revoked");
    let mock = Mock::start(3600, true);
    world.write_registry(&mock.mcp_url);
    world.sign_in(&mock.mcp_url);
    mock.revoke();

    let mut client = Client::start(&world);
    client.handshake();
    let names = client.tool_names();
    let local: BTreeSet<String> = LOCAL_TOOLS
        .iter()
        .map(|t| format!("alpha_mock__{t}"))
        .collect();
    assert!(local.is_subset(&names), "{names:?}");
    assert!(
        !names.iter().any(|n| n.starts_with("remote_mock__")),
        "a server that cannot authenticate lists no tools: {names:?}"
    );
    let reply = client.call("alpha_mock__echo", json!({"text": "FAKE-pong"}));
    assert_eq!(text_of(&reply), "FAKE-pong", "{reply}");
    assert!(
        !mock.mcp_auth().is_empty(),
        "the gateway did try the revoked remote"
    );
    assert!(
        !mock.methods().iter().any(|m| m == "tools/call"),
        "no tool call reached the revoked remote"
    );
    world.assert_tokens_not_on_disk(&["FAKE-access-1", "FAKE-refresh-1"]);
}
