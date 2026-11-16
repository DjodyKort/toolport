use std::io::Cursor;

use serde_json::{json, Value};
use toolport_plus_api::{
    convert, MemoryHost, MemoryRegistry, MemorySecrets, ServerRecord, HTTP_AUTH_KEY,
};

use super::{set_from, MAX_VALUE_BYTES};
use crate::ctl::run_with;

const VALUE: &str = "FAKE-vaulted-value-1c9e";

fn server(fields: Value) -> ServerRecord {
    convert(&fields).unwrap()
}

fn host() -> MemoryHost {
    MemoryHost {
        registry: MemoryRegistry::with_servers(vec![
            server(json!({
                "id": "srv-alpha", "name": "alpha", "enabled": true, "transport": "stdio",
                "command": "alpha-mcp",
                "launch": {"inputs": [
                    {"key": "LAUNCH_TOKEN", "secret": true},
                    {"key": "REGION", "secret": false},
                ]},
            })),
            server(json!({
                "id": "srv-beta", "name": "beta", "enabled": true, "transport": "http",
                "url": "https://beta.example.invalid/mcp",
            })),
        ]),
        secrets: MemorySecrets::default(),
        ..Default::default()
    }
}

fn run(host: &MemoryHost, argv: &[&str]) -> (i32, Value, String) {
    let args: Vec<String> = ["--json"]
        .into_iter()
        .chain(argv.iter().copied())
        .map(String::from)
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(host, &args, &mut out, &mut err);
    let text = String::from_utf8(out).unwrap();
    (
        code,
        serde_json::from_str(text.trim()).unwrap_or(Value::Null),
        text,
    )
}

fn set(host: &MemoryHost, argv: &[&str], stdin: &str) -> Result<Value, (String, String)> {
    let rest: Vec<String> = argv.iter().map(|word| word.to_string()).collect();
    set_from(
        host,
        &rest,
        &mut Cursor::new(stdin.as_bytes().to_vec()),
        false,
    )
    .map(|output| output.data)
    .map_err(|error| (error.code().to_string(), error.message))
}

fn vaulted(host: &MemoryHost, server: &str, key: &str) -> Option<String> {
    host.secrets
        .vault
        .borrow()
        .get(&(server.to_string(), key.to_string()))
        .cloned()
}

#[test]
fn test01_set_vaults_the_value_and_declares_the_key_without_the_value() {
    let host = host();
    let data = set(&host, &["alpha", "API_TOKEN"], &format!("{VALUE}\n")).unwrap();
    assert_eq!(
        data,
        json!({"server": "srv-alpha", "key": "API_TOKEN", "stored": true})
    );
    assert_eq!(
        vaulted(&host, "srv-alpha", "API_TOKEN").as_deref(),
        Some(VALUE)
    );
    let env = host.registry.servers.borrow()[0].env().unwrap();
    assert_eq!(env.len(), 1);
    assert!(env[0].secret && env[0].value.is_none() && env[0].key == "API_TOKEN");
    let registry_text = serde_json::to_string(&*host.registry.servers.borrow()).unwrap();
    assert!(!registry_text.contains(VALUE));
}

#[test]
fn test02_set_reads_the_value_from_a_named_variable_when_asked() {
    let host = host();
    host.secrets.set_env("SYNTHETIC_VALUE", VALUE);
    set(
        &host,
        &["alpha", "API_TOKEN", "--value-env", "SYNTHETIC_VALUE"],
        "",
    )
    .unwrap();
    assert_eq!(
        vaulted(&host, "srv-alpha", "API_TOKEN").as_deref(),
        Some(VALUE)
    );
    let (code, message) =
        set(&host, &["alpha", "OTHER", "--value-env", "NOT_SET"], "").unwrap_err();
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("input", "environment variable NOT_SET is not set")
    );
}

#[test]
fn test03_set_refuses_an_unknown_server_an_empty_value_and_a_typed_in_terminal() {
    let host = host();
    let (code, message) = set(&host, &["nope", "API_TOKEN"], VALUE).unwrap_err();
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("not_found", "unknown server: nope")
    );
    let (code, message) = set(&host, &["alpha", "API_TOKEN"], "\n").unwrap_err();
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("input", "value is empty")
    );
    let big = "x".repeat(MAX_VALUE_BYTES as usize + 1);
    let (_, message) = set(&host, &["alpha", "API_TOKEN"], &big).unwrap_err();
    assert_eq!(message, "value is too large");
    let rest = vec!["alpha".to_string(), "API_TOKEN".to_string()];
    let interactive = set_from(&host, &rest, &mut Cursor::new(Vec::new()), true)
        .err()
        .expect("a terminal gives no value");
    assert_eq!(interactive.code(), "usage");
    assert!(host.secrets.vault.borrow().is_empty());
}

#[test]
fn test04_a_key_with_an_equals_sign_a_namespace_or_a_reserved_name_is_a_usage_error() {
    let host = host();
    for key in ["A=B", "a::b", "__oauth_client_secret__", " padded", ""] {
        let (code, message) = set(&host, &["alpha", key], VALUE).unwrap_err();
        assert_eq!(code, "usage", "{key:?}");
        assert!(message.starts_with("invalid secret key"), "{message}");
    }
}

#[test]
fn test05_the_access_token_key_is_for_remote_servers_and_never_declares_an_env_entry() {
    let host = host();
    let (code, message) = set(&host, &["alpha", HTTP_AUTH_KEY], VALUE).unwrap_err();
    assert_eq!(code, "usage");
    assert!(
        message.contains("is the bearer token of a remote server"),
        "{message}"
    );
    set(&host, &["beta", HTTP_AUTH_KEY], VALUE).unwrap();
    assert_eq!(
        vaulted(&host, "srv-beta", HTTP_AUTH_KEY).as_deref(),
        Some(VALUE)
    );
    assert!(host.registry.servers.borrow()[1].env().unwrap().is_empty());
}

