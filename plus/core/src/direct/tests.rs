use serde_json::json;
use toolport_plus_api::{
    convert, ClientEntryInfo, MemoryClients, MemoryHost, MemoryRegistry, RegistryAccess,
    ServerRecord,
};

use super::*;

fn server(value: serde_json::Value) -> ServerRecord {
    convert(&value).unwrap()
}

fn stdio(id: &str, name: &str) -> ServerRecord {
    server(
        json!({"id": id, "name": name, "enabled": true, "transport": "stdio", "command": "run-it"}),
    )
}

fn host_with(servers: Vec<ServerRecord>) -> MemoryHost {
    MemoryHost {
        registry: MemoryRegistry::with_servers(servers),
        clients: MemoryClients::with_client("claude-code", "Claude Code"),
        ..Default::default()
    }
}

fn args<'a>(server: &'a str) -> Args<'a> {
    Args {
        server,
        client: "claude-code",
        ..Default::default()
    }
}

fn entry_names(host: &MemoryHost) -> Vec<String> {
    host.clients.clients.borrow()[0]
        .entries
        .iter()
        .map(|e| e.name.clone())
        .collect()
}

#[test]
fn test01_add_writes_a_launcher_entry_and_records_it() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    let done = add(&host, &args("alpha")).unwrap();
    assert_eq!(done["action"], "added");
    assert_eq!(entry_names(&host), vec!["alpha"]);
    let written = host.clients.writes.borrow()[0].2.clone().unwrap();
    assert_eq!(written.args, vec!["direct", "run", "srv-a"]);
    assert_eq!(
        written.command.as_deref(),
        Some("/opt/toolport/toolportctl")
    );
    let record = host.plus_data().unwrap().direct_entries.unwrap()["claude-code"]["alpha"].clone();
    assert_eq!(record.server, "srv-a");
    assert_eq!(record.path.as_deref(), Some("/home/test/.claude-code.json"));
    assert_eq!(done["result"]["applied"], true);
}

#[test]
fn test02_a_plan_writes_nothing_and_its_hash_applies_it() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    let plan = add(
        &host,
        &Args {
            dry_run: true,
            ..args("alpha")
        },
    )
    .unwrap();
    assert!(host.clients.writes.borrow().is_empty());
    assert!(host.plus_data().unwrap().direct_entries.is_none());
    assert_eq!(plan["result"], serde_json::Value::Null);
    let hash = plan["planHash"].as_str().unwrap().to_string();
    let done = add(
        &host,
        &Args {
            plan_hash: Some(&hash),
            ..args("alpha")
        },
    )
    .unwrap();
    assert_eq!(done["action"], "added");
}

#[test]
fn test03_a_plan_hash_is_refused_when_the_client_file_changed_since() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    let plan = add(
        &host,
        &Args {
            dry_run: true,
            ..args("alpha")
        },
    )
    .unwrap();
    let hash = plan["planHash"].as_str().unwrap().to_string();
    host.clients.clients.borrow_mut()[0]
        .entries
        .push(ClientEntryInfo {
            name: "edited".into(),
            ..Default::default()
        });
    let error = add(
        &host,
        &Args {
            plan_hash: Some(&hash),
            ..args("alpha")
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "conflict");
    assert!(host.clients.writes.borrow().is_empty());
}

#[test]
fn test04_adding_twice_is_unchanged_and_writes_once() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    add(&host, &args("alpha")).unwrap();
    let again = add(&host, &args("alpha")).unwrap();
    assert_eq!(again["action"], "unchanged");
    assert_eq!(again["changed"], false);
    assert_eq!(host.clients.writes.borrow().len(), 1);
}

#[test]
fn test05_an_entry_of_the_same_name_that_toolport_did_not_write_is_refused_without_force() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    host.clients.clients.borrow_mut()[0]
        .entries
        .push(ClientEntryInfo {
            name: "alpha".into(),
            transport: "stdio".into(),
            command: Some("their-own".into()),
            ..Default::default()
        });
    let error = add(&host, &args("alpha")).unwrap_err();
    assert_eq!(error.code(), "conflict");
    assert!(error.message.contains("did not write"));
    let forced = add(
        &host,
        &Args {
            force: true,
            ..args("alpha")
        },
    )
    .unwrap();
    assert_eq!(forced["action"], "updated");
}

