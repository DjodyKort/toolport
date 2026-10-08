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
