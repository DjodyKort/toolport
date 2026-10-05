use super::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub(super) const FAKE_SECRET: &str = "FAKE-SECRET-VALUE-do-not-print-7f3a";

const EXPECTED_TOOLS: &[&str] = &[
    "skills_list",
    "sources_ls",
    "context_measure",
    "context_bundle_ls",
    "context_bundle_status",
    "context_bundle_apply",
    "context_bundle_undo",
    "context_compose",
    "tasks_list",
    "tasks_get",
    "tasks_history",
    "tasks_run",
    "tasks_cancel",
    "library_status",
    "library_pull",
    "plugins_ls",
    "plugins_show",
    "plugins_config",
    "plugins_mcp",
    "hooks_ls",
    "skills_get",
    "skills_lint",
    "skills_status",
    "skills_list_transpilers",
    "skills_tap_list",
    "skills_search",
    "skills_tap_add",
    "skills_tap_remove",
    "skills_tap_update",
    "skills_install",
    "skills_diff",
    "skills_audit",
    "skills_bundle",
    "skills_unbundle",
    "skills_clean",
    "skills_uninstall",
    "skills_resolve",
    "skills_scaffold",
    "skills_sync",
    "skills_edit_body",
    "skills_edit_frontmatter",
    "skills_delete",
    "agents_list",
    "agents_get",
    "agents_lint",
    "agents_list_transpilers",
    "agents_scaffold",
    "agents_sync",
    "agents_diff",
    "agents_audit",
    "agents_status",
    "agents_clean",
    "agents_uninstall",
    "agents_edit_body",
    "styles_list",
    "styles_get",
    "styles_lint",
    "styles_active",
    "styles_list_transpilers",
    "styles_scaffold",
    "styles_sync_tier1",
    "styles_apply",
    "styles_diff",
    "styles_status",
    "styles_clean",
    "styles_edit_body",
    "styles_remove",
    "compression_status",
    "compression_enable",
    "compression_disable",
    "compression_set_provider",
    "compression_use",
    "compression_sync",
    "compression_seal",
    "servers_list",
    "servers_get",
    "servers_list_profiles",
    "servers_detect_source",
    "servers_git_status",
    "servers_check_updates",
    "servers_add_profile_tag",
    "servers_remove_profile_tag",
    "servers_install",
    "servers_update_config",
    "servers_apply_update",
    "servers_set_mode",
    "servers_fork_sync",
    "servers_auth",
    "servers_uninstall",
    "clients_list",
    "clients_sync",
    "client_direct_ls",
    "client_direct_add",
    "client_direct_rm",
    "skills_git_push",
    "sync_push",
    "where_am_i",
    "doctor",
    "flow_diagram",
];

const EXPECTED_RESOURCES: &[&str] = &[
    "mcpm://paths",
    "mcpm://status",
    "mcpm://flow",
    "mcpm://inventory/skills",
    "mcpm://inventory/agents",
    "mcpm://inventory/styles",
    "mcpm://inventory/servers",
    "mcpm://clients",
    "mcpm://architecture",
    "mcpm://workflows",
    "mcpm://router/status",
];

pub(super) struct Fixture {
    _env: crate::plus::compression::manage_tests::EnvGuard,
    base: crate::plus::testutil::DataDirFx,
    pub(super) home: PathBuf,
    pub(super) repo: PathBuf,
}