#[test]
fn test06_a_remote_server_gets_its_address_only() {
    let host = host_with(vec![server(json!({
        "id": "srv-r", "name": "figma", "enabled": true, "transport": "http",
        "url": "https://example.invalid/mcp"
    }))]);
    let done = add(&host, &args("figma")).unwrap();
    assert_eq!(done["remote"]["url"], "https://example.invalid/mcp");
    let written = host.clients.writes.borrow()[0].2.clone().unwrap();
    assert!(written.command.is_none());
    assert!(written.env.is_empty());
}

#[test]
fn test07_servers_that_need_the_gateway_are_refused_with_a_reason() {
    let host = host_with(vec![
        server(
            json!({"id": "srv-o", "name": "oauthed", "enabled": true, "transport": "http",
            "url": "https://example.invalid/mcp", "clientCredentials": {"clientId": "x"}}),
        ),
        server(
            json!({"id": "srv-c", "name": "rooted", "enabled": true, "transport": "stdio",
            "command": "x", "cwd": "${ROOT}/sub"}),
        ),
    ]);
    for (key, reason) in [("oauthed", "sign-in is managed"), ("rooted", "${ROOT}")] {
        let error = add(&host, &args(key)).unwrap_err();
        assert_eq!(error.code(), "refused", "{key}");
        assert!(error.message.contains(reason), "{}", error.message);
    }
    assert!(host.clients.writes.borrow().is_empty());
}

#[test]
fn test08_a_client_that_cannot_hold_an_entry_reports_why() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    host.clients.clients.borrow_mut()[0].direct_unsupported = Some(
        "Goose cannot hold a direct entry: its config is not a JSON or TOML server list".into(),
    );
    let error = add(&host, &args("alpha")).unwrap_err();
    assert_eq!(error.code(), "unsupported");
    assert!(error.message.contains("cannot hold a direct entry"));
}

#[test]
fn test09_the_entry_carries_no_secret_value() {
    let host = host_with(vec![server(json!({
        "id": "srv-s", "name": "keyed", "enabled": true, "transport": "stdio", "command": "x",
        "env": [{"key": "API_KEY", "value": "FAKE-VALUE-9", "secret": true}]
    }))]);
    let done = add(&host, &args("keyed")).unwrap();
    let everything = format!("{done}{:?}", host.clients.writes.borrow());
    assert!(!everything.contains("FAKE-VALUE-9"), "{everything}");
}

#[test]
fn test10_remove_takes_the_entry_and_its_record_and_leaves_others() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    host.clients.clients.borrow_mut()[0]
        .entries
        .push(ClientEntryInfo {
            name: "native".into(),
            ..Default::default()
        });
    add(&host, &args("alpha")).unwrap();
    let done = remove(&host, &args("alpha")).unwrap();
    assert_eq!(done["action"], "removed");
    assert_eq!(entry_names(&host), vec!["native"]);
    assert!(host.plus_data().unwrap().direct_entries.is_none());
    assert_eq!(remove(&host, &args("alpha")).unwrap()["action"], "absent");
}

#[test]
fn test11_remove_refuses_a_hand_edited_entry_without_force() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    add(&host, &args("alpha")).unwrap();
    host.clients.clients.borrow_mut()[0].entries[0].args = vec!["mine".into()];
    let error = remove(&host, &args("alpha")).unwrap_err();
    assert_eq!(error.code(), "conflict");
    assert!(remove(
        &host,
        &Args {
            force: true,
            ..args("alpha")
        }
    )
    .is_ok());
}

