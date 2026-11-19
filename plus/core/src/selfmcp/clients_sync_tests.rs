use serde_json::{json, Value};
use toolport_plus_api::{ClientEntryInfo, ClientInfo, MemoryClients, MemoryHost, MemoryRegistry};

use super::{call_tool, ToolError};

const GATEWAY: &str = "/opt/toolport/toolport-gateway";

fn gateway_entry(command: &str, env_keys: &[&str]) -> ClientEntryInfo {
    ClientEntryInfo {
        name: "toolport".into(),
        transport: "stdio".into(),
        command: Some(command.into()),
        env_keys: env_keys.iter().map(|k| k.to_string()).collect(),
        ..Default::default()
    }
}

fn client(id: &str, installed: bool, entries: Vec<ClientEntryInfo>) -> ClientInfo {
    ClientInfo {
        id: id.into(),
        name: id.into(),
        config_path: format!("/home/test/.{id}.json"),
        config_exists: true,
        app_present: true,
        gateway_installed: installed,
        entries,
        ..Default::default()
    }
}

fn host(clients: Vec<ClientInfo>) -> MemoryHost {
    MemoryHost {
        registry: MemoryRegistry::with_servers(Vec::new()),
        clients: MemoryClients {
            clients: std::cell::RefCell::new(clients),
            gateway: Some(GATEWAY.into()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn call(host: &MemoryHost, args: Value) -> Result<Value, ToolError> {
    call_tool(host, "clients_sync", &args)
}

fn states(data: &Value) -> Vec<(String, String)> {
    data["clients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["client"].as_str().unwrap().to_string(),
                row["state"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn drifted() -> MemoryHost {
    host(vec![
        client(
            "claude-code",
            true,
            vec![gateway_entry(
                "/old/path/toolport-gateway",
                &["TOOLPORT_CLIENT_ID"],
            )],
        ),
        client(
            "cursor",
            true,
            vec![gateway_entry(GATEWAY, &["TOOLPORT_CLIENT_ID"])],
        ),
        client("zed", false, vec![]),
    ])
}

#[test]
fn test01_the_preview_lists_what_drifted_with_a_plan_hash_and_writes_nothing() {
    let host = drifted();
    let data = call(&host, json!({})).unwrap();
    assert_eq!(data["dryRun"], true);
    assert_eq!(
        states(&data),
        [
            ("claude-code".to_string(), "stale".to_string()),
            ("cursor".to_string(), "ok".to_string()),
        ]
    );
    assert_eq!(data["plan"]["steps"].as_array().unwrap().len(), 1);
    assert_eq!(data["planHash"].as_str().unwrap().len(), 16);
    assert!(host.clients.writes.borrow().is_empty());
}

#[test]
fn test02_apply_rewrites_only_the_gateway_entry_of_the_drifted_client() {
    let host = drifted();
    let hash = call(&host, json!({})).unwrap()["planHash"]
        .as_str()
        .unwrap()
        .to_string();
    let done = call(
        &host,
        json!({"dry_run": false, "plan_hash": hash, "confirm": true}),
    )
    .unwrap();
    assert_eq!(done["result"]["applied"], true);
    let writes = host.clients.writes.borrow();
    assert_eq!(writes.len(), 1);
    let (client, name, spec) = &writes[0];
    assert_eq!(
        (client.as_str(), name.as_str()),
        ("claude-code", "toolport")
    );
    let spec = spec.as_ref().unwrap();
    assert_eq!(spec.command.as_deref(), Some(GATEWAY));
    assert_eq!(
        spec.env.get("TOOLPORT_CLIENT_ID").map(String::as_str),
        Some("claude-code")
    );
    assert!(spec.args.is_empty());
}

#[test]
fn test03_apply_needs_confirm_and_a_plan_that_still_matches() {
    let host = drifted();
    let unconfirmed = call(&host, json!({"dry_run": false})).unwrap_err();
    assert_eq!(unconfirmed.kind, "refused");
    let stale = call(
        &host,
        json!({"dry_run": false, "confirm": true, "plan_hash": "0000000000000000"}),
    )
    .unwrap_err();
    assert_eq!(stale.kind, "conflict");
    assert!(host.clients.writes.borrow().is_empty());
}

#[test]
fn test04_a_second_run_changes_nothing() {
    let host = host(vec![client(
        "claude-code",
        true,
        vec![gateway_entry(GATEWAY, &["TOOLPORT_CLIENT_ID"])],
    )]);
    let done = call(&host, json!({"dry_run": false, "confirm": true})).unwrap();
    assert_eq!(done["changed"], false);
    assert!(host.clients.writes.borrow().is_empty());
}

#[test]
fn test05_a_client_without_the_entry_is_never_connected() {
    let host = drifted();
    let data = call(&host, json!({"client": "zed"})).unwrap();
    assert_eq!(states(&data), [("zed".to_string(), "skipped".to_string())]);
    let done = call(
        &host,
        json!({"client": "zed", "dry_run": false, "confirm": true}),
    )
    .unwrap();
    assert_eq!(done["changed"], false);
    assert!(host.clients.writes.borrow().is_empty());
}

#[test]
fn test06_scoped_customised_codex_and_connector_clients_are_left_alone() {
    let mut codex = client(
        "codex",
        true,
        vec![gateway_entry("/old", &["TOOLPORT_CLIENT_ID"])],
    );
    codex.name = "Codex".into();
    let mut account = client("desktop", true, vec![gateway_entry("/old", &[])]);
    account.uses_connectors = true;
    let mut unsupported = client("yaml", true, vec![gateway_entry("/old", &[])]);
    unsupported.direct_unsupported = Some("its config is not a JSON server list".into());
    let host = host(vec![
        client(
            "scoped",
            true,
            vec![gateway_entry(
                "/old",
                &["TOOLPORT_CLIENT_ID", "TOOLPORT_PROFILE"],
            )],
        ),
        client(
            "custom",
            true,
            vec![gateway_entry("/old", &["TOOLPORT_CLIENT_ID", "MY_FLAG"])],
        ),
        codex,
        account,
        unsupported,
        client(
            "renamed",
            true,
            vec![ClientEntryInfo {
                name: "gateway".into(),
                ..gateway_entry("/old", &[])
            }],
        ),
    ]);
    let data = call(&host, json!({})).unwrap();
    assert!(states(&data).iter().all(|(_, state)| state == "skipped"));
    assert!(data["clients"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["reason"].is_string()));
    call(&host, json!({"dry_run": false, "confirm": true})).unwrap();
    assert!(host.clients.writes.borrow().is_empty());
}

#[test]
fn test07_an_unknown_client_and_a_missing_gateway_are_errors() {
    let host = drifted();
    assert_eq!(
        call(&host, json!({"client": "ghost"})).unwrap_err().kind,
        "not_found"
    );
    let mut without = drifted();
    without.clients.gateway = None;
    assert_eq!(call(&without, json!({})).unwrap_err().kind, "backend_error");
}

#[test]
fn test08_a_client_file_that_changed_since_the_plan_is_refused() {
    let host = drifted();
    let hash = call(&host, json!({})).unwrap()["planHash"]
        .as_str()
        .unwrap()
        .to_string();
    host.clients.clients.borrow_mut()[0]
        .entries
        .push(ClientEntryInfo {
            name: "added-by-hand".into(),
            ..Default::default()
        });
    let error = call(
        &host,
        json!({"dry_run": false, "confirm": true, "plan_hash": hash}),
    )
    .unwrap_err();
    assert_eq!(error.kind, "conflict");
    assert!(host.clients.writes.borrow().is_empty());
}
