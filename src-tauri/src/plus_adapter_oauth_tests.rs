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
