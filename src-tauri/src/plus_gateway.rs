mod plus_gateway {
    use super::*;

    struct OneTool {
        calls: Arc<AtomicUsize>,
    }

    impl conduit_lib::downstream::Transport for OneTool {
        fn request(
            &mut self,
            method: &str,
            _params: Value,
        ) -> Result<Value, conduit_lib::downstream::TransportError> {
            match method {
                "initialize" => Ok(json!({ "protocolVersion": "2025-06-18" })),
                "tools/list" => Ok(json!({
                    "tools": [{
                        "name": "work",
                        "description": "Do the synthetic work",
                        "inputSchema": { "type": "object" }
                    }]
                })),
                "tools/call" => {
                    self.calls.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({ "content": [{ "type": "text", "text": "called" }], "isError": false }))
                }
                other => Err(conduit_lib::downstream::TransportError::Fatal(format!(
                    "unexpected {other}"
                ))),
            }
        }

        fn notify(
            &mut self,
            _method: &str,
            _params: Value,
        ) -> Result<(), conduit_lib::downstream::TransportError> {
            Ok(())
        }
    }

    struct World {
        state: GatewayState,
        calls: Arc<AtomicUsize>,
        _data: DataDirTestEnv,
    }

    fn world(tag: &str, big_mode: Option<&str>) -> World {
        let data = DataDirTestEnv::new(&format!("plus-mode-{tag}"));
        let state = http_state(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut router = Router::new();
        let mut reg = Registry::default();
        for id in ["big", "small"] {
            let downstream = DownstreamServer::connect(
                id.to_string(),
                Box::new(OneTool {
                    calls: Arc::clone(&calls),
                }),
            )
            .unwrap();
            router.add(downstream);
            let mut entry = stub_server(id, id);
            entry.enabled = true;
            if let (true, Some(mode)) = (id == "big", big_mode) {
                entry.unknown_fields.insert("serverMode".into(), json!(mode));
            }
            reg.servers.push(entry);
        }
        *state.registry.lock().unwrap() = reg;
        swap_router(&state, router);
        World {
            state,
            calls,
            _data: data,
        }
    }

    fn set_big_mode(world: &World, mode: Option<&str>) {
        let mut reg = world.state.registry.lock().unwrap();
        let big = reg.servers.iter_mut().find(|server| server.id == "big").unwrap();
        match mode {
            Some(mode) => big.unknown_fields.insert("serverMode".into(), json!(mode)),
            None => big.unknown_fields.remove("serverMode"),
        };
    }

    fn ask(world: &World, mode: DiscoveryMode, method: &str, params: Value) -> Value {
        process_request(
            &world.state,
            &json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}),
            &SearchGuard::default(),
            None,
            None,
            None,
            None,
            None,
            None,
            mode,
        )
        .unwrap()
    }

    fn listed(world: &World, mode: DiscoveryMode) -> Vec<String> {
        let list = ask(world, mode, "tools/list", json!({}));
        list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn test01_a_full_client_can_call_a_tool_that_is_not_in_its_list_by_exact_name() {
        let world = world("exact", Some("search"));
        let names = listed(&world, DiscoveryMode::Full);
        assert!(!names.contains(&"big__work".to_string()), "{names:?}");

        let reply = ask(
            &world,
            DiscoveryMode::Full,
            "tools/call",
            json!({"name": "big__work", "arguments": {}}),
        );
        assert_eq!(reply["result"]["content"][0]["text"], "called", "{reply}");
        assert_eq!(world.calls.load(Ordering::SeqCst), 1);

        let through_helper = ask(
            &world,
            DiscoveryMode::Full,
            "tools/call",
            json!({"name": "toolport_call_tool", "arguments": {"name": "big__work", "arguments": {}}}),
        );
        assert_eq!(
            through_helper["result"]["content"][0]["text"], "called",
            "{through_helper}"
        );
        assert_eq!(world.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test02_a_search_only_server_leaves_the_full_list_and_stays_searchable() {
        let world = world("search", Some("search"));
        let names = listed(&world, DiscoveryMode::Full);
        assert_eq!(
            names,
            [
                "toolport_status",
                "toolport_fetch_result",
                "toolport_search_tools",
                "toolport_call_tool",
                "small__work"
            ]
        );

        let found = ask(
            &world,
            DiscoveryMode::Full,
            "tools/call",
            json!({"name": "toolport_search_tools", "arguments": {"query": "synthetic work"}}),
        );
        let text = found["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("big__work"), "{text}");
    }

    #[test]
    fn test03_the_list_without_a_search_only_server_is_the_upstream_one() {
        let world = world("plain", None);
        let expected = listed(&world, DiscoveryMode::Full);
        assert!(expected.contains(&"big__work".to_string()));
        assert!(!expected.contains(&"toolport_search_tools".to_string()));
        set_big_mode(&world, Some("auto"));
        assert_eq!(listed(&world, DiscoveryMode::Full), expected);
        set_big_mode(&world, Some("search"));
        assert_ne!(listed(&world, DiscoveryMode::Full), expected);
        set_big_mode(&world, None);
        assert_eq!(listed(&world, DiscoveryMode::Full), expected);
    }

    #[test]
    fn test04_lazy_and_grouped_lists_ignore_the_mode() {
        let world = world("lazy", None);
        for mode in [DiscoveryMode::Lazy, DiscoveryMode::Grouped] {
            set_big_mode(&world, None);
            let before = listed(&world, mode);
            set_big_mode(&world, Some("search"));
            assert_eq!(listed(&world, mode), before, "{mode:?}");
        }
    }

    #[test]
    fn test05_the_savings_baseline_still_counts_the_hidden_tools() {
        let world = world("baseline", Some("search"));
        listed(&world, DiscoveryMode::Full);
        let rows = savings::entries();
        let row = rows
            .iter()
            .find(|row| row["kind"] == "catalog_exposure")
            .expect("the first list records its exposure");
        assert_eq!(row["fullToolCount"], 4, "status, fetch, big, small: {row}");
        assert_eq!(row["exposedToolCount"], 5, "floor, both helpers, small: {row}");
        assert!(row["byServerBytes"]["big"].as_u64().unwrap() > 0, "{row}");
        assert!(row["byServerBytes"].get("small").is_none(), "{row}");
    }

    #[test]
    fn test06_the_search_helper_tells_the_model_some_servers_are_search_only() {
        let world = world("note", Some("search"));
        let list = ask(&world, DiscoveryMode::Full, "tools/list", json!({}));
        let search = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "toolport_search_tools")
            .unwrap();
        assert!(search["description"]
            .as_str()
            .unwrap()
            .contains("Some servers are search-only"));
    }
}
