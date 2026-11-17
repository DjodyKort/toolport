use std::io::{Read, Write};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::oauth;

struct Mock {
    server: Arc<tiny_http::Server>,
    origin: String,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Mock {
    fn start(resource_suffix: &'static str) -> Mock {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let origin = format!("http://127.0.0.1:{port}");
        let served = Arc::clone(&server);
        let base = origin.clone();
        let worker = std::thread::spawn(move || {
            for mut request in served.incoming_requests() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let path = request.url().to_string();
                let document: Option<Value> = match path.as_str() {
                    "/.well-known/oauth-protected-resource/mcp" => Some(json!({
                        "resource": format!("{base}/mcp{resource_suffix}"),
                        "authorization_servers": [base],
                    })),
                    "/.well-known/oauth-authorization-server" => Some(json!({
                        "issuer": base,
                        "authorization_endpoint": format!("{base}/authorize"),
                        "token_endpoint": format!("{base}/token"),
                        "registration_endpoint": format!("{base}/register"),
                        "token_endpoint_auth_methods_supported": ["none"],
                    })),
                    "/register" => Some(json!({"client_id": "synthetic-client"})),
                    "/token" => Some(json!({
                        "access_token": "synthetic-access",
                        "refresh_token": "synthetic-refresh",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                    })),
                    _ => None,
                };
                let response = match document {
                    Some(document) => tiny_http::Response::from_string(document.to_string())
                        .with_header(
                            tiny_http::Header::from_bytes(b"Content-Type", b"application/json")
                                .unwrap(),
                        )
                        .boxed(),
                    None => tiny_http::Response::from_string("not found")
                        .with_status_code(404)
                        .boxed(),
                };
                let _ = request.respond(response);
            }
        });
        Mock {
            server,
            origin,
            worker: Some(worker),
        }
    }

    fn mcp_url(&self) -> String {
        format!("{}/mcp", self.origin)
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn query_param(url: &str, name: &str) -> String {
    let query = url.split_once('?').unwrap().1;
    let raw = query
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .unwrap();
    urlencoding::decode(raw).unwrap().into_owned()
}

fn follow_redirect(redirect_uri: &str, state: &str) {
    let address = redirect_uri
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap();
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    let request = format!(
        "GET /callback?code=synthetic-code&state={state} HTTP/1.1\r\nHost: {address}\r\n\
         Connection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut sink = Vec::new();
    let _ = stream.read_to_end(&mut sink);
}

#[test]
fn discovery_accepts_the_resource_a_server_publishes_with_a_trailing_slash() {
    let mock = Mock::start("/");
    let endpoints = oauth::discover(&mock.mcp_url()).expect("trailing slash is the same resource");
    assert_eq!(endpoints.issuer, mock.origin);
}

#[test]
fn discovery_still_refuses_a_resource_with_another_path() {
    let mock = Mock::start("/other");
    let error = oauth::discover(&mock.mcp_url()).unwrap_err();
    assert!(error.contains("different resource"), "{error}");
}

#[test]
fn the_sign_in_hands_the_consent_url_to_the_observer_instead_of_the_browser() {
    let mock = Mock::start("/");
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&observed);
    let mcp_url = mock.mcp_url();
    let result = toolport_plus::oauth::consent::with_observer(
        move |url| {
            sink.lock().unwrap().push(url.to_string());
            let redirect_uri = query_param(url, "redirect_uri");
            let state = query_param(url, "state");
            std::thread::spawn(move || follow_redirect(&redirect_uri, &state));
        },
        || oauth::authenticate_cancellable(&mcp_url, None, &oauth::Cancellation::default()),
    )
    .expect("the observer answers the redirect, so the flow completes");
    assert_eq!(result.client_id, "synthetic-client");
    assert_eq!(result.access_token, "synthetic-access");
    let seen = observed.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].starts_with(&format!("{}/authorize?", mock.origin)),
        "{}",
        seen[0]
    );
    assert_eq!(query_param(&seen[0], "client_id"), "synthetic-client");
    assert_eq!(query_param(&seen[0], "resource"), mcp_url);
}

#[test]
fn the_adapter_signs_a_remote_server_in_through_a_consent_sink_and_vaults_the_token() {
    use crate::plus_adapter::HostAdapter;
    use toolport_plus_api::{AuthAccess, VaultAccess, HTTP_AUTH_KEY};

    let mock = Mock::start("/");
    crate::secrets::tests::with_isolated_vault(|| {
        let dir = crate::registry::conduit_dir().unwrap();
        std::fs::write(
            dir.join("registry.json"),
            json!({
                "version": 1,
                "servers": [{"id": "srv-remote", "name": "remote", "transport": "http",
                             "url": mock.mcp_url()}],
                "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
                "activeProfileId": "default",
            })
            .to_string(),
        )
        .unwrap();
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed);
        HostAdapter
            .sign_in_remote(
                "srv-remote",
                Some(Arc::new(move |url: &str| {
                    sink.lock().unwrap().push(url.to_string());
                    let redirect_uri = query_param(url, "redirect_uri");
                    let state = query_param(url, "state");
                    std::thread::spawn(move || follow_redirect(&redirect_uri, &state));
                })),
            )
            .expect("sign-in completes once the consent URL is followed");
        assert_eq!(observed.lock().unwrap().len(), 1);
        assert_eq!(
            HostAdapter
                .read_vault_secret("srv-remote", HTTP_AUTH_KEY)
                .unwrap()
                .as_deref(),
            Some("synthetic-access")
        );
    });
}
