use serde_json::json;
use toolport_plus_api::{MemoryHost, MemoryRegistry, RegistryAccess, ServerRecord};

use super::*;

fn host() -> MemoryHost {
    let server = ServerRecord {
        id: "srv-a".into(),
        name: "Alpha".into(),
        enabled: true,
        fork: ForkServerFields {
            forward_instructions: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    MemoryHost {
        registry: MemoryRegistry::with_servers(vec![server]),
        ..Default::default()
    }
}

fn stored(host: &MemoryHost) -> ServerRecord {
    host.registry.get_server("srv-a").unwrap().unwrap()
}

#[test]
fn test01_parse_accepts_the_two_modes_and_trims() {
    assert_eq!(parse("auto"), Ok(AUTO));
    assert_eq!(parse(" search "), Ok(SEARCH));
}

#[test]
fn test02_parse_refuses_a_retired_mcpm_mode_and_says_what_to_use() {
    for retired in ["direct", "router", "legacy", "bridge"] {
        let message = parse(retired).unwrap_err();
        assert!(
            message.contains(&format!("'{retired}' no longer exists"))
                && message.ends_with("auto or search"),
            "{message}"
        );
    }
    assert_eq!(parse("listed"), Err("mode must be auto or search".into()));
}

#[test]
fn test03_apply_search_stores_the_key_and_auto_removes_it() {
    let host = host();
    let done = apply(&host, &stored(&host), SEARCH).unwrap();
    assert_eq!(done["mode"], "search");
    assert_eq!(done["changed"], true);
    assert_eq!(stored(&host).fork.server_mode.as_deref(), Some("search"));
    let again = apply(&host, &stored(&host), SEARCH).unwrap();
    assert_eq!(again["changed"], false);
    let back = apply(&host, &stored(&host), AUTO).unwrap();
    assert_eq!(back["changed"], true);
    assert_eq!(stored(&host).fork.server_mode, None);
}

#[test]
fn test04_apply_keeps_the_other_fork_keys() {
    let host = host();
    apply(&host, &stored(&host), SEARCH).unwrap();
    assert_eq!(stored(&host).fork.forward_instructions, Some(true));
}

#[test]
fn test05_an_unreadable_stored_value_counts_as_auto() {
    for value in [json!("listed"), json!(7), json!(null), json!({"a": 1})] {
        let fields = json!({"serverMode": value}).as_object().unwrap().clone();
        assert_eq!(of(&fields), AUTO, "{value}");
        assert!(!is_search_only(&fields));
    }
    let search = json!({"serverMode": "search"}).as_object().unwrap().clone();
    assert!(is_search_only(&search));
    assert!(!is_search_only(&Map::new()));
}

fn tool(name: &str) -> Value {
    json!({"name": name, "description": format!("{name} does a thing.")})
}

fn helpers() -> [Value; 2] {
    [tool("toolport_search_tools"), tool("toolport_call_tool")]
}

fn names(tools: &[Value]) -> Vec<&str> {
    tools.iter().filter_map(name_of).collect()
}

#[test]
fn test06_listing_drops_search_only_tools_and_adds_the_helpers_behind_the_floor() {
    let tools = vec![
        tool("toolport_status"),
        tool("toolport_fetch_result"),
        tool("big__one"),
        tool("small__one"),
        tool("big__two"),
    ];
    let listed = listing(tools, |name| name.starts_with("big__"), helpers());
    assert_eq!(
        names(&listed),
        [
            "toolport_status",
            "toolport_fetch_result",
            "toolport_search_tools",
            "toolport_call_tool",
            "small__one"
        ]
    );
}

#[test]
fn test07_listing_keeps_helpers_a_fallback_client_already_has() {
    let tools = vec![
        tool("toolport_status"),
        tool("toolport_fetch_result"),
        tool("toolport_search_tools"),
        tool("toolport_call_tool"),
        tool("big__one"),
    ];
    let listed = listing(tools, |name| name.starts_with("big__"), helpers());
    assert_eq!(
        names(&listed),
        [
            "toolport_status",
            "toolport_fetch_result",
            "toolport_search_tools",
            "toolport_call_tool"
        ]
    );
}

#[test]
fn test08_listing_tells_the_model_about_search_only_servers_in_the_search_description() {
    let listed = listing(vec![tool("big__one")], |_| true, helpers());
    let search = listed
        .iter()
        .find(|t| name_of(t) == Some(SEARCH_TOOL))
        .unwrap();
    assert!(
        search["description"]
            .as_str()
            .unwrap()
            .ends_with("Some servers are search-only: their tools are not listed. Find them with this tool and call them with toolport_call_tool.")
    );
    let call = listed
        .iter()
        .find(|t| name_of(t) == Some("toolport_call_tool"))
        .unwrap();
    assert_eq!(call["description"], "toolport_call_tool does a thing.");
}
