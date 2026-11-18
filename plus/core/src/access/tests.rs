use serde_json::{json, Value};
use toolport_plus_api::{
    convert, AccessSetInfo, MemoryClients, MemoryHost, MemoryRegistry, RegistryAccess,
};

use super::*;
use crate::op::ErrorKind;

fn world() -> MemoryHost {
    let server = |id: &str| {
        convert(
            &json!({"id": id, "name": id.to_uppercase(), "enabled": true,
            "transport": "stdio", "command": "run-it"}),
        )
        .unwrap()
    };
    let set = |id: &str, name: &str, members: &[&str]| AccessSetInfo {
        id: id.into(),
        name: name.into(),
        server_ids: members.iter().map(|m| m.to_string()).collect(),
    };
    let host = MemoryHost {
        registry: MemoryRegistry::with_servers(vec![server("a"), server("b")]),
        clients: MemoryClients::with_client("claude-code", "Claude Code"),
        ..Default::default()
    };
    *host.registry.access_sets.borrow_mut() = vec![
        set("default", "Default", &["a", "b"]),
        set("focus", "Focus", &["a"]),
    ];
    *host.registry.default_access.borrow_mut() = Some("default".into());
    host
}

fn add_args<'a>(name: &'a str, servers: &'a [String], dry_run: bool) -> AddArgs<'a> {
    AddArgs {
        name,
        servers,
        dry_run,
    }
}

fn names(host: &MemoryHost) -> Vec<String> {
    host.registry
        .access_sets
        .borrow()
        .iter()
        .map(|set| set.name.clone())
        .collect()
}

#[test]
fn test01_ls_lists_each_set_with_its_default_clients_and_folders() {
    let host = world();
    host.registry
        .client_scopes
        .borrow_mut()
        .insert("claude-code".into(), "focus".into());
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj".into(), "focus".into()));
    let data = ls(&host).unwrap();
    assert_eq!(data["defaultAccess"], "default");
    let focus = &data["accessSets"][1];
    assert_eq!(focus["clients"], json!(["claude-code"]));
    assert_eq!(focus["folders"], json!(["/work/proj"]));
    assert_eq!(focus["servers"][0]["name"], "A");
    assert_eq!(data["accessSets"][0]["default"], true);
}

#[test]
fn test02_add_previews_then_creates_a_set_with_its_servers_and_names_the_undo() {
    let host = world();
    let servers = vec!["B".to_string()];
    let preview = add(&host, &add_args("Deep Work", &servers, true)).unwrap();
    assert_eq!(preview["dryRun"], true);
    assert_eq!(preview["result"], Value::Null);
    assert_eq!(names(&host), ["Default", "Focus"]);
    let done = add(&host, &add_args("Deep Work", &servers, false)).unwrap();
    assert_eq!(done["accessSet"]["id"], "deep-work");
    assert_eq!(done["accessSet"]["servers"][0]["id"], "b");
    assert_eq!(
        done["result"]["undo"],
        "toolportctl access-set rm 'Deep Work'"
    );
}

#[test]
fn test03_add_refuses_a_duplicate_name_a_reserved_name_and_an_unknown_server() {
    let host = world();
    let none: Vec<String> = Vec::new();
    for (name, kind) in [
        ("focus", ErrorKind::Conflict),
        ("@all", ErrorKind::Usage),
        ("  ", ErrorKind::Usage),
    ] {
        let error = add(&host, &add_args(name, &none, false)).unwrap_err();
        assert_eq!(error.kind, kind, "{name}");
    }
    let unknown = vec!["nope".to_string()];
    let error = add(&host, &add_args("New", &unknown, false)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(names(&host).len(), 2);
}

#[test]
fn test04_add_without_servers_warns_that_the_set_reaches_nothing() {
    let host = world();
    let none: Vec<String> = Vec::new();
    let preview = add(&host, &add_args("Empty", &none, true)).unwrap();
    assert!(preview["plan"]["warnings"][0]
        .as_str()
        .unwrap()
        .contains("reaches nothing"));
}

fn rm_args<'a>(set: &'a str, orphans: Option<Orphans>, dry_run: bool) -> RmArgs<'a> {
    RmArgs {
        set,
        orphans,
        dry_run,
    }
}

