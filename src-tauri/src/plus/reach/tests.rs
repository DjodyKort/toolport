use super::*;
use crate::clients::{DetectedClient, GatewayEntryState, McpServer};
use serde_json::json;

fn server(id: &str, name: &str, transport: &str, url: Option<&str>) -> ServerEntry {
    serde_json::from_value(json!({
        "id": id,
        "name": name,
        "transport": transport,
        "url": url,
    }))
    .unwrap()
}

fn entry(name: &str, transport: &str, url: Option<&str>, command: Option<&str>, args: &[&str]) -> McpServer {
    McpServer {
        name: name.to_string(),
        transport: transport.to_string(),
        command: command.map(String::from),
        args: args.iter().map(|a| a.to_string()).collect(),
        env_keys: Vec::new(),
        url: url.map(String::from),
    }
}

fn client(id: &str, gateway: GatewayEntryState, servers: Vec<McpServer>) -> DetectedClient {
    DetectedClient {
        id: id.to_string(),
        name: id.to_string(),
        uses_connectors: false,
        config_path: format!("/home/{id}.json"),
        config_exists: true,
        app_present: true,
        servers,
        plugin_servers: Vec::new(),
        gateway_installed: gateway != GatewayEntryState::Absent,
        entry_state: gateway,
        error: None,
    }
}

fn registry(servers: Vec<ServerEntry>, enabled: &[&str]) -> Registry {
    let mut reg = Registry::default();
    reg.servers = servers;
    reg.profiles[0].enabled_server_ids = enabled.iter().map(|s| s.to_string()).collect();
    reg
}

fn route<'a>(report: &'a Report, server: &str, client: &str) -> Vec<&'a Route> {
    report
        .servers
        .iter()
        .find(|s| s.id == server)
        .unwrap()
        .routes
        .iter()
        .filter(|r| r.client == client)
        .collect()
}

#[test]
fn a_server_in_the_profile_is_reached_through_the_gateway_of_a_managed_client() {
    let reg = registry(vec![server("alpha", "Alpha", "stdio", None)], &["alpha"]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Managed, vec![])]);
    let routes = route(&report, "alpha", "claude-code");
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].how, How::Gateway);
    assert_eq!(report.clients[0].profile.as_deref(), Some("Default"));
    assert!(report.findings.is_empty());
}

#[test]
fn a_client_without_the_gateway_entry_does_not_reach_it() {
    let reg = registry(vec![server("alpha", "Alpha", "stdio", None)], &["alpha"]);
    let report = assess(&reg, &[client("cursor", GatewayEntryState::Absent, vec![])]);
    assert!(route(&report, "alpha", "cursor").is_empty());
    assert_eq!(report.clients[0].gateway, "absent");
    assert_eq!(report.clients[0].profile, None);
}

#[test]
fn a_server_outside_the_clients_profile_is_not_served_to_it() {
    let mut reg = registry(vec![server("alpha", "Alpha", "stdio", None)], &[]);
    reg.profiles.push(crate::registry::Profile {
        id: "work".into(),
        name: "Work".into(),
        enabled_server_ids: vec!["alpha".into()],
        tool_scope: Default::default(),
        instructions: None,
    });
    reg.client_scopes.insert("cursor".into(), "Work".into());
    let report = assess(
        &reg,
        &[
            client("cursor", GatewayEntryState::Managed, vec![]),
            client("claude-code", GatewayEntryState::Managed, vec![]),
        ],
    );
    assert_eq!(route(&report, "alpha", "cursor").len(), 1);
    assert!(route(&report, "alpha", "claude-code").is_empty());
}

fn scoped_to_work(enabled_in_work: &[&str]) -> Registry {
    let mut reg = registry(
        vec![
            server("alpha", "Alpha", "stdio", None),
            server("slack", "Slack", "http", Some("https://mcp.slack.com/mcp")),
        ],
        &["alpha", "slack"],
    );
    reg.profiles.push(crate::registry::Profile {
        id: "work".into(),
        name: "Work".into(),
        enabled_server_ids: enabled_in_work.iter().map(|s| s.to_string()).collect(),
        tool_scope: Default::default(),
        instructions: None,
    });
    reg.client_scopes.insert("claude-code".into(), "Work".into());
    reg
}

#[test]
fn a_server_on_in_the_active_profile_but_left_out_of_a_clients_profile_is_a_finding() {
    let reg = scoped_to_work(&["alpha"]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Managed, vec![])]);
    let found: Vec<&Finding> = report.findings.iter().filter(|f| f.kind == "left-out-by-profile").collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].level, "look");
    assert_eq!(found[0].server.as_deref(), Some("slack"));
    assert_eq!(found[0].client.as_deref(), Some("claude-code"));
    assert!(found[0].text.contains("the profile Work of claude-code leaves it out"), "{}", found[0].text);
}

