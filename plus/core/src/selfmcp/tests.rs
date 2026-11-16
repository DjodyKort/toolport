use serde_json::{json, Value};
use toolport_plus_api::MemoryHost;

use super::{call_tool, handle_message, read_resource, tools_list, Gate, RESOURCES, TOOLS};

fn host() -> MemoryHost {
    MemoryHost {
        registry: toolport_plus_api::MemoryRegistry::with_servers(Vec::new()),
        seam: true,
        ..Default::default()
    }
}

#[test]
fn tool_names_are_unique_and_every_gated_tool_exposes_confirm() {
    let names: std::collections::BTreeSet<&str> = TOOLS.iter().map(|t| t.name).collect();
    assert_eq!(names.len(), TOOLS.len());
    for descriptor in tools_list().as_array().unwrap() {
        let name = descriptor["name"].as_str().unwrap();
        let tool = TOOLS.iter().find(|t| t.name == name).unwrap();
        assert_eq!(
            descriptor["inputSchema"]["properties"]
                .get("confirm")
                .is_some(),
            tool.gate != Gate::None,
            "{name}"
        );
    }
}

#[test]
fn a_call_validates_arguments_before_it_runs() {
    let host = host();
    for (args, kind) in [
        (json!({"x": 1}), "invalid_arguments"),
        (json!([1]), "invalid_arguments"),
        (json!({"confirm": true}), "invalid_arguments"),
    ] {
        assert_eq!(
            call_tool(&host, "where_am_i", &args).unwrap_err().kind,
            kind
        );
    }
    assert_eq!(
        call_tool(&host, "nope", &json!({})).unwrap_err().kind,
        "unknown_tool"
    );
    assert!(call_tool(&host, "where_am_i", &Value::Null).is_ok());
}

#[test]
fn a_result_never_carries_a_live_secret() {
    let host = host();
    let value = call_tool(&host, "where_am_i", &json!({})).unwrap();
    assert!(value.get("secretsBackend").is_some());
    assert!(!value.to_string().contains("[redacted]"));
}

#[test]
fn json_rpc_answers_initialize_ping_and_unknown_methods() {
    let host = host();
    let reply = |message: Value| handle_message(&host, &message).unwrap();
    let init = reply(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
    assert_eq!(init["result"]["serverInfo"]["name"], super::SERVER_NAME);
    assert_eq!(init["result"]["serverInfo"]["version"], "0.0.0-test");
    assert_eq!(
        reply(json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))["result"],
        json!({})
    );
    assert_eq!(
        reply(json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}))["error"]["code"],
        -32601
    );
    assert!(handle_message(
        &host,
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .is_none());
}

#[test]
fn every_resource_reads_through_the_host() {
    let host = host();
    for def in RESOURCES {
        let (text, mime) = read_resource(&host, def.uri).unwrap();
        assert_eq!(mime, def.mime);
        assert!(
            !text.is_empty() || def.uri.ends_with("servers"),
            "{}",
            def.uri
        );
    }
    assert_eq!(
        read_resource(&host, "mcpm://nope").unwrap_err().kind,
        "unknown_resource"
    );
}

mod servers {
    use serde_json::json;
    use toolport_plus_api::{AccessSetInfo, MemoryHost, MemoryRegistry};

    use super::super::call_tool;

