//! Requests built on `HttpAccess`.

use serde_json::Value;
use toolport_plus_api::{HttpAccess, HttpError, HttpRequest};

/// A reply read as JSON. The status is part of the answer, whatever it is.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonReply {
    pub status: u16,
    /// None when the body is empty, not JSON, or was cut at the request's cap.
    pub json: Option<Value>,
}

impl JsonReply {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Sends `request`, asking for JSON unless the caller set `Accept`. A non-2xx
/// reply is an `Ok` with its status; only a failure to get any reply is `Err`.
pub fn fetch_json(http: &dyn HttpAccess, mut request: HttpRequest) -> Result<JsonReply, HttpError> {
    if !request.has_header("accept") {
        request = request.header("Accept", "application/json");
    }
    let response = http.send(&request)?;
    Ok(JsonReply {
        status: response.status,
        json: response.json(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use toolport_plus_api::{HttpBody, HttpResponse, ScriptedHttp};

    fn reply(status: u16, body: &str) -> Result<HttpResponse, HttpError> {
        Ok(HttpResponse {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
            truncated: false,
            url: "http://example.test/".into(),
        })
    }

    #[test]
    fn a_json_reply_carries_status_and_body() {
        let http = ScriptedHttp::answering(vec![reply(200, r#"{"ok":true}"#)]);
        let reply = fetch_json(&http, HttpRequest::get("http://example.test/")).unwrap();
        assert!(reply.is_success());
        assert_eq!(reply.json, Some(json!({"ok": true})));
    }

    #[test]
    fn error_statuses_come_back_as_values_with_their_body() {
        let http = ScriptedHttp::answering(vec![
            reply(401, r#"{"error":"invalid_token"}"#),
            reply(500, "<html>oops</html>"),
        ]);
        let unauthorized = fetch_json(&http, HttpRequest::get("http://example.test/")).unwrap();
        assert_eq!(unauthorized.status, 401);
        assert_eq!(unauthorized.json, Some(json!({"error": "invalid_token"})));
        let broken = fetch_json(&http, HttpRequest::get("http://example.test/")).unwrap();
        assert_eq!((broken.status, broken.json), (500, None));
    }

    #[test]
    fn transport_failures_stay_errors() {
        let http = ScriptedHttp::answering(vec![Err(HttpError::Connect("refused".into()))]);
        let error = fetch_json(&http, HttpRequest::get("http://example.test/")).unwrap_err();
        assert_eq!(error, HttpError::Connect("refused".into()));
    }

    #[test]
    fn asks_for_json_without_overriding_the_callers_accept() {
        let http = ScriptedHttp::answering(vec![reply(200, "{}"), reply(200, "{}")]);
        fetch_json(&http, HttpRequest::get("http://example.test/")).unwrap();
        fetch_json(
            &http,
            HttpRequest::post("http://example.test/", HttpBody::Empty)
                .header("accept", "text/plain"),
        )
        .unwrap();
        let sent = http.requests.borrow();
        assert_eq!(
            sent[0].headers,
            vec![("Accept".into(), "application/json".into())]
        );
        assert_eq!(
            sent[1].headers,
            vec![("accept".into(), "text/plain".into())]
        );
    }
}