#[test]
fn a_left_out_server_the_client_reaches_through_its_own_entry_is_no_finding() {
    let reg = scoped_to_work(&["alpha"]);
    let own = entry("slack", "http", Some("https://mcp.slack.com/mcp"), None, &[]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Managed, vec![own])]);
    assert!(report.findings.iter().all(|f| f.kind != "left-out-by-profile"));
}

#[test]
fn a_client_without_the_gateway_entry_has_no_profile_to_leave_servers_out() {
    let reg = scoped_to_work(&[]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Absent, vec![])]);
    assert!(report.findings.iter().all(|f| f.kind != "left-out-by-profile"));
}

#[test]
fn an_entry_the_client_holds_on_its_own_is_matched_by_name_and_reported_as_direct_only() {
    let reg = registry(vec![server("figma-mcp-server", "Figma MCP Server", "sse", None)], &[]);
    let own = entry("Figma MCP Server", "http", Some("https://mcp.figma.com/mcp"), None, &[]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Managed, vec![own])]);
    let routes = route(&report, "figma-mcp-server", "claude-code");
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].how, How::OwnEntry);
    assert_eq!(routes[0].basis, Some("name"));
    assert_eq!(routes[0].transport.as_deref(), Some("http"));
    assert_eq!(routes[0].target.as_deref(), Some("https://mcp.figma.com/mcp"));
    assert!(!routes[0].recorded);
    let kinds: Vec<&str> = report.findings.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, ["direct-only"]);
    assert_eq!(report.findings[0].level, "fyi");
    assert!(report.unregistered.is_empty());
}

#[test]
fn an_own_entry_matches_by_url_and_by_command_when_the_names_differ() {
    let mut by_command = server("beta", "Beta", "stdio", None);
    by_command.command = Some("npx".into());
    by_command.args = vec!["-y".into(), "beta-mcp".into()];
    let reg = registry(
        vec![server("alpha", "Alpha", "http", Some("https://a.example/mcp/")), by_command],
        &["alpha", "beta"],
    );
    let entries = vec![
        entry("a-remote", "http", Some("https://A.example/mcp"), None, &[]),
        entry("b-local", "stdio", None, Some("npx"), &["-y", "beta-mcp"]),
    ];
    let report = assess(&reg, &[client("cursor", GatewayEntryState::Absent, entries)]);
    assert_eq!(route(&report, "alpha", "cursor")[0].basis, Some("url"));
    assert_eq!(route(&report, "beta", "cursor")[0].basis, Some("command"));
}

#[test]
fn a_server_reached_both_ways_by_one_client_is_flagged_as_twice() {
    let reg = registry(vec![server("alpha", "Alpha", "http", None)], &["alpha"]);
    let report = assess(
        &reg,
        &[client(
            "cursor",
            GatewayEntryState::Managed,
            vec![entry("alpha", "http", Some("https://a.example"), None, &[])],
        )],
    );
    assert_eq!(route(&report, "alpha", "cursor").len(), 2);
    let kinds: Vec<&str> = report.findings.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, ["twice"]);
}

#[test]
fn a_launcher_entry_is_reported_as_a_launcher_with_its_state() {
    let reg = registry(vec![server("alpha", "Alpha", "stdio", None)], &["alpha"]);
    let launcher = entry("alpha", "stdio", None, Some("/opt/toolportctl"), &["direct", "run", "alpha"]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Managed, vec![launcher])]);
    let routes = route(&report, "alpha", "claude-code");
    let launchers: Vec<_> = routes.iter().filter(|r| r.how == How::Launcher).collect();
    assert_eq!(launchers.len(), 1);
    assert_eq!(launchers[0].state.as_deref(), Some("unrecorded"));
    assert!(report.findings.iter().all(|f| f.kind != "twice"));
}

#[test]
fn a_launcher_for_a_server_the_registry_lacks_is_a_finding_not_a_route() {
    let reg = registry(vec![], &[]);
    let launcher = entry("ghost", "stdio", None, Some("toolportctl"), &["direct", "run", "ghost"]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Managed, vec![launcher])]);
    let kinds: Vec<&str> = report.findings.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, ["orphan-launcher"]);
    assert!(report.unregistered.is_empty());
}

#[test]
fn a_recorded_launcher_that_was_edited_into_a_remote_entry_says_so() {
    let mut reg = registry(vec![server("figma-mcp-server", "Figma MCP Server", "sse", None)], &[]);
    reg.unknown_fields.insert(
        "plus".into(),
        json!({"directEntries": {"claude-code": {"Figma MCP Server": {
            "server": "figma-mcp-server",
            "command": "/opt/toolportctl",
            "args": ["direct", "run", "figma-mcp-server"],
            "env": {},
        }}}}),
    );
    let own = entry("Figma MCP Server", "http", Some("https://mcp.figma.com/mcp"), None, &[]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Managed, vec![own])]);
    let routes = route(&report, "figma-mcp-server", "claude-code");
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].how, How::OwnEntry);
    assert_eq!(routes[0].basis, Some("record"));
    assert!(routes[0].recorded);
    assert_eq!(routes[0].state.as_deref(), Some("customized"));
    assert!(routes[0].note.as_deref().unwrap().contains("edited since"));
}

