//! The one HTTP surface the fork logic uses.
//!
//! Status is data: a 401, 404 or 500 comes back as an `HttpResponse`, never as
//! an error, so a caller that probes a service sees what it answered. An
//! `HttpError` means no response arrived or the request was refused before it
//! was sent. Conduit implements `HttpAccess` over its own agent, so the egress
//! guard and the resolvers apply to every request.

use std::time::Duration;

use serde_json::Value;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HttpBody {
    Empty,
    Json(Value),
    Form(Vec<(String, String)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: HttpBody,
    /// Deadline for the whole exchange, redirects included.
    pub timeout: Duration,
    /// Follow up to five redirects. Off returns a 3xx response as it is. Headers set on the
    /// request travel only to the origin it names; a redirect to another origin keeps just
    /// `Accept`, `Accept-Language` and `User-Agent`.
    pub follow_redirects: bool,
    /// The response body is cut here and flagged `truncated`.
    pub max_body_bytes: usize,
}

impl HttpRequest {
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: HttpBody::Empty,
            timeout: DEFAULT_TIMEOUT,
            follow_redirects: true,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Get, url)
    }

    pub fn post(url: impl Into<String>, body: HttpBody) -> Self {
        Self {
            body,
            ..Self::new(HttpMethod::Post, url)
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn no_redirects(mut self) -> Self {
        self.follow_redirects = false;
        self
    }

    pub fn max_body_bytes(mut self, max: usize) -> Self {
        self.max_body_bytes = max;
        self
    }

    pub fn has_header(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// True when the body was longer than the request's `max_body_bytes`.
    pub truncated: bool,
    /// Where the response came from, after any redirects.
    pub url: String,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// None when the body is not JSON, or was truncated.
    pub fn json(&self) -> Option<Value> {
        if self.truncated {
            return None;
        }
        serde_json::from_slice(&self.body).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The host is one this build never contacts.
    EgressRefused(String),
    /// No connection: lookup, TCP or TLS setup failed. Nothing was sent.
    Connect(String),
    Timeout(String),
    TooManyRedirects,
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EgressRefused(message) | Self::Other(message) => f.write_str(message),
            Self::Connect(message) => write!(f, "connection failed: {message}"),
            Self::Timeout(message) => write!(f, "timed out: {message}"),
            Self::TooManyRedirects => f.write_str("too many redirects"),
        }
    }
}

impl std::error::Error for HttpError {}

pub trait HttpAccess {
    /// Sends the request. A response of any status is `Ok`.
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.as_bytes().to_vec(),
            truncated: false,
            url: "http://example.test/".into(),
        }
    }

    #[test]
    fn only_2xx_is_success_and_other_statuses_stay_values() {
        assert!(response(204, "").is_success());
        for status in [301, 401, 404, 500] {
            assert!(!response(status, "").is_success());
        }
    }

    #[test]
    fn json_reads_a_body_unless_it_was_cut() {
        assert_eq!(response(200, r#"{"a":1}"#).json(), Some(json!({"a": 1})));
        assert_eq!(response(200, "not json").json(), None);
        let cut = HttpResponse {
            truncated: true,
            ..response(200, r#"{"a":1}"#)
        };
        assert_eq!(cut.json(), None);
    }

    #[test]
    fn header_lookup_ignores_case() {
        assert_eq!(
            response(200, "").header("content-type"),
            Some("application/json")
        );
        assert!(HttpRequest::get("http://x")
            .header("Accept", "a")
            .has_header("accept"));
    }
}
