use serde_json::{json, Value};
use toolport_plus_api::{
    convert, MemoryHost, MemoryRegistry, MemorySecrets, RunDecision, HTTP_AUTH_KEY,
};

use super::{call_tool, ToolError};

const SENTINEL: &str = "SENTINEL-secret-value-5d1c-do-not-print";

fn host() -> MemoryHost {
    let servers = [
        json!({"id": "srv-alpha", "name": "alpha", "enabled": true, "transport": "stdio", "command": "alpha-mcp"}),
        json!({"id": "srv-beta", "name": "beta", "enabled": true, "transport": "http", "url": "https://beta.example.invalid/mcp"}),
    ]
    .iter()
    .map(|fields| convert(fields).unwrap())
    .collect();
    MemoryHost {
        registry: MemoryRegistry::with_servers(servers),
        secrets: MemorySecrets::default(),
        ..Default::default()
    }
}

fn call(host: &MemoryHost, tool: &str, args: Value) -> Result<Value, ToolError> {
    call_tool(host, tool, &args)
}

fn vaulted(host: &MemoryHost, server: &str, key: &str) -> Option<String> {
    host.secrets
        .vault
        .borrow()
        .get(&(server.to_string(), key.to_string()))
        .cloned()
}

fn approved(host: &MemoryHost) {
    host.tasks.decision.set(RunDecision::Approved);
}

#[test]
fn test01_set_previews_by_default_and_names_the_key_not_a_value() {
    let host = host();
    let preview = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": "API_TOKEN"}),
    )
    .unwrap();
    assert_eq!(preview["dryRun"], true);
    assert_eq!(preview["server"], "srv-alpha");
    assert_eq!(preview["key"], "API_TOKEN");
    assert_eq!(preview["replaces"], false);
    assert!(preview["plan"]["summary"]
        .as_str()
        .unwrap()
        .contains("API_TOKEN"));
    assert_eq!(vaulted(&host, "srv-alpha", "API_TOKEN"), None);
    assert!(host.tasks.approvals.borrow().is_empty());
}

#[test]
fn test02_set_refuses_a_value_on_a_preview() {
    let host = host();
    let error = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": "API_TOKEN", "value": SENTINEL}),
    )
    .unwrap_err();
    assert_eq!(error.kind, "invalid_arguments");
    assert!(!error.message.contains(SENTINEL));
    assert_eq!(vaulted(&host, "srv-alpha", "API_TOKEN"), None);
}

#[test]
fn test03_set_needs_confirm_to_apply() {
    let host = host();
    approved(&host);
    let error = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": "API_TOKEN", "value": SENTINEL, "dry_run": false}),
    )
    .unwrap_err();
    assert_eq!(error.kind, "refused");
    assert!(!error.message.contains(SENTINEL));
    assert!(host.tasks.approvals.borrow().is_empty());
    assert_eq!(vaulted(&host, "srv-alpha", "API_TOKEN"), None);
}

#[test]
fn test04_set_without_a_broker_stores_nothing() {
    let host = host();
    let error = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": "API_TOKEN", "value": SENTINEL, "dry_run": false, "confirm": true}),
    )
    .unwrap_err();
    assert_eq!(error.kind, "approval_unavailable");
    assert!(!error.message.contains(SENTINEL));
    assert_eq!(vaulted(&host, "srv-alpha", "API_TOKEN"), None);
}

#[test]
fn test05_set_stores_nothing_when_the_person_denies_or_the_approval_times_out() {
    for decision in [
        RunDecision::Denied,
        RunDecision::Timeout,
        RunDecision::Stale,
    ] {
        let host = host();
        host.tasks.decision.set(decision);
        let error = call(
            &host,
            "secret_set",
            json!({"name": "alpha", "key": "API_TOKEN", "value": SENTINEL, "dry_run": false, "confirm": true}),
        )
        .unwrap_err();
        assert_eq!(error.kind, "approval_denied", "{decision:?}");
        assert_eq!(vaulted(&host, "srv-alpha", "API_TOKEN"), None);
    }
}

#[test]
fn test06_set_applies_after_approval_and_never_carries_the_value() {
    let host = host();
    approved(&host);
    let result = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": "API_TOKEN", "value": SENTINEL, "dry_run": false, "confirm": true}),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"dryRun": false, "server": "srv-alpha", "key": "API_TOKEN", "stored": true, "replaced": false})
    );
    assert_eq!(
        vaulted(&host, "srv-alpha", "API_TOKEN").as_deref(),
        Some(SENTINEL)
    );
    assert!(!result.to_string().contains(SENTINEL));
    let approvals = host.tasks.approvals.borrow();
    assert_eq!(approvals.len(), 1);
    assert!(!format!("{approvals:?}").contains(SENTINEL));
    assert!(format!("{approvals:?}").contains("API_TOKEN"));
}

