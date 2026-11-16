use serde_json::json;
use toolport_plus_api::{
    convert, AccessSetInfo, CatalogEntryRecord, MemoryHost, MemoryRegistry, RegistryAccess,
    ServerRecord,
};

use super::env::Change;
use super::*;

fn record(value: serde_json::Value) -> ServerRecord {
    convert(&value).unwrap()
}

fn base(id: &str, name: &str, transport: &str) -> ServerRecord {
    record(json!({
        "id": id, "name": name, "enabled": true, "transport": transport,
        "command": (transport == "stdio").then_some("base-mcp"),
        "args": ["--base"],
        "url": (transport != "stdio").then_some("https://base.example.invalid/mcp"),
        "cwd": "/base",
    }))
}

fn host_with(servers: Vec<ServerRecord>) -> MemoryHost {
    let registry = MemoryRegistry::with_servers(servers);
    *registry.access_sets.borrow_mut() = vec![
        AccessSetInfo {
            id: "default".into(),
            name: "Default".into(),
            server_ids: vec![],
        },
        AccessSetInfo {
            id: "focus".into(),
            name: "Focus".into(),
            server_ids: vec![],
        },
    ];
    MemoryHost {
        registry,
        ..Default::default()
    }
}

fn stdio(name: &str) -> Patch {
    Patch {
        name: Some(name.into()),
        command: Some("run-mcp".into()),
        ..Default::default()
    }
}

fn set(key: &str, value: &str) -> Change {
    Change::Set {
        key: key.into(),
        value: value.into(),
    }
}

fn stored(host: &MemoryHost, id: &str) -> ServerRecord {
    host.registry.get_server(id).unwrap().unwrap()
}

#[test]
fn test01_a_server_is_found_by_id_then_by_name_ignoring_case() {
    let all = vec![base("one", "Two", "stdio"), base("two", "Three", "stdio")];
    assert_eq!(find(&all, "two").unwrap().name, "Three");
    assert_eq!(find(&all, "TWO").unwrap().id, "one");
    assert_eq!(find(&all, "three").unwrap().id, "two");
    assert!(find(&all, "ghost").is_none() && find(&all, " two").is_none());
    assert!(named(&all, "one").is_none());
}

#[test]
fn test02_a_switch_reads_on_and_off_spellings_and_nothing_else() {
    for on in ["on", "ON", "true", "Yes", "1", " on "] {
        assert_eq!(parse_switch(on), Ok(true), "{on}");
    }
    for off in ["off", "Off", "false", "no", "0"] {
        assert_eq!(parse_switch(off), Ok(false), "{off}");
    }
    assert_eq!(
        parse_switch("maybe"),
        Err("'maybe' is not on or off".into())
    );
}

#[test]
fn test03_the_transport_follows_the_endpoint_the_patch_sets() {
    let new = |command: Option<&str>, url: Option<&str>, transport: Option<&str>| {
        spec_from(
            &Patch {
                name: Some("n".into()),
                transport: transport.map(String::from),
                command: command.map(String::from),
                url: url.map(String::from),
                ..Default::default()
            },
            None,
        )
        .transport
    };
    assert_eq!(new(Some("run"), None, None), "stdio");
    assert_eq!(new(None, Some("https://x.invalid"), None), "http");
    assert_eq!(new(Some("run"), Some("https://x.invalid"), None), "stdio");
    assert_eq!(new(None, None, None), "stdio");
    assert_eq!(new(None, Some("https://x.invalid"), Some("sse")), "sse");

    let (stdio_base, sse_base) = (base("s", "S", "stdio"), base("e", "E", "sse"));
    let url = Patch {
        url: Some("https://x.invalid".into()),
        ..Default::default()
    };
    assert_eq!(spec_from(&url, Some(&stdio_base)).transport, "http");
    assert_eq!(spec_from(&url, Some(&sse_base)).transport, "sse");
    assert_eq!(
        spec_from(&Patch::default(), Some(&sse_base)).transport,
        "sse"
    );
}

#[test]
fn test04_an_edit_keeps_the_base_values_it_does_not_name() {
    let kept = spec_from(&Patch::default(), Some(&base("b", "Base", "stdio")));
    assert_eq!(
        (
            kept.name.as_str(),
            kept.command.as_deref(),
            kept.args.clone(),
            kept.cwd.as_deref()
        ),
        (
            "Base",
            Some("base-mcp"),
            vec!["--base".to_string()],
            Some("/base")
        )
    );
    assert_eq!(
        kept.inherit_env, None,
        "an edit that does not name it keeps the saved choice"
    );
    let cleared = spec_from(
        &Patch {
            args: Some(vec![]),
            inherit_env: Some(true),
            ..Default::default()
        },
        Some(&base("b", "Base", "stdio")),
    );
    assert!(cleared.args.is_empty() && cleared.inherit_env == Some(true));
}