#[test]
fn an_entry_that_matches_no_server_is_listed_as_unregistered() {
    let reg = registry(vec![server("alpha", "Alpha", "stdio", None)], &["alpha"]);
    let report = assess(
        &reg,
        &[client(
            "cursor",
            GatewayEntryState::Managed,
            vec![entry("playwright", "stdio", None, Some("npx"), &["@playwright/mcp"])],
        )],
    );
    assert_eq!(report.unregistered.len(), 1);
    assert_eq!(report.unregistered[0].entry, "playwright");
    assert_eq!(report.unregistered[0].target.as_deref(), Some("npx (1 argument)"));
}

#[test]
fn targets_never_carry_a_credential() {
    let url = entry(
        "x",
        "http",
        Some("https://user:hunter2@host.example/mcp?token=abc#frag"),
        None,
        &[],
    );
    assert_eq!(target_of(&url).as_deref(), Some("https://host.example/mcp"));
    let command = entry("y", "stdio", None, Some("/usr/bin/tool"), &["--api-key", "abc123"]);
    assert_eq!(target_of(&command).as_deref(), Some("tool (2 arguments)"));
    let shown = serde_json::to_string(&assess(
        &registry(vec![], &[]),
        &[client("cursor", GatewayEntryState::Absent, vec![url, command])],
    ).to_value())
    .unwrap();
    for secret in ["hunter2", "abc123", "token=abc"] {
        assert!(!shown.contains(secret), "{secret} leaked into {shown}");
    }
}

#[test]
fn only_narrows_by_client_and_by_server_and_refuses_unknown_keys() {
    let reg = registry(
        vec![server("alpha", "Alpha", "stdio", None), server("beta", "Beta", "stdio", None)],
        &["alpha", "beta"],
    );
    let all = assess(
        &reg,
        &[
            client("cursor", GatewayEntryState::Managed, vec![]),
            client("claude-code", GatewayEntryState::Managed, vec![]),
        ],
    );
    let one = all.clone().only(Some("cursor"), None).unwrap();
    assert_eq!(one.clients.len(), 1);
    assert!(one.servers.iter().all(|s| s.routes.iter().all(|r| r.client == "cursor")));
    let by_name = all.clone().only(None, Some("alpha")).unwrap();
    assert_eq!(by_name.servers.len(), 1);
    assert_eq!(all.clone().only(None, Some("nope")).unwrap_err().kind, crate::plus::op::ErrorKind::NotFound);
    assert_eq!(all.only(Some("nope"), None).unwrap_err().kind, crate::plus::op::ErrorKind::NotFound);
}

#[test]
fn the_text_names_each_route_and_the_json_keeps_the_same_facts() {
    let reg = registry(vec![server("figma-mcp-server", "Figma MCP Server", "sse", None)], &[]);
    let own = entry("Figma MCP Server", "http", Some("https://mcp.figma.com/mcp"), None, &[]);
    let report = assess(&reg, &[client("claude-code", GatewayEntryState::Managed, vec![own])]);
    let text = report.text();
    assert!(text.contains("Figma MCP Server (sse, in no profile)"), "{text}");
    assert!(text.contains("through its own entry 'Figma MCP Server' (http, https://mcp.figma.com/mcp), not through the gateway"), "{text}");
    assert!(text.contains("fyi: Figma MCP Server is in no profile"), "{text}");
    let value = report.to_value();
    assert_eq!(value["servers"][0]["routes"][0]["how"], "own-entry");
    assert_eq!(value["findings"][0]["kind"], "direct-only");
}

mod claude_code_state {
    use super::*;
    use crate::plus::exec::CmdOutput;
    use crate::plus::plugins::claude::ClaudeRunner;
    use claude_code::{annotate, disabled_in, parse_list, probe};
    use std::time::Duration;

    const LIST: &str = "Checking MCP server health…\n\n\
        claude.ai Google Drive: https://drivemcp.googleapis.com/mcp/v1 - ✔ Connected\n\
        Figma MCP Server: https://mcp.figma.com/mcp (HTTP) - ✔ Connected\n\
        miro: https://mcp.miro.com - ⚠ Needs authentication\n\
        plugin:ecc:chrome-devtools: npx -y chrome-devtools-mcp@latest - ✗ Failed to connect\n";

    struct Canned(Result<CmdOutput, String>);