impl std::ops::Deref for Fixture {
    type Target = crate::plus::testutil::DataDirFx;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl Fixture {
    pub(super) fn new(tag: &str) -> Self {
        Self::build(crate::plus::testutil::DataDirFx::new("selfmcp", tag))
    }

    pub(super) fn with_vault(tag: &str) -> Self {
        Self::build(
            crate::plus::testutil::DataDirFx::new("selfmcp", tag).with_secret_key(&"cd".repeat(32)),
        )
    }

    fn build(base: crate::plus::testutil::DataDirFx) -> Self {
        let dir = base.dir.clone();
        base.write_registry(&json!({
            "version": 1,
            "servers": [
                {
                    "id": "srv-alpha", "name": "alpha", "transport": "stdio",
                    "command": "alpha-mcp", "args": [],
                    "env": [{"key": "API_KEY", "value": FAKE_SECRET, "secret": true},
                            {"key": "PLAIN", "value": FAKE_SECRET}]
                },
                {"id": "srv-beta", "name": "beta", "transport": "http", "url": "https://example.invalid/mcp"}
            ],
            "profiles": [{"id": "default", "name": "Default", "enabledServerIds": ["srv-alpha"]}],
            "activeProfileId": "default"
        }));
        let home = dir.join("home");
        let repo = dir.join("skills-repo");
        for (rel, text) in [
            (
                "skills/demo/SKILL.md",
                "---\nname: demo\ndescription: A synthetic demo skill\n---\nBody text\n",
            ),
            (
                "agents/helper/AGENT.md",
                "---\nname: helper\ndescription: A synthetic helper agent\nmodel: inherit\n---\nAgent prompt\n",
            ),
            (
                "styles/plain/STYLE.md",
                "---\nname: plain\ndescription: A synthetic plain style\nkeep-coding-instructions: true\n---\nStyle text\n",
            ),
        ] {
            let path = repo.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        std::fs::create_dir_all(&home).unwrap();
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        backend::TEST_REPO.with(|r| *r.borrow_mut() = Some(repo.clone()));
        Self {
            _env: crate::plus::compression::manage_tests::EnvGuard::home(&home),
            base,
            home,
            repo,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
        backend::TEST_REPO.with(|r| *r.borrow_mut() = None);
    }
}

fn sample_args(tool: &ToolDef) -> Value {
    let mut map = serde_json::Map::new();
    for param in tool.params.iter().filter(|p| p.required) {
        let value = match param.ty {
            catalog::Ty::Str => json!("x"),
            catalog::Ty::Bool => json!(true),
            catalog::Ty::Obj => json!({}),
            catalog::Ty::StrList => json!(["x"]),
            catalog::Ty::Int => json!(1),
        };
        map.insert(param.name.to_string(), value);
    }
    Value::Object(map)
}

fn err_kind(result: Result<Value, ToolError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(e) => e.kind,
    }
}

#[test]
fn registry_has_all_99_tools_and_11_resources() {
    let names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
    assert_eq!(names.len(), 99);
    assert_eq!(
        names.iter().copied().collect::<BTreeSet<_>>(),
        EXPECTED_TOOLS.iter().copied().collect::<BTreeSet<_>>()
    );
    let uris: Vec<&str> = RESOURCES.iter().map(|r| r.uri).collect();
    assert_eq!(uris.len(), 11);
    assert_eq!(
        uris.iter().copied().collect::<BTreeSet<_>>(),
        EXPECTED_RESOURCES.iter().copied().collect::<BTreeSet<_>>()
    );
}

#[test]
fn module_counts_match_the_parity_matrix() {
    let count = |prefix: &str| TOOLS.iter().filter(|t| t.name.starts_with(prefix)).count();
    assert_eq!(count("skills_") - 1, 23);
    assert_eq!(count("agents_"), 12);
    assert_eq!(count("styles_"), 13);
    assert_eq!(count("compression_"), 7);
    assert_eq!(count("servers_"), 15);
    assert_eq!(count("clients_"), 2);
    assert_eq!(count("client_direct_"), 3);
    assert_eq!(count("sources_"), 1);
    assert_eq!(count("library_"), 2);
    assert_eq!(count("context_"), 6);
    assert_eq!(count("plugins_"), 4);
    assert_eq!(count("hooks_"), 1);
    assert_eq!(count("tasks_"), 5);
}

#[test]
fn every_tool_has_a_valid_object_schema() {
    for tool in TOOLS {
        let schema = catalog::input_schema(tool);
        assert_eq!(schema["type"], "object", "{}", tool.name);
        let props = schema["properties"].as_object().unwrap();
        for param in tool.params {
            assert!(
                props.contains_key(param.name),
                "{} {}",
                tool.name,
                param.name
            );
        }
        for required in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            assert!(
                props.contains_key(required.as_str().unwrap()),
                "{}",
                tool.name
            );
        }
        assert_eq!(
            props.contains_key("confirm"),
            tool.gate != Gate::None,
            "{}",
            tool.name
        );
        assert!((1..=4).contains(&tool.tier), "{}", tool.name);
        assert!(!tool.description.is_empty());
    }
}

#[test]
fn tiers_three_and_four_always_carry_a_gate() {
    for tool in TOOLS {
        if tool.tier >= 3 {
            assert_ne!(tool.gate, Gate::None, "{}", tool.name);
        }
        if tool.tier == 1 {
            assert_eq!(tool.gate, Gate::None, "{}", tool.name);
        }
    }
    let destructive: BTreeSet<&str> = TOOLS
        .iter()
        .filter(|t| t.tier == 4)
        .map(|t| t.name)
        .collect();
    assert_eq!(
        destructive,
        BTreeSet::from([
            "skills_git_push",
            "sync_push",
            "styles_remove",
            "servers_uninstall",
            "skills_clean",
            "skills_uninstall",
            "agents_clean",
            "agents_uninstall",
            "styles_clean",
            "compression_disable"
        ])
    );
}

#[test]
fn gated_tools_refuse_without_confirmation() {
    let _fixture = Fixture::new("gates");
    for tool in TOOLS.iter().filter(|t| t.gate == Gate::Always) {
        let args = sample_args(tool);
        assert_eq!(
            err_kind(call_tool(tool.name, &args)),
            "refused",
            "{}",
            tool.name
        );
        let mut explicit = args.clone();
        explicit["confirm"] = json!(false);
        assert_eq!(
            err_kind(call_tool(tool.name, &explicit)),
            "refused",
            "{}",
            tool.name
        );
        let mut confirmed = args;
        confirmed["confirm"] = json!(true);
        assert_ne!(
            err_kind(call_tool(tool.name, &confirmed)),
            "refused",
            "{}",
            tool.name
        );
    }
}

#[test]
fn dry_run_waives_the_gate_only_where_declared() {
    let _fixture = Fixture::new("dryrun");
    for name in ["clients_sync", "sync_push"] {
        assert_eq!(err_kind(call_tool(name, &json!({}))), "refused", "{name}");
        assert_ne!(
            err_kind(call_tool(name, &json!({"dry_run": true}))),
            "refused",
            "{name}"
        );
    }
    let refused = call_tool("styles_remove", &json!({"dry_run": true})).unwrap_err();
    assert_eq!(refused.kind, "refused");
    assert!(refused.message.contains("WARNING"));
}

#[test]
fn a_dry_run_that_is_on_by_default_previews_without_confirm_and_applies_only_with_it() {
    let _fixture = Fixture::new("dryrun-default");
    let gated: Vec<&ToolDef> = TOOLS
        .iter()
        .filter(|t| {
            t.gate == Gate::UnlessDryRun
                && t.params
                    .iter()
                    .any(|p| p.name == "dry_run" && p.default == Some(true))
        })
        .collect();
    assert!(!gated.is_empty());
    for tool in gated {
        let args = sample_args(tool);
        let with = |dry_run: Option<bool>| {
            let mut args = args.clone();
            if let Some(dry_run) = dry_run {
                args["dry_run"] = json!(dry_run);
            }
            args
        };
        for preview in [None, Some(true)] {
            assert_ne!(
                err_kind(call_tool(tool.name, &with(preview))),
                "refused",
                "{} {preview:?}",
                tool.name
            );
        }
        let refused = call_tool(tool.name, &with(Some(false))).unwrap_err();
        assert_eq!(refused.kind, "refused", "{}", tool.name);
        assert_eq!(refused.message.contains("WARNING"), tool.tier >= 4);
    }
}

const DEFAULT_DRY_RUN_TOOLS: &[&str] = &[
    "library_pull",
    "skills_tap_add",
    "skills_tap_remove",
    "skills_tap_update",
    "skills_install",
    "skills_bundle",
    "skills_unbundle",
    "skills_clean",
    "skills_uninstall",
    "skills_resolve",
    "agents_clean",
    "agents_uninstall",
    "styles_clean",
    "compression_enable",
    "compression_disable",
    "compression_set_provider",
    "compression_use",
    "compression_sync",
    "compression_seal",
    "client_direct_add",
    "client_direct_rm",
    "plugins_config",
    "plugins_mcp",
];

#[test]
fn every_tool_whose_dry_run_defaults_to_true_says_so_and_the_list_is_closed() {
    let found: BTreeSet<&str> = TOOLS
        .iter()
        .filter(|t| {
            t.params
                .iter()
                .any(|p| p.name == "dry_run" && p.default == Some(true))
        })
        .map(|t| t.name)
        .collect();
    assert_eq!(
        found,
        DEFAULT_DRY_RUN_TOOLS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
    );
    for tool in TOOLS.iter().filter(|t| found.contains(t.name)) {
        let schema = catalog::input_schema(tool);
        assert_eq!(
            schema["properties"]["dry_run"]["default"], true,
            "{}",
            tool.name
        );
        assert!(
            tool.description.contains("dry_run is on by default"),
            "{} must say that dry_run is on by default",
            tool.name
        );
    }
    for tool in TOOLS.iter().filter(|t| t.gate == Gate::UnlessDryRun) {
        assert!(
            found.contains(tool.name) || tool.name == "clients_sync" || tool.name == "sync_push",
            "{} gates on dry_run without defaulting it to true",
            tool.name
        );
    }
}

#[test]
fn argument_validation_rejects_bad_input() {
    assert_eq!(err_kind(call_tool("nope", &json!({}))), "unknown_tool");
    assert_eq!(
        err_kind(call_tool("skills_get", &json!({}))),
        "invalid_arguments"
    );
    assert_eq!(
        err_kind(call_tool("skills_get", &json!({"name": 3}))),
        "invalid_arguments"
    );
    assert_eq!(
        err_kind(call_tool("doctor", &json!({"extra": 1}))),
        "invalid_arguments"
    );
    assert_eq!(
        err_kind(call_tool(
            "skills_delete",
            &json!({"name": "x", "confirm": "yes"})
        )),
        "invalid_arguments"
    );
    assert_eq!(
        err_kind(call_tool("doctor", &json!("text"))),
        "invalid_arguments"
    );
}

#[test]
fn registry_reads_never_return_secret_values() {
    let _fixture = Fixture::new("secrets");
    let mut outputs = Vec::new();
    for (name, args) in [
        ("servers_list", json!({})),
        ("servers_get", json!({"name": "alpha"})),
        ("servers_list_profiles", json!({})),
        ("where_am_i", json!({})),
        ("doctor", json!({})),
        ("clients_list", json!({})),
    ] {
        outputs.push(format!("{name}: {:?}", call_tool(name, &args)));
    }
    for def in RESOURCES {
        outputs.push(format!("{}: {:?}", def.uri, read_resource(def.uri)));
    }
    for text in &outputs {
        assert!(!text.contains(FAKE_SECRET), "{text}");
    }
    let got = call_tool("servers_get", &json!({"name": "alpha"})).unwrap();
    assert_eq!(got["env"][0], json!({"key": "API_KEY", "secret": true}));
    assert_eq!(got["transport"], "stdio");
    assert_eq!(got["enabled"], true);
    let listed = call_tool("servers_list", &json!({})).unwrap();
    assert_eq!(listed["servers"].as_array().unwrap().len(), 2);
}

#[test]
fn scrubber_masks_sensitive_keys_and_live_secrets() {
    std::env::set_var("TOOLPORT_HTTP_TOKEN", "synthetic-live-token-123");
    let scrubbed = redact::scrub(json!({
        "apiToken": "abc",
        "password": ["a"],
        "secret": true,
        "secretsBackend": "encrypted-file",
        "note": "value synthetic-live-token-123 inside",
        "nested": [{"Authorization": "Bearer q"}],
    }));
    std::env::remove_var("TOOLPORT_HTTP_TOKEN");
    assert_eq!(scrubbed["apiToken"], "[redacted]");
    assert_eq!(scrubbed["password"], "[redacted]");
    assert_eq!(scrubbed["secret"], true);
    assert_eq!(scrubbed["secretsBackend"], "encrypted-file");
    assert_eq!(scrubbed["note"], "value [redacted] inside");
    assert_eq!(scrubbed["nested"][0]["Authorization"], "[redacted]");
}

#[test]
fn skills_tools_read_a_synthetic_repository() {
    let _fixture = Fixture::new("skills");
    let repo = std::env::temp_dir().join(format!("selfmcp-repo-{}", std::process::id()));
    let skill_dir = repo.join("skills/demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: A synthetic demo skill\n---\nBody text\n",
    )
    .unwrap();
    let path = repo.to_string_lossy().to_string();
    let list = call_tool("skills_list", &json!({"repo_path": path})).unwrap();
    assert_eq!(list["skills"][0]["name"], "demo");
    let got = call_tool("skills_get", &json!({"name": "demo", "repo_path": path})).unwrap();
    assert!(got["body"].as_str().unwrap().contains("Body text"));
    assert_eq!(
        err_kind(call_tool(
            "skills_get",
            &json!({"name": "missing", "repo_path": path})
        )),
        "not_found"
    );
    let lint = call_tool("skills_lint", &json!({"repo_path": path})).unwrap();
    assert!(lint["messages"].is_array());
    let transpilers = call_tool("skills_list_transpilers", &json!({})).unwrap();
    assert!(!transpilers["transpilers"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn skills_get_returns_the_first_skill_of_a_name_and_skips_unreadable_ones() {
    let _fixture = Fixture::new("skills-first");
    let repo = std::env::temp_dir().join(format!("selfmcp-first-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    let write = |dir: &str, text: &str| {
        let dir = repo.join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
    };
    write("skills/a-broken", "no frontmatter at all");
    write(
        "skills/b-first",
        "---\nname: demo\ndescription: d\n---\nfirst body\n",
    );
    write(
        "skills/c-second",
        "---\nname: demo\ndescription: d\n---\nsecond body\n",
    );
    write(
        "rules/d-rule",
        "---\nname: demo\ndescription: d\n---\nrule body\n",
    );
    write(
        "rules/e-other",
        "---\nname: other\ndescription: o\n---\nother body\n",
    );
    std::fs::create_dir_all(repo.join("skills/f-empty")).unwrap();
    let path = repo.to_string_lossy().to_string();
    let get = |name: &str| call_tool("skills_get", &json!({"name": name, "repo_path": path}));
    let first = get("demo").unwrap();
    assert_eq!(first["body"], "first body");
    assert!(first["path"]
        .as_str()
        .unwrap()
        .ends_with("skills/b-first/SKILL.md"));
    assert_eq!(first["type"], "skill");
    let other = get("other").unwrap();
    assert_eq!(other["body"], "other body");
    assert_eq!(other["type"], "rule");
    assert_eq!(err_kind(get("a-broken")), "not_found");
    let listed = call_tool("skills_list", &json!({"repo_path": path})).unwrap();
    assert_eq!(listed["skills"].as_array().unwrap().len(), 4);
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn absent_null_and_empty_arguments_call_a_tool_the_same_way() {
    let _fixture = Fixture::new("rpc-args");
    let call = |params: Value| {
        let mut message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call"});
        message["params"] = params;
        handle_message(&message).unwrap()
    };
    let bare = call(json!({"name": "servers_list"}));
    assert_eq!(bare["result"]["isError"], false);
    assert_eq!(
        call(json!({"name": "servers_list", "arguments": null})),
        bare
    );
    assert_eq!(call(json!({"name": "servers_list", "arguments": {}})), bare);
    let missing =
        handle_message(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call"})).unwrap();
    assert_eq!(missing["error"]["code"], -32602);
    assert_eq!(
        call_tool("servers_list", &Value::Null).unwrap(),
        call_tool("servers_list", &json!({})).unwrap()
    );
}

#[test]
fn json_rpc_surface_lists_calls_and_reads() {
    let _fixture = Fixture::new("rpc");
    let call = |message: Value| handle_message(&message).unwrap();
    let init = call(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
    assert_eq!(init["result"]["serverInfo"]["name"], SERVER_NAME);
    assert!(
        handle_message(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none()
    );
    let tools = call(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 99);
    let resources = call(json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}));
    assert_eq!(
        resources["result"]["resources"].as_array().unwrap().len(),
        11
    );
    let refused = call(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "servers_uninstall", "arguments": {"name": "alpha"}}}));
    assert_eq!(refused["result"]["isError"], true);
    assert!(refused["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .starts_with("refused"));
    let ok = call(json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
        "params": {"name": "servers_list", "arguments": {}}}));
    assert_eq!(ok["result"]["isError"], false);
    let read = call(
        json!({"jsonrpc": "2.0", "id": 6, "method": "resources/read",
        "params": {"uri": "mcpm://inventory/servers"}}),
    );
    let text = read["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("alpha [stdio/unknown]"));
    let missing = call(
        json!({"jsonrpc": "2.0", "id": 7, "method": "resources/read",
        "params": {"uri": "mcpm://nope"}}),
    );
    assert_eq!(missing["error"]["code"], -32002);
    let unknown = call(json!({"jsonrpc": "2.0", "id": 8, "method": "bogus"}));
    assert_eq!(unknown["error"]["code"], -32601);
}

#[test]
fn binary_path_points_at_the_selfmcp_binary() {
    assert!(register::binary_path().contains(BINARY_NAME));
}
