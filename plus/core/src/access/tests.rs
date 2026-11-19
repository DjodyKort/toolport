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

fn with_data_dir(host: &mut MemoryHost) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    host.secrets.data_dir = Some(dir.path().to_path_buf());
    dir
}

fn removal_files(dir: &tempfile::TempDir) -> usize {
    std::fs::read_dir(dir.path().join("plus/access/removed"))
        .map(|read| read.count())
        .unwrap_or(0)
}

fn token_of(done: &Value) -> String {
    done["result"]["undo"]
        .as_str()
        .unwrap()
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string()
}

fn restore_args<'a>(name: &'a str, token: &'a str, dry_run: bool) -> AddArgs<'a> {
    AddArgs {
        name,
        servers: &[],
        restore: Some(token),
        dry_run,
    }
}

fn add_args<'a>(name: &'a str, servers: &'a [String], dry_run: bool) -> AddArgs<'a> {
    AddArgs {
        name,
        servers,
        restore: None,
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
fn test07_rm_detach_unbinds_clients_and_folders_and_the_undo_line_names_the_removal() {
    let mut host = world();
    let _dir = with_data_dir(&mut host);
    host.registry
        .client_scopes
        .borrow_mut()
        .insert("claude-code".into(), "focus".into());
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj".into(), "focus".into()));
    let preview = rm(&host, &rm_args("focus", Some(Orphans::Detach), true)).unwrap();
    let done = rm(&host, &rm_args("focus", Some(Orphans::Detach), false)).unwrap();
    assert_eq!(names(&host), ["Default"]);
    assert!(host.registry.client_scopes.borrow().is_empty());
    assert!(host.registry.folder_routes.borrow().is_empty());
    let undo = done["result"]["undo"].as_str().unwrap();
    assert!(
        undo.starts_with("toolportctl access-set add Focus --restore removed-"),
        "{undo}"
    );
    assert_eq!(preview["plan"]["undo"], done["result"]["undo"]);
}

#[test]
fn test08_rm_leave_keeps_the_bindings_dangling_and_says_so() {
    let mut host = world();
    let _dir = with_data_dir(&mut host);
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

fn carrying_world() -> (MemoryHost, tempfile::TempDir) {
    let mut host = world();
    let dir = with_data_dir(&mut host);
    host.registry.tool_scopes.borrow_mut().insert(
        "focus".into(),
        [("a".to_string(), vec!["read".to_string()])].into(),
    );
    host.registry
        .instructions
        .borrow_mut()
        .insert("focus".into(), "Keep answers short".into());
    host.registry
        .client_scopes
        .borrow_mut()
        .insert("claude-code".into(), "focus".into());
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj".into(), "focus".into()));
    (host, dir)
}

#[test]
fn test30_the_undo_of_rm_brings_back_tool_lists_instructions_and_detached_bindings() {
    let (host, dir) = carrying_world();
    let done = rm(&host, &rm_args("focus", Some(Orphans::Detach), false)).unwrap();
    assert_eq!(removal_files(&dir), 1);
    assert!(host.registry.tool_scopes.borrow().get("focus").is_none());
    assert!(host.registry.instructions.borrow().is_empty());
    let token = token_of(&done);

    let preview = add(&host, &restore_args("", &token, true)).unwrap();
    let steps: Vec<&str> = preview["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["detail"].as_str().unwrap())
        .collect();
    assert!(
        steps.contains(&"expose 1 tool(s) of A in Focus, as before"),
        "{steps:?}"
    );
    assert!(steps.contains(&"restore the instructions of Focus"));
    assert_eq!(names(&host), ["Default"], "a plan changes nothing");
    assert_eq!(removal_files(&dir), 1);

    let restored = add(&host, &restore_args("Focus", &token, false)).unwrap();
    assert_eq!(names(&host), ["Default", "Focus"]);
    assert_eq!(
        host.registry.tool_scopes.borrow()["focus"]["a"],
        ["read".to_string()]
    );
    assert_eq!(
        host.registry.instructions.borrow()["focus"],
        "Keep answers short"
    );
    assert_eq!(host.registry.client_scopes.borrow()["claude-code"], "focus");
    assert_eq!(
        *host.registry.folder_routes.borrow(),
        [("/work/proj".to_string(), "focus".to_string())]
    );
    assert_eq!(restored["accessSet"]["id"], "focus");
    assert_eq!(
        restored["result"]["undo"],
        "toolportctl access-set rm Focus --detach"
    );
    assert_eq!(removal_files(&dir), 0, "a used record is gone");
    let again = add(&host, &restore_args("", &token, false)).unwrap_err();
    assert_eq!(again.kind, ErrorKind::NotFound);
}

#[test]
fn test31_restore_leaves_bindings_the_person_changed_since_and_says_so() {
    let (host, _dir) = carrying_world();
    let done = rm(&host, &rm_args("focus", Some(Orphans::Detach), false)).unwrap();
    host.registry
        .client_scopes
        .borrow_mut()
        .insert("claude-code".into(), "default".into());
    host.registry
        .folder_routes
        .borrow_mut()
        .push(("/work/proj".into(), "default".into()));
    let restored = add(&host, &restore_args("", &token_of(&done), false)).unwrap();
    assert_eq!(
        host.registry.client_scopes.borrow()["claude-code"],
        "default"
    );
    assert_eq!(host.registry.folder_routes.borrow()[0].1, "default");
    let warnings = restored["plan"]["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 2);
    assert_eq!(
        restored["result"]["undo"],
        "toolportctl access-set rm Focus"
    );
}

#[test]
fn test32_restore_refuses_a_name_that_is_taken_a_wrong_name_and_a_bad_id() {
    let (host, _dir) = carrying_world();
    let done = rm(&host, &rm_args("focus", Some(Orphans::Detach), false)).unwrap();
    let token = token_of(&done);
    let wrong = add(&host, &restore_args("Other", &token, true)).unwrap_err();
    assert_eq!(wrong.kind, ErrorKind::Usage);
    let bad = add(&host, &restore_args("", "../registry", true)).unwrap_err();
    assert_eq!(bad.kind, ErrorKind::Usage);
    let servers = vec!["a".to_string()];
    let mixed = add(
        &host,
        &AddArgs {
            name: "",
            servers: &servers,
            restore: Some(&token),
            dry_run: true,
        },
    )
    .unwrap_err();
    assert_eq!(mixed.kind, ErrorKind::Usage);
    add(&host, &add_args("Focus", &[], false)).unwrap();
    let taken = add(&host, &restore_args("", &token, false)).unwrap_err();
    assert_eq!(taken.kind, ErrorKind::Conflict);
}

#[test]
fn test33_restore_leaves_out_servers_that_are_gone() {
    let (host, _dir) = carrying_world();
    let done = rm(&host, &rm_args("focus", Some(Orphans::Detach), false)).unwrap();
    host.registry
        .servers
        .borrow_mut()
        .retain(|server| server.id != "a");
    let restored = add(&host, &restore_args("", &token_of(&done), false)).unwrap();
    assert!(restored["plan"]["warnings"][0]
        .as_str()
        .unwrap()
        .contains("a no longer exist"));
    assert!(host.registry.access_sets.borrow()[1].server_ids.is_empty());
    assert!(host.registry.tool_scopes.borrow()["focus"].is_empty());
}

#[test]
fn test34_rm_without_a_data_dir_deletes_nothing_and_a_plan_writes_no_record() {
    let mut host = world();
    let error = rm(&host, &rm_args("focus", None, false)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Failed("no_data_dir"));
    assert_eq!(names(&host).len(), 2);
    let dir = with_data_dir(&mut host);
    rm(&host, &rm_args("focus", None, true)).unwrap();
    assert_eq!(removal_files(&dir), 0);
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

fn with_registry_file(host: &mut MemoryHost, scope: Value) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("registry.json");
    std::fs::write(
        &path,
        json!({"profiles": [{"id": "focus", "name": "Focus", "toolScope": scope}]}).to_string(),
    )
    .unwrap();
    host.registry.path = Some(path);
    dir
}

fn only<'a>(tools: &'a [String]) -> ToolsArgs<'a> {
    ToolsArgs {
        set: "Focus",
        server: "a",
        change: ToolChange::Only(tools),
        dry_run: false,
    }
}

#[test]
fn test19_tools_ls_lists_only_the_servers_a_set_narrows() {
    let mut host = world();
    let _dir = with_registry_file(&mut host, json!({"a": ["read", "list"], "b": []}));
    let data = tools_ls(&host, None).unwrap();
    assert_eq!(data["accessSets"][0]["narrowed"], json!([]));
    let focus = tools_ls(&host, Some("focus")).unwrap();
    let narrowed = &focus["accessSets"][0]["narrowed"];
    assert_eq!(narrowed[0]["server"]["name"], "A");
    assert_eq!(narrowed[0]["tools"], json!(["read", "list"]));
    assert_eq!(narrowed[1]["tools"], json!([]));
    assert_eq!(narrowed[1]["listed"], false);
}

#[test]
fn test20_tools_set_previews_then_narrows_and_names_the_command_that_undoes_it() {
    let mut host = world();
    let _dir = with_registry_file(&mut host, json!({"a": ["old"]}));
    let tools = vec!["read".to_string(), " read ".to_string(), "list".to_string()];
    let preview = tools_set(
        &host,
        &ToolsArgs {
            dry_run: true,
            ..only(&tools)
        },
    )
    .unwrap();
    assert_eq!(preview["tools"], json!(["read", "list"]));
    assert!(host.registry.tool_scopes.borrow().is_empty());
    let done = tools_set(&host, &only(&tools)).unwrap();
    assert_eq!(
        host.registry.tool_scopes.borrow()["focus"]["a"],
        ["read", "list"]
    );
    assert_eq!(
        done["result"]["undo"],
        "toolportctl access-set tools Focus A --tool old"
    );
}

#[test]
fn test21_tools_set_all_removes_the_list_and_none_exposes_no_tool() {
    let mut host = world();
    let _dir = with_registry_file(&mut host, json!({}));
    let none = tools_set(&host, &only(&[])).unwrap();
    assert_eq!(host.registry.tool_scopes.borrow()["focus"]["a"].len(), 0);
    assert_eq!(
        none["result"]["undo"],
        "toolportctl access-set tools Focus A --all"
    );
    assert!(none["plan"]["warnings"][0]
        .as_str()
        .unwrap()
        .contains("exposes no tool"));
}

#[test]
fn test21b_tools_set_all_removes_the_list_so_every_tool_is_exposed_again() {
    let mut host = world();
    let _dir = with_registry_file(&mut host, json!({"a": ["x"]}));
    host.registry.tool_scopes.borrow_mut().insert(
        "focus".into(),
        [("a".to_string(), vec!["x".to_string()])].into(),
    );
    let all = tools_set(
        &host,
        &ToolsArgs {
            change: ToolChange::All,
            ..only(&[])
        },
    )
    .unwrap();
    assert_eq!(all["tools"], Value::Null);
    assert_eq!(
        all["result"]["undo"],
        "toolportctl access-set tools Focus A --tool x"
    );
    assert!(host.registry.tool_scopes.borrow().is_empty());
}

#[test]
fn test22_tools_set_refuses_a_server_the_set_does_not_list_an_empty_name_and_a_no_op_writes_nothing(
) {
    let mut host = world();
    let _dir = with_registry_file(&mut host, json!({"a": ["read"]}));
    let tools = vec!["read".to_string()];
    let outside = ToolsArgs {
        server: "b",
        ..only(&tools)
    };
    assert_eq!(
        tools_set(&host, &outside).unwrap_err().kind,
        ErrorKind::Conflict
    );
    let blank = vec![" ".to_string()];
    assert_eq!(
        tools_set(&host, &only(&blank)).unwrap_err().kind,
        ErrorKind::Usage
    );
    let same = tools_set(&host, &only(&tools)).unwrap();
    assert_eq!(same["result"]["applied"], false);
    assert!(host.registry.tool_scopes.borrow().is_empty());
}

fn aged_record(dir: &tempfile::TempDir, token: &str, days: u64) -> std::path::PathBuf {
    let folder = dir.path().join("plus/access/removed");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("{token}.json"));
    std::fs::write(&path, "{}").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    path
}

#[test]
fn test35_rm_and_restore_prune_records_older_than_thirty_days_but_not_the_new_one() {
    let mut host = world();
    let dir = with_data_dir(&mut host);
    let old = aged_record(&dir, "removed-0000000000000000", 31);
    let recent = aged_record(&dir, "removed-1111111111111111", 29);
    let done = rm(&host, &rm_args("focus", None, false)).unwrap();
    assert!(!old.exists(), "a record past 30 days goes");
    assert!(recent.exists(), "a record inside 30 days stays");
    let token = token_of(&done);
    assert!(dir
        .path()
        .join(format!("plus/access/removed/{token}.json"))
        .exists());

    let older = aged_record(&dir, "removed-2222222222222222", 40);
    add(&host, &restore_args("", &token, false)).unwrap();
    assert!(!older.exists(), "a restore prunes too");
    assert!(recent.exists());

    let dry = aged_record(&dir, "removed-3333333333333333", 40);
    rm(&host, &rm_args("focus", None, true)).unwrap();
    assert!(dry.exists(), "a plan changes nothing");
}