#[test]
fn test05_an_add_stores_env_and_switches_and_rejects_a_duplicate_name() {
    let host = host_with(vec![]);
    let patch = Patch {
        inherit_env: Some(true),
        forward_instructions: Some(true),
        ..stdio("  Alpha ")
    };
    let id = add(&host, &patch, &[set("JAVA_HOME", "/opt/jdk")]).unwrap();
    let server = stored(&host, &id);
    assert_eq!(server.name, "Alpha");
    assert!(server.switch("inheritEnv"));
    assert_eq!(server.fork.forward_instructions, Some(true));
    assert_eq!(server.env().unwrap()[0].value.as_deref(), Some("/opt/jdk"));
    assert_eq!(
        add(&host, &stdio("ALPHA"), &[]),
        Err(ServerError::Exists),
        "the name check ignores case"
    );
    assert_eq!(host.registry.servers.borrow().len(), 1);
}

#[test]
fn test06_a_refused_env_entry_writes_nothing_and_names_its_rule() {
    let host = host_with(vec![]);
    let refused = add(&host, &stdio("gamma"), &[set("API_KEY", "x")]).unwrap_err();
    assert!(
        matches!(&refused, ServerError::Refused(r) if r.rule == "credential-name"),
        "{refused:?}"
    );
    assert!(host.registry.servers.borrow().is_empty());
    let id = add(&host, &stdio("gamma"), &[set("MODE", "fast")]).unwrap();
    let again = update(
        &host,
        &id,
        &Patch {
            args: Some(vec!["--moved".into()]),
            ..Default::default()
        },
        &[set("GITHUB_TOKEN", "x")],
    );
    assert!(matches!(again, Err(ServerError::Refused(_))));
    assert!(
        stored(&host, &id).strings("args").is_empty(),
        "fields and env commit together or not at all"
    );
}

#[test]
fn test07_an_update_reports_the_env_keys_that_really_changed() {
    let host = host_with(vec![]);
    let id = add(&host, &stdio("rea"), &[set("JAVA_HOME", "/opt/jdk")]).unwrap();
    let changed = update(
        &host,
        &id,
        &Patch {
            args: Some(vec!["--moved".into()]),
            ..Default::default()
        },
        &[
            Change::Unset {
                key: "JAVA_HOME".into(),
            },
            set("MODE", "fast"),
        ],
    )
    .unwrap();
    assert_eq!(changed, ["JAVA_HOME", "MODE"]);
    let server = stored(&host, &id);
    assert_eq!(server.strings("args"), ["--moved"]);
    assert_eq!(server.env().unwrap().len(), 1);
    assert_eq!(
        update(&host, "ghost", &Patch::default(), &[]),
        Err(ServerError::NotFound("No server with id 'ghost'".into()))
    );
}

#[test]
fn test08_inherit_env_is_set_on_a_stdio_server_and_dropped_for_a_remote_one() {
    let host = host_with(vec![]);
    let id = add(&host, &stdio("local"), &[]).unwrap();
    update(
        &host,
        &id,
        &Patch {
            inherit_env: Some(true),
            ..Default::default()
        },
        &[],
    )
    .unwrap();
    assert!(stored(&host, &id).switch("inheritEnv"));
    update(&host, &id, &Patch::default(), &[]).unwrap();
    assert!(
        stored(&host, &id).switch("inheritEnv"),
        "an edit that does not name it keeps it"
    );
    update(
        &host,
        &id,
        &Patch {
            url: Some("https://x.invalid/mcp".into()),
            command: None,
            transport: Some("http".into()),
            inherit_env: Some(true),
            ..Default::default()
        },
        &[],
    )
    .unwrap();
    assert!(
        !stored(&host, &id).switch("inheritEnv"),
        "a remote server has no process"
    );
}

