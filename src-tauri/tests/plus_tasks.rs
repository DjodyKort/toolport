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
        tasks_only_at_2025_11_25: false,
        task_page: 0,
        job_budget_s: 0,
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

    fn wait_until(mut predicate: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        predicate()
    }

    fn task_rows(case: &Case) -> Vec<Value> {
        case.audit()
            .into_iter()
            .filter(|row| row["kind"] == "task")
            .collect()
    }

    fn request(client: &mut Client, method: &str, task: &str) -> Value {
        client.request(method, json!({ "taskId": task }))
    }

    fn fake_server_sent(case: &Case, method: &str) -> usize {
        case.received(method).len()
    }

    #[test]
    fn test13_every_tasks_action_is_audited_with_its_tool_and_its_action() {
        let _guard = case_lock();
        let case = Case::new("audit", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let first = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 0.3 })));
        let second = task_id(&with_task(&mut client, 11, &job, json!({ "seconds": 5 })));
        assert_eq!(
            request(&mut client, "tasks/get", &first)["result"]["taskId"],
            first
        );
        assert!(client.request("tasks/list", json!({}))["result"]["tasks"].is_array());
        assert!(request(&mut client, "tasks/result", &first)["result"]["content"].is_array());
        assert_eq!(
            request(&mut client, "tasks/cancel", &second)["result"]["status"],
            "cancelled"
        );

        assert!(
            wait_until(|| task_rows(&case).len() >= 4),
            "{:?}",
            case.audit()
        );
        let rows = task_rows(&case);
        let action = |name: &str| {
            rows.iter()
                .find(|row| row["action"] == name)
                .unwrap_or_else(|| panic!("no audit row for {name}: {rows:?}"))
                .clone()
        };
        for name in ["tasks/get", "tasks/result"] {
            let row = action(name);
            assert_eq!(row["tool"], "slow_job", "{row}");
            assert_eq!(row["taskId"], first);
            assert_eq!(row["serverId"], SERVER_ID);
            assert_eq!(row["outcome"], "ok");
        }
        assert_eq!(action("tasks/cancel")["taskId"], second);
        let listed = action("tasks/list");
        assert_eq!(listed["serverId"], SERVER_ID);
        assert!(
            listed.get("tool").is_none() && listed.get("ok").is_none(),
            "{listed}"
        );
    }

    #[test]
    fn test14_stricter_safety_after_the_task_began_refuses_get_and_result_but_not_cancel() {
        let _guard = case_lock();
        let case = Case::new("strict", TASKS, timeouts());
        let _broker = case.broker(&["approved"]);
        let mut client = tasking_client(&case, SERVER_ID);
        let reset = client.tool_of(SERVER_ID, "reset_job");
        let id = task_id(&with_task(
            &mut client,
            10,
            &reset,
            json!({ "seconds": 30 }),
        ));
        assert!(request(&mut client, "tasks/get", &id)
            .get("result")
            .is_some());

        case.edit_registry(|registry| registry["safetyLevel"] = json!("strict"));
        assert!(
            wait_until(|| request(&mut client, "tasks/get", &id)
                .get("error")
                .is_some()),
            "the daemon takes the change up"
        );
        let reached = fake_server_sent(&case, "tasks/get");
        for method in ["tasks/get", "tasks/result"] {
            let refused = request(&mut client, method, &id);
            assert_eq!(error_code(&refused), -32602, "{refused}");
            assert!(
                refused["error"]["message"].as_str().is_some_and(
                    |message| message.contains("tool of this task is no longer available")
                ),
                "{refused}"
            );
        }
        assert_eq!(fake_server_sent(&case, "tasks/get"), reached);
        assert_eq!(fake_server_sent(&case, "tasks/result"), 0);

        let cancelled = request(&mut client, "tasks/cancel", &id);
        assert_eq!(cancelled["result"]["status"], "cancelled", "{cancelled}");
        assert!(
            wait_until(|| task_rows(&case)
                .iter()
                .any(|row| row["action"] == "tasks/get" && row["outcome"] == "refused")
                && task_rows(&case)
                    .iter()
                    .any(|row| row["action"] == "tasks/cancel" && row["outcome"] == "ok")),
            "{:?}",
            task_rows(&case)
        );
    }

    #[test]
    fn test15_a_tool_disabled_after_the_task_began_refuses_its_result() {
        let _guard = case_lock();
        let case = Case::new("tool-off", TASKS, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 0.2 })));
        case.edit_registry(|registry| {
            registry["servers"][0]["disabledTools"] = json!(["slow_job"]);
        });
        assert!(
            wait_until(|| request(&mut client, "tasks/result", &id)
                .get("error")
                .is_some()),
            "the daemon takes the change up"
        );
        let reached = fake_server_sent(&case, "tasks/result");
        let refused = request(&mut client, "tasks/result", &id);
        assert_eq!(error_code(&refused), -32602, "{refused}");
        assert_eq!(fake_server_sent(&case, "tasks/result"), reached);
        assert!(
            wait_until(|| task_rows(&case)
                .iter()
                .any(|row| row["action"] == "tasks/result" && row["outcome"] == "refused")),
            "{:?}",
            task_rows(&case)
        );
    }

    #[test]
    fn test16_the_result_of_a_destructive_tool_asks_a_human_and_get_and_list_do_not() {
        let _guard = case_lock();
        let case = Case::new("approval", TASKS, timeouts());
        let broker = case.broker(&["approved"]);
        let mut client = tasking_client(&case, SERVER_ID);
        let reset = client.tool_of(SERVER_ID, "reset_job");
        let id = task_id(&with_task(
            &mut client,
            10,
            &reset,
            json!({ "seconds": 0.2 }),
        ));
        assert_eq!(broker.requests().len(), 1, "the creation was asked about");
        assert_eq!(
            request(&mut client, "tasks/get", &id)["result"]["taskId"],
            id
        );
        assert!(client.request("tasks/list", json!({}))["result"]["tasks"].is_array());
        assert_eq!(broker.requests().len(), 1, "polling asks nothing");

        let result = request(&mut client, "tasks/result", &id);
        assert!(result["result"]["content"].is_array(), "{result}");
        let asked = broker.requests();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[1].tool, "reset_job");
        assert_eq!(asked[1].server, SERVER_ID);
        assert_eq!(asked[1].arguments["taskAction"], "tasks/result");
        assert_eq!(asked[1].arguments["taskId"], id);
        assert!(
            wait_until(|| case.audit().iter().any(|row| row["kind"] == "approval"
                && row["decision"] == "approved"
                && row["tool"] == "reset_job"
                && row["held"] == false)
                && task_rows(&case)
                    .iter()
                    .any(|row| row["action"] == "tasks/result" && row["outcome"] == "ok")),
            "{:?}",
            case.audit()
        );
    }

    #[test]
    fn test17_a_denied_result_never_reaches_the_server_and_leaves_polling_alone() {
        let _guard = case_lock();
        let case = Case::new("denied", TASKS, timeouts());
        let broker = case.broker(&["approved", "denied"]);
        let mut client = tasking_client(&case, SERVER_ID);
        let reset = client.tool_of(SERVER_ID, "reset_job");
        let id = task_id(&with_task(
            &mut client,
            10,
            &reset,
            json!({ "seconds": 0.2 }),
        ));
        let refused = request(&mut client, "tasks/result", &id);
        assert_eq!(error_code(&refused), -32602, "{refused}");
        assert!(
            refused["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("denied by a human reviewer")),
            "{refused}"
        );
        assert_eq!(fake_server_sent(&case, "tasks/result"), 0);
        assert_eq!(broker.requests().len(), 2);
        assert!(request(&mut client, "tasks/get", &id)
            .get("result")
            .is_some());
        assert!(
            wait_until(|| case.audit().iter().any(|row| row["kind"] == "approval"
                && row["decision"] == "denied"
                && row["tool"] == "reset_job")
                && task_rows(&case)
                    .iter()
                    .any(|row| row["action"] == "tasks/result" && row["outcome"] == "refused")),
            "{:?}",
            case.audit()
        );
    }

    #[test]
    fn test18_a_non_destructive_task_asks_nobody() {
        let _guard = case_lock();
        let case = Case::new("no-approval", TASKS, timeouts());
        let broker = case.broker(&["denied"]);
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 0.2 })));
        let result = request(&mut client, "tasks/result", &id);
        assert!(result["result"]["content"].is_array(), "{result}");
        assert!(broker.requests().is_empty());
    }

    const LEGACY_TODAY: &str = "2025-06-18";
    const TASKS_REVISION: &str = "2025-11-25";

    fn handshake(case: &Case, version: &str, capabilities: Value) -> Value {
        let mut client = case.client();
        client.initialize();
        client.tool_of(SERVER_ID, "slow_job");
        client.initialize_at(version, capabilities)
    }

    #[test]
    fn test19_the_tasks_revision_is_negotiated_only_for_a_client_that_asks_for_it_and_declared_tasks(
    ) {
        let _guard = case_lock();
        let case = Case::new("negotiation", TASKS, timeouts());
        let tasks = json!({ "tasks": {} });

        let both = handshake(&case, TASKS_REVISION, tasks.clone());
        assert_eq!(both["protocolVersion"], TASKS_REVISION);
        assert!(both["capabilities"].get("tasks").is_some(), "{both}");

        let newer = handshake(&case, "2099-01-01", tasks.clone());
        assert_eq!(newer["protocolVersion"], TASKS_REVISION, "{newer}");
        assert!(newer["capabilities"].get("tasks").is_some(), "{newer}");

        let older = handshake(&case, LEGACY_TODAY, tasks.clone());
        assert_eq!(older["protocolVersion"], LEGACY_TODAY, "{older}");
        assert!(older["capabilities"].get("tasks").is_none(), "{older}");

        let unknown = handshake(&case, "1999-01-01", tasks);
        assert_eq!(unknown["protocolVersion"], LEGACY_TODAY, "{unknown}");
        assert!(unknown["capabilities"].get("tasks").is_none(), "{unknown}");
    }

    #[test]
    fn test20_a_client_without_tasks_is_negotiated_exactly_as_before() {
        let _guard = case_lock();
        let case = Case::new("negotiation-plain", TASKS, timeouts());
        for (asked, answered) in [
            (TASKS_REVISION, TASKS_REVISION),
            ("2026-07-28", "2026-07-28"),
            ("2099-01-01", LEGACY_TODAY),
            (LEGACY_TODAY, LEGACY_TODAY),
            ("2024-11-05", "2024-11-05"),
            ("1999-01-01", LEGACY_TODAY),
        ] {
            let reply = handshake(&case, asked, json!({}));
            assert_eq!(reply["protocolVersion"], answered, "asked for {asked}");
            assert!(reply["capabilities"].get("tasks").is_none(), "{reply}");
        }
    }

    #[test]
    fn test21_a_client_that_declared_tasks_at_an_older_revision_cannot_create_a_task() {
        let _guard = case_lock();
        let case = Case::new("negotiation-old", TASKS, timeouts());
        let mut client = case.client();
        client.initialize();
        let job = client.tool_of(SERVER_ID, "slow_job");
        let reply = client.initialize_at(LEGACY_TODAY, json!({ "tasks": {} }));
        assert!(reply["capabilities"].get("tasks").is_none(), "{reply}");
        let plain = with_task(&mut client, 10, &job, json!({ "seconds": 0 }));
        assert_eq!(plain["content"][0]["text"], "slept 0 s", "{plain}");
        assert!(
            case.received_calls("slow_job")
                .iter()
                .all(|call| call["task"].is_null()),
            "no task parameter reaches the server"
        );
    }

    #[test]
    fn test22_a_stdio_server_is_offered_the_tasks_revision_and_may_answer_lower() {
        let _guard = case_lock();
        let case = Case::pair_with("offer", Behaviour::default(), timeouts(), TASKS, timeouts());
        let mut client = case.client();
        client.initialize();
        client.tool_of(SERVER_ID, "echo");
        client.tool_of(PEER_ID, "slow_job");
        for log in [&case.log, &case.peer_log] {
            let offered: Vec<Value> = Case::entries_of(log)
                .into_iter()
                .filter(|entry| entry["dir"] == "in" && entry["method"] == "initialize")
                .map(|entry| entry["version"].clone())
                .collect();
            assert_eq!(offered, vec![json!(TASKS_REVISION)], "{}", log.display());
        }
    }

    #[test]
    fn test23_a_server_that_declares_tasks_only_at_the_tasks_revision_gets_them_relayed() {
        let _guard = case_lock();
        let gated = Behaviour {
            tasks_only_at_2025_11_25: true,
            ..TASKS
        };
        let case = Case::new("gated", gated, timeouts());
        let mut client = tasking_client(&case, SERVER_ID);
        let job = client.tool_of(SERVER_ID, "slow_job");
        let id = task_id(&with_task(&mut client, 10, &job, json!({ "seconds": 0 })));
        let result = client.request("tasks/result", json!({ "taskId": id }));
        assert_eq!(
            result["result"]["content"][0]["text"], "job task-1 done after 0 s",
            "{result}"
        );
        assert_eq!(
            case.received_calls("slow_job")[0]["task"],
            json!({ "ttl": 60000 })
        );
    }
}
