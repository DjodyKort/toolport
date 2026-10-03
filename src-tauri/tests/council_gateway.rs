//! The gateway aggregates a council-shaped stdio server: the launcher declared
//! by `plus::council::server_entry` is swapped for the mock fixture, which
//! impersonates the council catalog via `MOCK_MCP_PROFILE=council`.

use conduit_lib::downstream::{DownstreamServer, StdioTransport};
use conduit_lib::plus::council;
use conduit_lib::router::Router;

#[test]
fn gateway_lists_council_tools_and_resources() {
    let entry = council::server_entry();
    assert!(entry
        .env
        .iter()
        .any(|v| v.key == "OPENROUTER_API_KEY" && v.secret && v.value.is_none()));

    let mock = env!("CARGO_BIN_EXE_mock-mcp-server");
    let env = vec![("MOCK_MCP_PROFILE".to_string(), "council".to_string())];
    let transport = StdioTransport::spawn(mock, &[], &env, None).expect("spawn council mock");
    let mut server = DownstreamServer::connect(entry.id.clone(), Box::new(transport))
        .expect("connect council mock");
    server.load_resources_prompts();
    let mut router = Router::new();
    router.add(server);

    let tools: Vec<String> = router
        .aggregated_tools()
        .iter()
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect();
    for expected in council::TOOLS {
        let name = format!("council__{}", expected.name);
        assert!(tools.contains(&name), "{name} missing in {tools:?}");
    }
    assert_eq!(tools.len(), council::TOOLS.len(), "{tools:?}");

    let uris: Vec<String> = router
        .aggregated_resources()
        .iter()
        .filter_map(|r| r["uri"].as_str().map(String::from))
        .collect();
    for (uri, _) in council::RESOURCES {
        assert!(uris.contains(&uri.to_string()), "{uri} missing in {uris:?}");
    }
}