#[test]
fn test05_rm_refuses_the_default_access_and_the_last_set() {
    let host = world();
    let error = rm(&host, &rm_args("default", None, false)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert!(error.message.contains("default access"));
    host.registry.access_sets.borrow_mut().remove(0);
    *host.registry.default_access.borrow_mut() = None;
    let error = rm(&host, &rm_args("focus", None, false)).unwrap_err();
    assert!(error.message.contains("only access set"));
}

#[test]
fn test06_rm_of_a_set_clients_use_needs_an_explicit_choice() {
    let host = world();
    host.registry
        .client_scopes
        .borrow_mut()
        .insert("claude-code".into(), "focus".into());
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj".into(), "focus".into()));
    let error = rm(&host, &rm_args("Focus", None, false)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert!(error.message.contains("--detach") && error.message.contains("--leave"));
    assert_eq!(
        error.details.unwrap(),
        json!({"clients": ["claude-code"], "folders": ["/work/proj"]})
    );
    assert_eq!(names(&host).len(), 2);
}

#[test]
fn test07_rm_detach_unbinds_clients_and_folders_and_the_undo_restores_them() {
    let host = world();
    host.registry
        .client_scopes
        .borrow_mut()
        .insert("claude-code".into(), "focus".into());
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj".into(), "focus".into()));
    let done = rm(&host, &rm_args("focus", Some(Orphans::Detach), false)).unwrap();
    assert_eq!(names(&host), ["Default"]);
    assert!(host.registry.client_scopes.borrow().is_empty());
    assert!(host.registry.folder_routes.borrow().is_empty());
    assert_eq!(
        done["result"]["undo"],
        "toolportctl access-set add Focus --server a && toolportctl client scope claude-code Focus && toolportctl folder route add /work/proj Focus"
    );
}

#[test]
fn test08_rm_leave_keeps_the_bindings_dangling_and_says_so() {
    let host = world();
    host.registry
        .client_scopes
        .borrow_mut()
        .insert("claude-code".into(), "focus".into());
    let preview = rm(&host, &rm_args("focus", Some(Orphans::Leave), true)).unwrap();
    assert!(preview["plan"]["warnings"][0]
        .as_str()
        .unwrap()
        .contains("reaches nothing"));
    rm(&host, &rm_args("focus", Some(Orphans::Leave), false)).unwrap();
    assert_eq!(host.registry.client_scopes.borrow()["claude-code"], "focus");
    let view = host.registry.read_view().unwrap().unwrap();
    let scope = view
        .scope_reach()
        .into_iter()
        .find(|s| s.key == "claude-code")
        .unwrap();
    assert!(scope.dangling);
}

#[test]
fn test09_default_sets_and_clears_with_a_plan_and_an_undo() {
    let host = world();
    let preview = default(&host, Change::Set("FOCUS"), true).unwrap();
    assert_eq!(
        preview["plan"]["undo"],
        "toolportctl access-set default Default"
    );
    assert_eq!(
        host.registry.default_access.borrow().as_deref(),
        Some("default")
    );
    default(&host, Change::Set("FOCUS"), false).unwrap();
    assert_eq!(
        host.registry.default_access.borrow().as_deref(),
        Some("focus")
    );
    let cleared = default(&host, Change::Clear, false).unwrap();
    assert!(host.registry.default_access.borrow().is_none());
    assert_eq!(
        cleared["result"]["undo"],
        "toolportctl access-set default Focus"
    );
    let again = default(&host, Change::Clear, false).unwrap();
    assert_eq!(again["result"]["applied"], false);
}

#[test]
fn test10_default_refuses_an_unknown_set() {
    let host = world();
    let error = default(&host, Change::Set("nope"), false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert!(error.message.contains("Default, Focus"));
}

#[test]
fn test11_scope_binds_a_known_client_and_clearing_returns_it_to_the_default() {
    let host = world();
    let done = scope(&host, "claude-code", Change::Set("Focus"), false).unwrap();
    assert_eq!(host.registry.client_scopes.borrow()["claude-code"], "focus");
    assert_eq!(
        done["result"]["undo"],
        "toolportctl client scope claude-code --none"
    );
    assert_eq!(done["accessSet"]["name"], "Focus");
    scope(&host, "claude-code", Change::Clear, false).unwrap();
    assert!(host.registry.client_scopes.borrow().is_empty());
}

#[test]
fn test12_scope_refuses_an_unknown_client_and_an_unknown_set() {
    let host = world();
    let error = scope(&host, "claud-code", Change::Set("Focus"), false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert!(error.message.contains("claude-code"));
    let error = scope(&host, "claude-code", Change::Set("nope"), false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert!(host.registry.client_scopes.borrow().is_empty());
}

#[test]
fn test13_scope_warns_when_a_folder_route_outranks_the_binding() {
    let host = world();
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj".into(), "default".into()));
    let preview = scope(&host, "claude-code", Change::Set("Focus"), true).unwrap();
    assert!(preview["plan"]["warnings"][0]
        .as_str()
        .unwrap()
        .contains("outranks"));
}

#[test]
fn test14_route_add_stores_the_folder_and_replaces_a_spelling_of_the_same_path() {
    let host = world();
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj/".into(), "default".into()));
    let done = route_add(&host, "/work/proj", "Focus", false).unwrap();
    assert_eq!(
        *host.registry.folder_routes.borrow(),
        vec![("/work/proj".to_string(), "focus".to_string())]
    );
    assert_eq!(
        done["result"]["undo"],
        "toolportctl folder route add /work/proj Default"
    );
    let again = route_add(&host, "/work/proj", "focus", false).unwrap();
    assert_eq!(again["result"]["applied"], false);
}

#[test]
fn test15_route_add_refuses_a_missing_set_and_a_relative_folder() {
    let host = world();
    assert_eq!(
        route_add(&host, "/work/proj", "gone", false)
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
    let error = route_add(&host, "proj", "focus", false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Usage);
    assert!(error.message.contains("absolute"));
    assert!(host.registry.folder_routes.borrow().is_empty());
}

#[test]
fn test16_route_rm_removes_the_stored_spelling_and_a_missing_route_is_not_found() {
    let host = world();
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj/".into(), "focus".into()));
    let done = route_rm(&host, "/work/proj", false).unwrap();
    assert!(host.registry.folder_routes.borrow().is_empty());
    assert_eq!(
        done["result"]["undo"],
        "toolportctl folder route add /work/proj Focus"
    );
    assert_eq!(
        route_rm(&host, "/work/proj", false).unwrap_err().kind,
        ErrorKind::NotFound
    );
}

#[test]
fn test17_routes_list_the_folders_with_their_set_and_the_servers_they_reach() {
    let host = world();
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj".into(), "focus".into()));
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/old".into(), "gone".into()));
    let data = routes(&host).unwrap();
    assert_eq!(data["routes"][0]["accessSet"]["name"], "Focus");
    assert_eq!(data["routes"][0]["servers"][0]["name"], "A");
    assert_eq!(data["routes"][1]["missing"], true);
    assert_eq!(data["routes"][1]["accessSet"], Value::Null);
}

#[test]
fn test18_the_reads_answer_empty_without_a_registry_and_the_writes_are_refused() {
    let host = MemoryHost::default();
    assert_eq!(ls(&host).unwrap()["accessSets"], json!([]));
    assert_eq!(routes(&host).unwrap()["routes"], json!([]));
    let error = default(&host, Change::Clear, false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Failed("access_sets"));
}