#[test]
fn test09_a_catalog_entry_is_added_once_by_name() {
    let mut host = host_with(vec![]);
    host.catalog = vec![CatalogEntryRecord {
        name: "Stripe".into(),
        transport: "http".into(),
        source: "curated".into(),
        env_keys: vec![],
        extra: json!({"url": "https://stripe.example.invalid/mcp"})
            .as_object()
            .unwrap()
            .clone(),
    }];
    let entry = find_catalog(&host, "stripe").unwrap();
    let id = add_catalog(&host, &entry).unwrap();
    assert_eq!(stored(&host, &id).text("source"), Some("catalog:curated"));
    assert_eq!(add_catalog(&host, &entry), Err(ServerError::Exists));
    assert!(find_catalog(&host, "no-such").is_none());
}

#[test]
fn test10_uninstall_removes_the_server_and_its_vault_entries_unless_kept() {
    let with_secret = record(json!({
        "id": "srv-a", "name": "a", "transport": "stdio", "command": "x",
        "env": [{"key": "TOKEN_A", "secret": true}, {"key": "MODE", "value": "fast", "secret": false}],
    }));
    let host = host_with(vec![with_secret.clone()]);
    let preview = uninstall(
        &host,
        &UninstallArgs {
            key: "a",
            dry_run: true,
            keep_clients: false,
            keep_secrets: false,
        },
    )
    .unwrap();
    assert!(preview.dry_run && preview.secrets_removed.is_empty());
    assert_eq!(host.registry.servers.borrow().len(), 1);
    let done = uninstall(
        &host,
        &UninstallArgs {
            key: "a",
            dry_run: false,
            keep_clients: false,
            keep_secrets: false,
        },
    )
    .unwrap();
    assert_eq!(done.secrets_removed, ["TOKEN_A"]);
    assert_eq!(
        *host.secrets.deleted_secrets.borrow(),
        [("srv-a".to_string(), "TOKEN_A".to_string())]
    );
    assert!(host.registry.servers.borrow().is_empty());

    let kept = host_with(vec![with_secret]);
    uninstall(
        &kept,
        &UninstallArgs {
            key: "srv-a",
            dry_run: false,
            keep_clients: false,
            keep_secrets: true,
        },
    )
    .unwrap();
    assert!(kept.secrets.deleted_secrets.borrow().is_empty());
    assert_eq!(
        uninstall(
            &kept,
            &UninstallArgs {
                key: "srv-a",
                dry_run: false,
                keep_clients: false,
                keep_secrets: false
            }
        )
        .err()
        .unwrap()
        .message,
        "no server 'srv-a'"
    );
}

#[test]
fn test11_membership_is_by_set_id_or_name_and_independent_of_the_global_switch() {
    let mut off = base("srv-b", "beta", "stdio");
    off.enabled = false;
    let host = host_with(vec![off]);
    assert_eq!(
        set_member(&host, "srv-b", "Default", true).unwrap(),
        ["default"]
    );
    assert_eq!(
        set_member(&host, "srv-b", "focus", true).unwrap(),
        ["default", "focus"]
    );
    assert!(
        !stored(&host, "srv-b").enabled,
        "listing a server does not switch it on"
    );
    assert_eq!(
        set_member(&host, "srv-b", "DEFAULT", false).unwrap(),
        ["focus"]
    );
    assert_eq!(
        set_member(&host, "srv-b", "nope", true),
        Err(ServerError::NotFound("access set not found: nope".into()))
    );
    let view = host.read_view().unwrap().unwrap();
    assert_eq!(access_sets_of(&view.access_sets(), "srv-b"), ["focus"]);
}

#[test]
fn test12_the_views_carry_no_secret_value_and_say_what_the_process_sees() {
    let server = record(json!({
        "id": "srv-a", "name": "a", "enabled": true, "transport": "stdio", "command": "x",
        "inheritEnv": true,
        "env": [{"key": "TOKEN_A", "value": "FAKE-VALUE-123456", "secret": true}],
    }));
    let info = info_value(&server, &["default".to_string()]).unwrap();
    assert!(!info.to_string().contains("FAKE-VALUE-123456"));
    assert_eq!(info["env"], json!([{"key": "TOKEN_A", "secret": true}]));
    assert_eq!(info["inheritEnv"], true);
    assert!(child_environment(&server).starts_with("the whole shell environment"));
    let remote =
        record(json!({"id": "r", "name": "r", "transport": "http", "url": "https://x.invalid"}));
    assert_eq!(child_environment(&remote), "not a local process");
    let plain = record(json!({"id": "p", "name": "p", "transport": "stdio", "command": "x"}));
    assert!(child_environment(&plain).starts_with("only a fixed set"));
}