#[test]
fn test07_every_set_asks_again_and_a_second_value_replaces_the_first() {
    let host = host();
    approved(&host);
    for value in ["first-value-1234", SENTINEL] {
        call(
            &host,
            "secret_set",
            json!({"name": "alpha", "key": "API_TOKEN", "value": value, "dry_run": false, "confirm": true}),
        )
        .unwrap();
    }
    assert_eq!(host.tasks.approvals.borrow().len(), 2);
    assert_eq!(
        vaulted(&host, "srv-alpha", "API_TOKEN").as_deref(),
        Some(SENTINEL)
    );
    let preview = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": "API_TOKEN"}),
    )
    .unwrap();
    assert_eq!(preview["replaces"], true);
}

#[test]
fn test08_set_needs_a_value_to_apply_and_a_remote_server_for_the_access_token() {
    let host = host();
    approved(&host);
    let missing = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": "API_TOKEN", "dry_run": false, "confirm": true}),
    )
    .unwrap_err();
    assert_eq!(missing.kind, "invalid_arguments");
    let local_token = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": HTTP_AUTH_KEY}),
    )
    .unwrap_err();
    assert_eq!(local_token.kind, "invalid_arguments");
    let remote = call(
        &host,
        "secret_set",
        json!({"name": "beta", "key": HTTP_AUTH_KEY, "value": SENTINEL, "dry_run": false, "confirm": true}),
    )
    .unwrap();
    assert_eq!(remote["stored"], true);
    assert_eq!(
        vaulted(&host, "srv-beta", HTTP_AUTH_KEY).as_deref(),
        Some(SENTINEL)
    );
    assert!(host.tasks.approvals.borrow().len() == 1);
}

#[test]
fn test09_an_error_that_carries_the_value_is_masked() {
    let host = host();
    approved(&host);
    *host.secrets.vault_error.borrow_mut() = Some(format!("vault rejected {SENTINEL}"));
    let error = call(
        &host,
        "secret_set",
        json!({"name": "alpha", "key": "API_TOKEN", "value": SENTINEL, "dry_run": false, "confirm": true}),
    )
    .unwrap_err();
    assert!(!error.message.contains(SENTINEL), "{}", error.message);
}

#[test]
fn test10_rm_previews_by_default_and_needs_confirm_and_approval() {
    let host = host();
    approved(&host);
    host.secrets
        .vault
        .borrow_mut()
        .insert(("srv-alpha".into(), "API_TOKEN".into()), SENTINEL.into());
    let preview = call(
        &host,
        "secret_rm",
        json!({"name": "alpha", "key": "API_TOKEN"}),
    )
    .unwrap();
    assert_eq!(preview["dryRun"], true);
    assert_eq!(preview["exists"], true);
    assert!(!preview.to_string().contains(SENTINEL));
    let unconfirmed = call(
        &host,
        "secret_rm",
        json!({"name": "alpha", "key": "API_TOKEN", "dry_run": false}),
    )
    .unwrap_err();
    assert_eq!(unconfirmed.kind, "refused");
    assert!(vaulted(&host, "srv-alpha", "API_TOKEN").is_some());
    assert!(host.tasks.approvals.borrow().is_empty());
}

#[test]
fn test11_rm_removes_only_after_approval() {
    let host = host();
    host.secrets
        .vault
        .borrow_mut()
        .insert(("srv-alpha".into(), "API_TOKEN".into()), SENTINEL.into());
    let apply = json!({"name": "alpha", "key": "API_TOKEN", "dry_run": false, "confirm": true});
    assert_eq!(
        call(&host, "secret_rm", apply.clone()).unwrap_err().kind,
        "approval_unavailable"
    );
    host.tasks.decision.set(RunDecision::Denied);
    assert_eq!(
        call(&host, "secret_rm", apply.clone()).unwrap_err().kind,
        "approval_denied"
    );
    assert!(vaulted(&host, "srv-alpha", "API_TOKEN").is_some());
    approved(&host);
    let done = call(&host, "secret_rm", apply.clone()).unwrap();
    assert_eq!(done["removed"], true);
    assert_eq!(vaulted(&host, "srv-alpha", "API_TOKEN"), None);
    assert_eq!(
        call(&host, "secret_rm", apply).unwrap_err().kind,
        "not_found"
    );
}
