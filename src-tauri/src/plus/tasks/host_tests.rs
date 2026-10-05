use super::host::run_guarded;
use crate::codemode::Limits;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

type Seen = Arc<Mutex<Vec<String>>>;

fn run(script: &str, servers: &[&str]) -> (Result<Value, String>, Vec<String>) {
    let seen: Seen = Seen::default();
    let log = Arc::clone(&seen);
    let servers: Vec<String> = servers.iter().map(|s| s.to_string()).collect();
    let result = run_guarded(script, Value::Null, Limits::default(), &servers, move |server, tool, _| {
        log.lock().unwrap().push(format!("{server}/{tool}"));
        Ok(json!({"content": [{"type": "text", "text": "reply"}]}))
    });
    let calls = seen.lock().unwrap().clone();
    (result, calls)
}

#[test]
fn a_call_to_a_listed_server_is_made() {
    let (result, calls) = run(r#"return toolport.call("acme__echo", { text: "x" });"#, &["acme", "other"]);
    assert_eq!(result.unwrap()["content"][0]["text"], "reply");
    assert_eq!(calls, ["acme/echo"]);
}

#[test]
fn a_call_to_an_unlisted_server_is_not_made_and_fails_the_step() {
    let (result, calls) = run(r#"var r = toolport.call("rogue__echo", { text: "x" }); return { kept: r };"#, &["acme"]);
    let error = result.unwrap_err();
    assert!(error.contains("\"rogue\"") && error.contains("requires.servers (acme)") && error.contains("not made"), "{error}");
    assert!(calls.is_empty(), "the denied call must not reach the server: {calls:?}");
}

#[test]
fn a_script_that_ignores_the_refusal_cannot_make_later_calls_or_keep_a_result() {
    let script = r#"
var denied = toolport.call("rogue__echo", {});
var later = toolport.call("acme__echo", { text: "after" });
return { denied: denied, later: later };"#;
    let (result, calls) = run(script, &["acme"]);
    assert!(result.unwrap_err().contains("\"rogue\""));
    assert!(calls.is_empty(), "nothing is called after a denial: {calls:?}");
}

#[test]
fn an_allowed_call_before_the_denial_stays_made_and_the_message_names_an_empty_list() {
    let script = r#"toolport.call("acme__echo", {}); toolport.call("rogue__echo", {}); return 1;"#;
    let (result, calls) = run(script, &["acme"]);
    assert!(result.is_err());
    assert_eq!(calls, ["acme/echo"]);
    let (result, calls) = run(r#"return toolport.call("acme__echo", {});"#, &[]);
    assert!(result.unwrap_err().contains("requires.servers (none)"));
    assert!(calls.is_empty());
}

#[test]
fn a_script_without_calls_returns_its_value() {
    let (result, calls) = run("return { sum: 1 + 2 };", &[]);
    assert_eq!(result.unwrap(), json!({"sum": 3}));
    assert!(calls.is_empty());
}