    impl ClaudeRunner for Canned {
        fn run(&self, args: &[&str], _timeout: Duration) -> Result<CmdOutput, String> {
            assert_eq!(args, ["mcp", "list"]);
            self.0.clone()
        }
    }

    fn output(code: i32, stdout: &str, stderr: &str) -> Result<CmdOutput, String> {
        Ok(CmdOutput { code, stdout: stdout.into(), stderr: stderr.into() })
    }

    fn figma_report() -> (Report, Vec<DetectedClient>) {
        let reg = registry(vec![server("figma-mcp-server", "Figma MCP Server", "sse", None)], &[]);
        let own = entry("Figma MCP Server", "http", Some("https://mcp.figma.com/mcp"), None, &[]);
        let mut claude = client("claude-code", GatewayEntryState::Managed, vec![own]);
        let config = std::env::temp_dir().join(format!("toolport-reach-held-{}.json", std::process::id()));
        std::fs::write(
            &config,
            json!({"projects": {
                "/home/a": {"disabledMcpServers": ["Figma MCP Server", "other"]},
                "/home/b": {"disabledMcpServers": ["other"]},
                "/home/c": {}
            }})
            .to_string(),
        )
        .unwrap();
        claude.config_path = config.to_string_lossy().into_owned();
        let detected = vec![claude];
        (assess(&reg, &detected), detected)
    }

    #[test]
    fn the_projects_an_entry_is_switched_off_for_come_from_the_claude_config() {
        let doc = json!({"projects": {
            "/a": {"disabledMcpServers": ["x", "y"]},
            "/b": {"disabledMcpServers": ["y"]},
            "/c": {}
        }});
        assert_eq!(disabled_in(&doc, "x"), vec!["/a"]);
        assert_eq!(disabled_in(&doc, "y"), vec!["/a", "/b"]);
        assert!(disabled_in(&doc, "z").is_empty());
        assert!(disabled_in(&json!({}), "x").is_empty());
    }

    #[test]
    fn the_list_of_claude_code_is_read_per_server_and_unknown_lines_are_skipped() {
        let states = parse_list(LIST);
        assert_eq!(states.len(), 4);
        assert_eq!(states["Figma MCP Server"], "connected");
        assert_eq!(states["miro"], "needs-auth");
        assert_eq!(states["plugin:ecc:chrome-devtools"], "failed");
        assert!(parse_list("Checking MCP server health…\n\nsomething odd").is_empty());
    }

    #[test]
    fn an_own_entry_of_claude_code_carries_what_claude_code_says_about_it() {
        let (mut report, detected) = figma_report();
        let live = probe(&Canned(output(0, LIST, "")));
        annotate(&mut report, &detected, Some(&live));
        let held = route(&report, "figma-mcp-server", "claude-code")[0].held.clone().unwrap();
        assert_eq!(held.disabled_in, vec!["/home/a"]);
        assert!(!held.enabled());
        assert_eq!(held.live.as_deref(), Some("connected"));
        let value = report.to_value();
        assert_eq!(value["servers"][0]["routes"][0]["held"]["live"], "connected");
        assert_eq!(value["servers"][0]["routes"][0]["held"]["enabled"], false);
        let text = report.text();
        assert!(text.contains("switched off in 1 project of Claude Code, connects"), "{text}");
    }

    #[test]
    fn without_the_live_check_only_the_config_is_reported() {
        let (mut report, detected) = figma_report();
        annotate(&mut report, &detected, None);
        let held = route(&report, "figma-mcp-server", "claude-code")[0].held.clone().unwrap();
        assert_eq!(held.live, None);
        assert!(report.findings.iter().all(|f| f.kind != "live-unavailable"));
    }

    #[test]
    fn a_client_that_is_not_claude_code_gets_no_held_state() {
        let reg = registry(vec![server("figma-mcp-server", "Figma MCP Server", "sse", None)], &[]);
        let own = entry("Figma MCP Server", "http", Some("https://mcp.figma.com/mcp"), None, &[]);
        let detected = vec![client("cursor", GatewayEntryState::Managed, vec![own])];
        let mut report = assess(&reg, &detected);
        annotate(&mut report, &detected, None);
        assert!(route(&report, "figma-mcp-server", "cursor")[0].held.is_none());
    }

    #[test]
    fn a_failed_check_is_a_finding_and_not_an_error() {
        let (mut report, detected) = figma_report();
        let live = probe(&Canned(Err("claude not found on PATH".into())));
        assert!(live.is_err());
        annotate(&mut report, &detected, Some(&live));
        let finding = report.findings.iter().find(|f| f.kind == "live-unavailable").unwrap();
        assert!(finding.text.contains("claude not found on PATH"), "{}", finding.text);
        let silent = probe(&Canned(output(1, "", "boom\n")));
        assert_eq!(silent, Err("boom".to_string()));
    }
}
