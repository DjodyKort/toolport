//! MCP Tasks (2025-11-25) through the gateway, against the fake MCP server's task mode
//! (`plus/fixtures/stdio/slow-mcp.mjs`, `SLOWMCP_TASKS=1`): a client that declares tasks gets a
//! task-augmented `tools/call` relayed, and `tasks/*` reach the server that owns the task.
#![cfg(feature = "plus")]

mod longcall_support;

mod tasks {
    use super::longcall_support::*;
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};

    const WINDOW_MS: u64 = 1000;
    const CAP_MS: u64 = 12_000;
    const ANSWER_WITHIN: Duration = Duration::from_secs(20);
    const TASKS: Behaviour = Behaviour {
        serial: false,
        ignore_cancel: false,
        tasks: true,
        task_page: 0,
    };
    const RELATED: &str = "io.modelcontextprotocol/related-task";

    fn timeouts() -> Timeouts {
        Timeouts {
            request_ms: Some(WINDOW_MS),
            max_request_ms: Some(CAP_MS),
            ..Default::default()
        }
    }

    /// A client that declared tasks, once the server's tools are published.
    fn tasking_client(case: &Case, tool_server: &str) -> Client {
        let mut client = case.client();
        client.initialize();
        client.tool_of(tool_server, "slow_job");
        client.initialize_with(json!({ "tasks": {} }));
        client
    }

    fn create(client: &mut Client, id: i64, tool: &str, arguments: Value) -> Value {
        client.start_call(id, tool, arguments, None);
        let reply = client.response(&json!(id), ANSWER_WITHIN).1;
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    fn with_task(client: &mut Client, id: i64, tool: &str, arguments: Value) -> Value {
        client.send(json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": tool, "arguments": arguments, "task": { "ttl": 60000 } }
        }));
        let reply = client.response(&json!(id), ANSWER_WITHIN).1;
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    fn task_id(created: &Value) -> String {
        created["task"]["taskId"]
            .as_str()
            .unwrap_or_else(|| panic!("no task in {created}"))
            .to_string()
    }

    fn error_code(reply: &Value) -> i64 {
        reply["error"]["code"].as_i64().unwrap_or(0)
    }

    #[test]
    fn test01_tasks_capability_is_offered_only_to_a_client_that_declared_tasks_and_with_a_task_server(
    ) {
        let _guard = case_lock();
        let case = Case::new("capability", TASKS, timeouts());
        let mut plain = case.client();
        plain.initialize();
        plain.tool_of(SERVER_ID, "slow_job");
        let old = plain.initialize_with(json!({}));
        assert!(old["capabilities"].get("tasks").is_none(), "{old}");
        let told = plain.initialize_with(json!({ "tasks": {} }));
        assert_eq!(
            told["capabilities"]["tasks"],
            json!({ "list": {}, "cancel": {}, "requests": { "tools": { "call": {} } } })
        );

        let quiet = Case::new("capability-plain", Behaviour::default(), timeouts());
        let mut client = quiet.client();
        client.initialize();
        client.tool_of(SERVER_ID, "echo");
        let reply = client.initialize_with(json!({ "tasks": {} }));
        assert!(reply["capabilities"].get("tasks").is_none(), "{reply}");
    }

    #[test]
    fn test02_task_support_of_a_tool_is_listed_and_a_server_without_tasks_lists_none() {
        let _guard = case_lock();
        let case = Case::pair_with(
            "listing",
            TASKS,
            timeouts(),
            Behaviour::default(),
            timeouts(),
        );
        let mut client = tasking_client(&case, SERVER_ID);
        client.tool_of(PEER_ID, "echo");
        let listed = client.request("tools/list", json!({}));
        let tool = |name: &str| {
            listed["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == name)
                .cloned()
                .unwrap_or_else(|| panic!("{name} not listed"))
        };
        assert_eq!(
            tool("slow__slow_job")["execution"]["taskSupport"],
            "optional"
        );
        assert_eq!(
            tool("slow__task_only")["execution"]["taskSupport"],
            "required"
        );
        assert!(tool("slow__echo").get("execution").is_none());
        assert!(tool("other__echo").get("execution").is_none());
    }

    #[test]
    fn test03_a_task_is_created_polled_and_its_result_waits_past_the_call_window() {
        let _guard = case_lock();
        let case = Case::new("lifecycle", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let started = Instant::now();
        let created = with_task(&mut client, 10, &job, json!({ "seconds": 2.5 }));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "create answers at once"
        );
        let id = task_id(&created);
        assert!(id.starts_with("tpt."), "{id}");
        assert_eq!(created["task"]["status"], "working");
        assert!(created["task"]["ttl"].is_number() && created["task"]["pollInterval"].is_number());
        let sent = case.received_calls("slow_job");
        assert_eq!(
            sent[0]["task"],
            json!({ "ttl": 60000 }),
            "the task parameter reached the server"
        );

        let polled = client.request("tasks/get", json!({ "taskId": id }));
        assert_eq!(polled["result"]["taskId"], json!(id));
        assert_eq!(polled["result"]["status"], "working");

        let waited = Instant::now();
        client.start_request(11, "tasks/result", json!({ "taskId": id }));
        let (arrived, result) = client.response(&json!(11), ANSWER_WITHIN);
        let elapsed = arrived.duration_since(waited);
        assert!(
            elapsed > Duration::from_millis(1500),
            "tasks/result outlives the {WINDOW_MS} ms window: {elapsed:?}"
        );
        assert_eq!(
            result["result"]["content"][0]["text"],
            "job task-1 done after 2.5 s"
        );
        assert_eq!(result["result"]["_meta"][RELATED]["taskId"], json!(id));
        let asked = case.received("tasks/result");
        assert_eq!(asked[0]["taskId"], "task-1", "the server sees its own id");

        let done = client.request("tasks/get", json!({ "taskId": id }));
        assert_eq!(done["result"]["status"], "completed");
    }

    #[test]
    fn test04_tasks_result_is_bounded_by_the_call_cap() {
        let _guard = case_lock();
        let capped = Timeouts {
            request_ms: Some(500),
            max_request_ms: Some(1500),
            ..Default::default()
        };
        let case = Case::new("cap", TASKS, capped);
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 6 })));
        let waited = Instant::now();
        client.start_request(11, "tasks/result", json!({ "taskId": id }));
        let (arrived, reply) = client.response(&json!(11), ANSWER_WITHIN);
        let elapsed = arrived.duration_since(waited);
        assert!(
            elapsed > Duration::from_millis(1200) && elapsed < Duration::from_secs(5),
            "cut at the cap, not at the window: {elapsed:?}"
        );
        assert!(
            reply.get("error").is_some() || reply["result"]["isError"] == json!(true),
            "{reply}"
        );
        let still = client.request("tasks/get", json!({ "taskId": id }));
        assert_eq!(
            still["result"]["status"], "working",
            "the task itself goes on"
        );
    }

    #[test]
    fn test05_cancel_ends_a_task_and_a_finished_task_cannot_be_cancelled() {
        let _guard = case_lock();
        let case = Case::new("cancel", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 30 })));
        let cancelled = client.request("tasks/cancel", json!({ "taskId": id }));
        assert_eq!(cancelled["result"]["taskId"], json!(id));
        assert_eq!(cancelled["result"]["status"], "cancelled");
        let again = client.request("tasks/cancel", json!({ "taskId": id }));
        assert_eq!(error_code(&again), -32602, "{again}");
        let result = client.request("tasks/result", json!({ "taskId": id }));
        assert_eq!(result["error"]["message"], "task cancelled");
        assert_eq!(
            result["error"]["data"]["_meta"][RELATED]["taskId"],
            json!(id),
            "the error is the server's own, under the client's id"
        );
    }

    #[test]
    fn test06_two_servers_that_mint_the_same_task_id_stay_apart_and_list_together() {
        let _guard = case_lock();
        let paged = Behaviour {
            task_page: 1,
            ..TASKS
        };
        let case = Case::pair("namespacing", paged, timeouts(), timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let slow = client.tool_of(SERVER_ID, "slow_job");
        let other = client.tool_of(PEER_ID, "slow_job");
        let first = task_id(&with_task(
            &mut client,
            10,
            &slow,
            json!({ "seconds": 0.3 }),
        ));
        let second = task_id(&with_task(
            &mut client,
            11,
            &other,
            json!({ "seconds": 0.4 }),
        ));
        assert_ne!(first, second, "both servers minted task-1");
        let from_first = client.request("tasks/result", json!({ "taskId": first }));
        let from_second = client.request("tasks/result", json!({ "taskId": second }));
        assert_eq!(
            from_first["result"]["content"][0]["text"],
            "job task-1 done after 0.3 s"
        );
        assert_eq!(
            from_second["result"]["content"][0]["text"],
            "job task-1 done after 0.4 s"
        );
        assert_eq!(case.received("tasks/result").len(), 1);
        assert_eq!(
            Case::entries_of(&case.peer_log)
                .iter()
                .filter(|e| e["method"] == "tasks/result")
                .count(),
            1
        );

        let mut listed = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..5 {
            let params = cursor
                .as_ref()
                .map_or(json!({}), |c| json!({ "cursor": c }));
            let page = client.request("tasks/list", params);
            for task in page["result"]["tasks"].as_array().unwrap() {
                listed.push(task["taskId"].as_str().unwrap().to_string());
            }
            match page["result"]["nextCursor"].as_str() {
                Some(next) => cursor = Some(next.to_string()),
                None => break,
            }
        }
        listed.sort();
        let mut expected = vec![first, second];
        expected.sort();
        assert_eq!(listed, expected);
        let bad = client.request("tasks/list", json!({ "cursor": "bogus" }));
        assert_eq!(error_code(&bad), -32602);
    }

    #[test]
    fn test07_an_unknown_foreign_or_other_sessions_task_id_is_invalid_params() {
        let _guard = case_lock();
        let case = Case::new("foreign", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 5 })));
        let mut stranger = tasking_client(&case, SERVER_ID);
        let attempts = |who: &mut Client, taken: &str| {
            for method in ["tasks/get", "tasks/result", "tasks/cancel"] {
                let reply = who.request(method, json!({ "taskId": taken }));
                assert_eq!(error_code(&reply), -32602, "{method} {taken}: {reply}");
            }
        };
        attempts(&mut client, "task-1");
        attempts(&mut client, "tpt.zz.zz");
        attempts(&mut stranger, id.as_str());
        let missing = client.request("tasks/get", json!({}));
        assert_eq!(error_code(&missing), -32602);
        assert!(case.received("tasks/get").is_empty() && case.received("tasks/cancel").is_empty());
        let alive = client.request("tasks/get", json!({ "taskId": id }));
        assert_eq!(alive["result"]["status"], "working");
        let none = stranger.request("tasks/list", json!({}));
        assert_eq!(
            none["result"]["tasks"],
            json!([]),
            "another session's tasks are not listed"
        );
    }

    #[test]
    fn test08_a_restarted_server_loses_its_tasks_and_their_ids_are_dropped() {
        let _guard = case_lock();
        let case = Case::new("restart", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 30 })));
        case.kill_server(&case.log);
        let first = client.request("tasks/get", json!({ "taskId": id }));
        assert_eq!(error_code(&first), -32602, "{first}");
        let second = client.request("tasks/get", json!({ "taskId": id }));
        assert_eq!(error_code(&second), -32602, "{second}");
        assert!(
            second["error"]["message"]
                .as_str()
                .unwrap()
                .contains("no longer available"),
            "{second}"
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        let fresh = loop {
            client.send(json!({
                "jsonrpc": "2.0", "id": 11, "method": "tools/call",
                "params": { "name": job, "arguments": { "seconds": 0.2 }, "task": {} }
            }));
            let reply = client.response(&json!(11), ANSWER_WITHIN).1;
            if let Some(fresh) = reply["result"]["task"]["taskId"].as_str() {
                break fresh.to_string();
            }
            assert!(
                Instant::now() < deadline,
                "the server never came back: {reply}"
            );
            std::thread::sleep(Duration::from_millis(500));
        };
        assert_ne!(fresh, id);
        let done = client.request("tasks/result", json!({ "taskId": fresh }));
        assert_eq!(
            done["result"]["content"][0]["text"],
            "job task-1 done after 0.2 s"
        );
        let old = client.request("tasks/result", json!({ "taskId": id }));
        assert_eq!(error_code(&old), -32602);
    }

    #[test]
    fn test09_a_status_notification_reaches_the_client_that_created_the_task() {
        let _guard = case_lock();
        let case = Case::new("status", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 0.5 })));
        let mut other = tasking_client(&case, SERVER_ID);
        client.listen(Duration::from_millis(2500));
        other.listen(Duration::from_millis(300));
        let statuses = |client: &Client| {
            client
                .inbox
                .iter()
                .filter(|(_, m)| m["method"] == "notifications/tasks/status")
                .map(|(_, m)| m["params"].clone())
                .collect::<Vec<_>>()
        };
        let seen = statuses(&client);
        assert_eq!(seen.len(), 1, "{:?}", client.inbox);
        assert_eq!(seen[0]["taskId"], json!(id));
        assert_eq!(seen[0]["status"], "completed");
        assert!(statuses(&other).is_empty());
    }

    #[test]
    fn test10_cancelling_a_waiting_tasks_result_is_forwarded_and_leaves_the_task_running() {
        let _guard = case_lock();
        let case = Case::new("result-cancel", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 30 })));
        client.start_request(11, "tasks/result", json!({ "taskId": id }));
        std::thread::sleep(Duration::from_millis(600));
        client.cancel(11, "no longer needed");
        let seen = (0..60).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            !case.received("notifications/cancelled").is_empty()
        });
        assert!(seen, "the server never heard of the cancel");
        let cancelled = &case.received("notifications/cancelled")[0];
        assert_eq!(cancelled["reason"], "no longer needed");
        let still = client.request("tasks/get", json!({ "taskId": id }));
        assert_eq!(still["result"]["status"], "working");
    }

    #[test]
    fn test11_a_client_that_did_not_declare_tasks_sees_todays_behaviour() {
        let _guard = case_lock();
        let case = Case::new("old-client", TASKS, timeouts());
        let mut client = case.client();
        client.initialize();
        let job = client.tool_of(SERVER_ID, "slow_job");
        let plain = create(&mut client, 10, &job, json!({ "seconds": 0 }));
        assert_eq!(plain["content"][0]["text"], "slept 0 s");
        client.send(json!({
            "jsonrpc": "2.0", "id": 11, "method": "tools/call",
            "params": { "name": job, "arguments": { "seconds": 0 }, "task": { "ttl": 1 } }
        }));
        let reply = client.response(&json!(11), ANSWER_WITHIN).1;
        assert_eq!(
            reply["result"]["content"][0]["text"], "slept 0 s",
            "{reply}"
        );
        assert!(
            case.received_calls("slow_job")
                .iter()
                .all(|call| call["task"].is_null()),
            "no task parameter reaches the server"
        );
        let get = client.request("tasks/get", json!({ "taskId": "x" }));
        assert_eq!(error_code(&get), -32601);
        assert_eq!(
            get["error"]["message"],
            "Tasks extension requires MCP 2026-07-28"
        );
        let result = client.request("tasks/result", json!({ "taskId": "x" }));
        assert_eq!(result["error"]["message"], "Method not found: tasks/result");
    }

    #[test]
    fn test12_ordinary_calls_and_lazy_calls_on_a_task_server_are_unaffected() {
        let _guard = case_lock();
        let case = Case::new("ordinary", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let sleep = client.tool_of(SERVER_ID, "sleep");
        client.start_call(
            10,
            &sleep,
            json!({ "seconds": 1.5, "progress_every_ms": 300 }),
            Some("pt"),
        );
        let (_, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(reply["result"]["content"][0]["text"], "slept 1.5 s");
        assert!(
            client.progress_seen("pt") >= 2,
            "progress still reaches the client"
        );
        let required = client.tool_of(SERVER_ID, "task_only");
        client.start_call(11, &required, json!({ "seconds": 0 }), None);
        let (_, refused) = client.response(&json!(11), ANSWER_WITHIN);
        assert!(
            error_text(&refused).contains("Task augmentation required"),
            "{refused}"
        );

        let mut lazy = case.lazy_client();
        lazy.initialize_with(json!({ "tasks": {} }));
        lazy.start_meta_call(20, &required, json!({ "seconds": 0 }), None);
        let (_, via_lazy) = lazy.response(&json!(20), ANSWER_WITHIN);
        assert!(
            error_text(&via_lazy).contains("Task augmentation required"),
            "{via_lazy}"
        );
    }
}