#[test]
fn test12_states_cover_ok_customized_missing_orphan_unrecorded_and_relocated() {
    let mut host = host_with(vec![
        stdio("srv-a", "alpha"),
        stdio("srv-b", "beta"),
        stdio("srv-c", "gamma"),
    ]);
    host.clients.launcher = Some(std::env::current_exe().unwrap().display().to_string());
    for key in ["alpha", "beta", "gamma"] {
        add(&host, &args(key)).unwrap();
    }
    {
        let mut clients = host.clients.clients.borrow_mut();
        clients[0]
            .entries
            .iter_mut()
            .find(|e| e.name == "beta")
            .unwrap()
            .args = vec!["x".into()];
        clients[0].entries.retain(|e| e.name != "gamma");
        clients[0].entries.push(ClientEntryInfo {
            name: "stray".into(),
            transport: "stdio".into(),
            command: Some("/usr/bin/toolportctl".into()),
            args: vec!["direct".into(), "run".into(), "srv-a".into()],
            ..Default::default()
        });
    }
    host.registry
        .servers
        .borrow_mut()
        .retain(|s| s.id != "srv-b");
    let states: Vec<(String, &str)> = assess(&host, None)
        .unwrap()
        .into_iter()
        .map(|row| (row.entry, row.state.as_str()))
        .collect();
    assert_eq!(
        states,
        vec![
            ("alpha".to_string(), "ok"),
            ("beta".to_string(), "orphan"),
            ("gamma".to_string(), "missing"),
            ("stray".to_string(), "unrecorded"),
        ]
    );
    host.clients.clients.borrow_mut()[0].config_path = "/home/test/moved.json".into();
    let moved = assess(&host, None).unwrap();
    assert_eq!(
        moved.iter().find(|r| r.entry == "alpha").unwrap().state,
        State::Relocated
    );
}

#[test]
fn test13_an_orphan_entry_can_be_removed_after_its_server_is_gone() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    add(&host, &args("alpha")).unwrap();
    host.registry.servers.borrow_mut().clear();
    assert_eq!(assess(&host, None).unwrap()[0].state, State::Orphan);
    let done = remove(&host, &args("alpha")).unwrap();
    assert_eq!(done["action"], "removed");
    assert!(entry_names(&host).is_empty());
}

#[test]
fn test14_uninstall_cleanup_removes_recorded_and_unrecorded_launcher_entries_only() {
    let host = host_with(vec![stdio("srv-a", "alpha"), stdio("srv-b", "beta")]);
    add(&host, &args("alpha")).unwrap();
    add(&host, &args("beta")).unwrap();
    let target = host.get_server("srv-a").unwrap().unwrap();
    let planned = remove_for_server(&host, &target, true).unwrap();
    assert_eq!(planned.len(), 1);
    assert_eq!(entry_names(&host).len(), 2);
    let done = remove_for_server(&host, &target, false).unwrap();
    assert_eq!(done[0]["action"], "removed");
    assert_eq!(entry_names(&host), vec!["beta"]);
}

#[test]
fn test15_a_launcher_that_moved_reads_as_stale() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    add(&host, &args("alpha")).unwrap();
    assert_eq!(assess(&host, None).unwrap()[0].state, State::Stale);
}

#[test]
fn test16_check_is_unhealthy_while_any_entry_drifted() {
    let mut host = host_with(vec![stdio("srv-a", "alpha")]);
    host.clients.launcher = Some(std::env::current_exe().unwrap().display().to_string());
    add(&host, &args("alpha")).unwrap();
    assert!(check(&host, None).unwrap().1);
    host.clients.clients.borrow_mut()[0].entries.clear();
    let (rows, healthy) = check(&host, Some("claude-code")).unwrap();
    assert!(!healthy);
    assert_eq!(rows[0].state, State::Missing);
}

fn uninstall_args(key: &str, keep_clients: bool) -> crate::servers::UninstallArgs<'_> {
    crate::servers::UninstallArgs {
        key,
        dry_run: false,
        keep_clients,
        keep_secrets: false,
    }
}

#[test]
fn test17_uninstall_takes_the_servers_direct_entries_with_it() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    add(&host, &args("alpha")).unwrap();
    let done = crate::servers::uninstall(&host, &uninstall_args("alpha", false)).unwrap();
    assert_eq!(done.clients[0]["action"], "removed");
    assert!(entry_names(&host).is_empty());
    assert!(host.plus_data().unwrap().direct_entries.is_none());
}

#[test]
fn test18_keep_clients_leaves_the_entry_and_its_record_and_shows_an_orphan() {
    let host = host_with(vec![stdio("srv-a", "alpha")]);
    add(&host, &args("alpha")).unwrap();
    let done = crate::servers::uninstall(&host, &uninstall_args("alpha", true)).unwrap();
    assert!(done.clients.is_empty());
    assert_eq!(entry_names(&host), vec!["alpha"]);
    assert_eq!(assess(&host, None).unwrap()[0].state, State::Orphan);
}