    fn host() -> MemoryHost {
        let registry = MemoryRegistry::with_servers(Vec::new());
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

    const GAMMA: &str = r#"{"name":"gamma","config":{"command":"gamma-mcp","args":["--flag"],"inheritEnv":true,"env":{"MODE":"fast"}},"profile_tags":["Focus"],"confirm":true}"#;

    fn call(
        host: &MemoryHost,
        tool: &str,
        args: &str,
    ) -> Result<serde_json::Value, super::super::ToolError> {
        call_tool(host, tool, &serde_json::from_str(args).unwrap())
    }

    #[test]
    fn test01_install_registers_a_server_with_env_inherit_env_and_access_sets() {
        let host = host();
        let done = call(&host, "servers_install", GAMMA).unwrap();
        assert_eq!(
            done,
            json!({"installed": true, "id": "gamma", "name": "gamma"})
        );
        let got = call(&host, "servers_get", r#"{"name":"gamma"}"#).unwrap();
        assert_eq!(got["inheritEnv"], true);
        assert_eq!(got["env"], json!([{"key": "MODE", "secret": false}]));
        assert_eq!(got["accessSets"], json!(["default", "focus"]));
        assert_eq!(
            call(&host, "servers_install", GAMMA).unwrap_err().kind,
            "conflict"
        );
    }

    #[test]
    fn test02_a_patch_changes_only_what_it_names_and_refuses_a_credential() {
        let host = host();
        call(&host, "servers_install", GAMMA).unwrap();
        let done = call(
            &host,
            "servers_update_config",
            r#"{"name":"gamma","patch":{"inheritEnv":false,"env":{"JAVA_HOME":"/opt/jdk","MODE":null}},"confirm":true}"#,
        )
        .unwrap();
        assert_eq!(done["updatedKeys"], json!(["env", "inheritEnv"]));
        assert_eq!(done["envChanged"], json!(["JAVA_HOME", "MODE"]));
        let refused = call(
            &host,
            "servers_update_config",
            r#"{"name":"gamma","patch":{"env":{"API_KEY":"plain"}},"confirm":true}"#,
        )
        .unwrap_err();
        assert_eq!(refused.kind, "invalid_arguments");
        assert!(refused.message.contains("rule credential-name"));
        let rename = call(
            &host,
            "servers_update_config",
            r#"{"name":"gamma","patch":{"name":"x"},"confirm":true}"#,
        );
        assert_eq!(
            rename.unwrap_err().message,
            "a patch cannot rename a server"
        );
        let unknown = call(
            &host,
            "servers_update_config",
            r#"{"name":"gamma","patch":{"bogus":1},"confirm":true}"#,
        );
        assert_eq!(
            unknown.unwrap_err().message,
            "unsupported config field: bogus"
        );
    }

    #[test]
    fn test03_profile_tags_list_and_unlist_a_server_in_an_access_set_without_switching_it_on() {
        let host = host();
        call(
            &host,
            "servers_install",
            r#"{"name":"beta","config":{"command":"x"},"confirm":true}"#,
        )
        .unwrap();
        host.registry.servers.borrow_mut()[0].enabled = false;
        let added = call(
            &host,
            "servers_add_profile_tag",
            r#"{"name":"beta","profile_tag":"focus"}"#,
        )
        .unwrap();
        assert_eq!(
            added,
            json!({"name": "beta", "accessSets": ["default", "focus"], "enabled": false})
        );
        let removed = call(
            &host,
            "servers_remove_profile_tag",
            r#"{"name":"beta","profile_tag":"Default"}"#,
        )
        .unwrap();
        assert_eq!(removed["accessSets"], json!(["focus"]));
        assert_eq!(
            call(
                &host,
                "servers_add_profile_tag",
                r#"{"name":"beta","profile_tag":"nope"}"#
            )
            .unwrap_err()
            .kind,
            "not_found"
        );
        let sets = call(&host, "servers_list_profiles", "{}").unwrap();
        assert_eq!(sets["accessSets"][1]["serverIds"], json!(["beta"]));
    }

    #[test]
    fn test04_restart_previews_by_default_and_apply_needs_confirm() {
        let host = host();
        call(
            &host,
            "servers_install",
            r#"{"name":"beta","config":{"command":"x"},"confirm":true}"#,
        )
        .unwrap();
        let preview = call(&host, "servers_restart", r#"{"name":"beta"}"#).unwrap();
        assert_eq!(preview["status"], "dry_run");
        let refused = call(
            &host,
            "servers_restart",
            r#"{"name":"beta","dry_run":false}"#,
        )
        .unwrap_err();
        assert_eq!(refused.kind, "refused");
    }

    #[test]
    fn test05_set_mode_is_accepted_and_says_nothing_changed() {
        let host = host();
        call(
            &host,
            "servers_install",
            r#"{"name":"beta","config":{"command":"x"},"confirm":true}"#,
        )
        .unwrap();
        let done = call(
            &host,
            "servers_set_mode",
            r#"{"name":"beta","mode":"direct","confirm":true}"#,
        )
        .unwrap();
        assert_eq!(done["changed"], false);
        assert_eq!(done["dropped"], true);
        let bad = call(
            &host,
            "servers_set_mode",
            r#"{"name":"beta","mode":"bogus","confirm":true}"#,
        );
        assert_eq!(bad.unwrap_err().kind, "invalid_arguments");
    }

    #[test]
    fn test06_uninstall_removes_the_server() {
        let host = host();
        call(
            &host,
            "servers_install",
            r#"{"name":"beta","config":{"command":"x"},"confirm":true}"#,
        )
        .unwrap();
        let done = call(
            &host,
            "servers_uninstall",
            r#"{"name":"beta","confirm":true}"#,
        )
        .unwrap();
        assert_eq!(done["id"], "beta");
        assert!(host.registry.servers.borrow().is_empty());
        let gone = call(
            &host,
            "servers_uninstall",
            r#"{"name":"beta","confirm":true}"#,
        )
        .unwrap_err();
        assert_eq!(gone.kind, "not_found");
    }
}