#[test]
fn test06_a_declared_secret_launch_input_is_vaulted_without_an_env_declaration() {
    let host = host();
    set(&host, &["alpha", "LAUNCH_TOKEN"], VALUE).unwrap();
    assert_eq!(
        vaulted(&host, "srv-alpha", "LAUNCH_TOKEN").as_deref(),
        Some(VALUE)
    );
    assert!(host.registry.servers.borrow()[0].env().unwrap().is_empty());
    set(&host, &["alpha", "REGION"], VALUE).unwrap();
    assert_eq!(host.registry.servers.borrow()[0].env().unwrap().len(), 1);
}

#[test]
fn test07_get_says_set_without_the_value_and_not_found_when_nothing_is_stored() {
    let host = host();
    set(&host, &["alpha", "API_TOKEN"], VALUE).unwrap();
    let (code, envelope, text) = run(&host, &["secret", "get", "alpha", "API_TOKEN"]);
    assert_eq!(code, 0);
    assert_eq!(
        envelope["data"],
        json!({"server": "srv-alpha", "key": "API_TOKEN", "set": true})
    );
    assert!(!text.contains(VALUE));
    let (code, envelope, _) = run(&host, &["secret", "get", "alpha", "NO_SUCH_KEY"]);
    assert_eq!(code, 1);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_eq!(
        envelope["error"]["message"],
        "NO_SUCH_KEY is not set for srv-alpha"
    );
}

#[test]
fn test08_only_get_reveal_prints_the_value_and_never_for_the_access_token() {
    let host = host();
    set(&host, &["alpha", "API_TOKEN"], VALUE).unwrap();
    set(&host, &["beta", HTTP_AUTH_KEY], VALUE).unwrap();
    let (code, envelope, _) = run(&host, &["secret", "get", "alpha", "API_TOKEN", "--reveal"]);
    assert_eq!((code, envelope["data"]["value"].as_str()), (0, Some(VALUE)));
    let (code, envelope, text) = run(&host, &["secret", "get", "beta", HTTP_AUTH_KEY, "--reveal"]);
    assert_eq!(
        (code, envelope["error"]["code"].as_str()),
        (2, Some("usage"))
    );
    assert!(!text.contains(VALUE));
    let (code, envelope, text) = run(&host, &["secret", "get", "beta", HTTP_AUTH_KEY]);
    assert_eq!((code, envelope["data"]["set"].as_bool()), (0, Some(true)));
    assert!(!text.contains(VALUE));
}

#[test]
fn test09_an_unreadable_vault_is_a_vault_failure_not_an_unset_secret() {
    let host = host();
    set(&host, &["alpha", "API_TOKEN"], VALUE).unwrap();
    *host.secrets.vault_error.borrow_mut() = Some("the keychain is locked".into());
    let (code, envelope, _) = run(&host, &["secret", "get", "alpha", "API_TOKEN"]);
    assert_eq!(code, 1);
    assert_eq!(envelope["error"]["code"], "vault");
    assert_eq!(envelope["error"]["message"], "the keychain is locked");
}

#[test]
fn test10_rm_drops_the_vault_entry_and_the_declaration_of_a_registered_server() {
    let host = host();
    set(&host, &["alpha", "API_TOKEN"], VALUE).unwrap();
    let (code, envelope, _) = run(&host, &["secret", "rm", "alpha", "API_TOKEN"]);
    assert_eq!(code, 0);
    assert_eq!(
        envelope["data"],
        json!({"server": "srv-alpha", "key": "API_TOKEN", "removed": true})
    );
    assert!(vaulted(&host, "srv-alpha", "API_TOKEN").is_none());
    assert!(host.registry.servers.borrow()[0].env().unwrap().is_empty());
}

#[test]
fn test11_rm_of_the_access_token_clears_the_vault_entry() {
    let host = host();
    set(&host, &["beta", HTTP_AUTH_KEY], VALUE).unwrap();
    let (code, _, _) = run(&host, &["secret", "rm", "beta", HTTP_AUTH_KEY]);
    assert_eq!(code, 0);
    assert!(vaulted(&host, "srv-beta", HTTP_AUTH_KEY).is_none());
}

#[test]
fn test12_rm_reaches_a_vault_entry_whose_server_is_gone_by_its_id() {
    let host = host();
    host.secrets.vault.borrow_mut().insert(
        ("srv-gone".to_string(), "OLD_KEY".to_string()),
        VALUE.to_string(),
    );
    let (code, envelope, _) = run(&host, &["secret", "rm", "srv-gone", "OLD_KEY"]);
    assert_eq!(
        (code, envelope["data"]["server"].as_str()),
        (0, Some("srv-gone"))
    );
    assert!(host.secrets.vault.borrow().is_empty());
}

#[test]
fn test13_a_missing_operand_prints_the_usage_line_of_that_command() {
    let host = host();
    for (argv, usage) in [
        (
            vec!["secret", "get"],
            "usage: secret get <server> <KEY> [--reveal]",
        ),
        (vec!["secret", "rm"], "usage: secret rm <server> <KEY>"),
        (
            vec!["secret", "set"],
            "usage: secret set <server> <KEY> [--value-env <VAR>] (value on stdin)",
        ),
    ] {
        let (code, envelope, _) = run(&host, &argv);
        assert_eq!(code, 2, "{argv:?}");
        assert_eq!(envelope["error"]["message"], usage);
    }
}
