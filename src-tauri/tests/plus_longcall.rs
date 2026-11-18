//! Characterisation of how the gateway treats long-running `tools/call`s today, against a fake
//! slow MCP stdio server (`plus/fixtures/stdio/slow-mcp.mjs`). Each case names the behaviour it
//! pins so a later change flips the assertion on purpose.
//!
//! The progress re-arm cases (`test02`, `test03`, `test15`) exist only with the `plus` feature;
//! without it the gateway is upstream's and keeps a fixed deadline.
//!
//! Timings: `requestTimeoutMs` is 1 s in the registry, calls last 2-3 s, margins are wide.

mod longcall_support;

mod longcall {
    use super::longcall_support::*;
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};

    const WINDOW_MS: u64 = 1000;
    const ANSWER_WITHIN: Duration = Duration::from_secs(20);

    fn timeouts(max_request_ms: Option<u64>) -> Timeouts {
        Timeouts {
            request_ms: Some(WINDOW_MS),
            max_request_ms,
            ..Default::default()
        }
    }

    fn timed_out(reply: &Value) -> bool {
        error_text(reply).contains("timed out waiting for 'tools/call' response")
    }

    fn elapsed_ms(since: Instant, at: Instant) -> u64 {
        at.duration_since(since).as_millis() as u64
    }

    fn started_client(case: &Case) -> Client {
        let mut client = case.client();
        client.initialize();
        client
    }

    fn wait_until(mut predicate: impl FnMut() -> bool, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        predicate()
    }

    #[test]
    fn test01_longcall_a_call_outliving_the_window_without_progress_times_out() {
        let _guard = case_lock();
        let case = Case::new("timeout", Behaviour::default(), timeouts(None));
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let sent = client.start_call(10, &sleep, json!({ "seconds": 3 }), None);
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert!(timed_out(&reply), "{reply}");
        assert!(
            error_text(&reply).contains("Timed out after send"),
            "the result says the call may have completed: {reply}"
        );
        let waited = elapsed_ms(sent, at);
        assert!(
            (WINDOW_MS - 100..WINDOW_MS + 1500).contains(&waited),
            "cut at the window, not later: {waited} ms"
        );
        assert!(
            !error_text(&reply).contains("absolute cap"),
            "no cap note without progress: {reply}"
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test02_longcall_progress_under_the_clients_token_extends_the_deadline() {
        let _guard = case_lock();
        let case = Case::new("rearm", Behaviour::default(), timeouts(Some(8000)));
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let sent = client.start_call(
            10,
            &sleep,
            json!({ "seconds": 2.5, "progress_every_ms": 300 }),
            Some("client-token"),
        );
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "slept 2.5 s", "{reply}");
        assert!(elapsed_ms(sent, at) >= 2400, "ran past the 1 s window");
        assert!(
            client.progress_seen("client-token") > 0,
            "progress reaches the client under its own token: {:?}",
            client.inbox
        );
        let server_token = case.received_calls("sleep")[0]["token"].clone();
        assert!(
            server_token
                .as_str()
                .is_some_and(|token| token.starts_with("tp-")),
            "the server is told a route token the gateway minted for the client's: {server_token}"
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test03_longcall_the_cap_ends_a_call_that_keeps_reporting_progress() {
        let _guard = case_lock();
        let case = Case::new("cap", Behaviour::default(), timeouts(Some(3000)));
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let sent = client.start_call(
            10,
            &sleep,
            json!({ "seconds": 20, "progress_every_ms": 300 }),
            Some("client-token"),
        );
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert!(timed_out(&reply), "{reply}");
        assert!(
            error_text(&reply).contains("absolute cap (maxRequestTimeoutMs)"),
            "{reply}"
        );
        let waited = elapsed_ms(sent, at);
        assert!(
            (2800..5500).contains(&waited),
            "ended at the 3 s cap: {waited} ms"
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test04_longcall_without_a_client_token_a_minted_token_extends_the_call_and_reaches_no_client(
    ) {
        let _guard = case_lock();
        let case = Case::new("notoken", Behaviour::default(), timeouts(Some(8000)));
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let sent = client.start_call(
            10,
            &sleep,
            json!({ "seconds": 2.5, "progress_every_ms": 300 }),
            None,
        );
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "slept 2.5 s", "{reply}");
        assert!(elapsed_ms(sent, at) >= 2400, "ran past the 1 s window");
        let token = case.received_calls("sleep")[0]["token"].clone();
        assert!(
            token
                .as_str()
                .is_some_and(|token| token.starts_with("tpx-")),
            "the server is told a token the gateway minted: {token}"
        );
        client.listen(Duration::from_millis(300));
        assert!(
            client
                .inbox
                .iter()
                .all(|(_, message)| message["method"] != "notifications/progress"),
            "the client asked for no progress and gets none: {:?}",
            client.inbox
        );
    }

    #[test]
    fn test05_longcall_progress_for_another_token_does_not_extend_the_deadline() {
        let _guard = case_lock();
        let case = Case::new("othertoken", Behaviour::default(), timeouts(Some(8000)));
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let sent = client.start_call(
            10,
            &sleep,
            json!({ "seconds": 2.5, "progress_every_ms": 300, "progress_for": "unknown" }),
            Some("client-token"),
        );
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert!(timed_out(&reply), "{reply}");
        assert!(elapsed_ms(sent, at) < WINDOW_MS + 1500);
        client.listen(Duration::from_millis(300));
        assert_eq!(
            client.progress_seen("never-issued"),
            0,
            "progress for a token nobody holds is not relayed: {:?}",
            client.inbox
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test06_longcall_the_budget_in_the_listing_is_the_window_of_the_call() {
        let _guard = case_lock();
        let case = Case::new("budget", Behaviour::default(), timeouts(None));
        let mut client = started_client(&case);
        let (name, listed) = client.tool("budgeted_sleep");
        assert_eq!(
            listed["_meta"]["odh.dev/budget"],
            json!(120),
            "the tool's own _meta passes through the gateway listing: {listed}"
        );
        let sent = client.start_call(10, &name, json!({ "seconds": 3 }), None);
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "slept 3 s", "{reply}");
        assert!(
            elapsed_ms(sent, at) >= 2900,
            "a 3 s call without any progress outlives the 1 s server window"
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test17_longcall_a_tool_budget_of_the_server_beats_the_budget_in_the_listing() {
        let _guard = case_lock();
        let case = Case::new(
            "tool-budgets",
            Behaviour::default(),
            Timeouts {
                request_ms: Some(10_000),
                tool_budgets: &[("budgeted_sleep", 1.0), ("sleep", 5.0)],
                ..Default::default()
            },
        );
        let mut client = started_client(&case);
        let budgeted = client.tool_named("budgeted_sleep");
        let sleep = client.tool_named("sleep");
        let sent = client.start_call(10, &budgeted, json!({ "seconds": 3 }), None);
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert!(timed_out(&reply), "{reply}");
        let waited = elapsed_ms(sent, at);
        assert!(
            (900..2500).contains(&waited),
            "cut at the 1 s toolBudgets entry, not the 120 s listing budget nor the 10 s window: {waited} ms"
        );
        let sent = client.start_call(11, &sleep, json!({ "seconds": 3 }), None);
        let (at, reply) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "slept 3 s", "{reply}");
        assert!(elapsed_ms(sent, at) >= 2900);
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test18_longcall_the_cap_beats_a_tool_budget_and_the_listing_budget() {
        let _guard = case_lock();
        let case = Case::new(
            "budget-cap",
            Behaviour::default(),
            Timeouts {
                request_ms: Some(WINDOW_MS),
                max_request_ms: Some(1500),
                tool_budgets: &[("sleep", 600.0)],
                ..Default::default()
            },
        );
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let budgeted = client.tool_named("budgeted_sleep");
        for (id, tool) in [(10, &sleep), (11, &budgeted)] {
            let sent = client.start_call(id, tool, json!({ "seconds": 5 }), None);
            let (at, reply) = client.response(&json!(id), ANSWER_WITHIN);
            assert!(timed_out(&reply), "{reply}");
            let waited = elapsed_ms(sent, at);
            assert!(
                (1400..3000).contains(&waited),
                "{tool}: cut at the 1.5 s cap: {waited} ms"
            );
        }
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test19_longcall_concurrent_calls_with_different_tokens_do_not_mix_progress() {
        let _guard = case_lock();
        let case = Case::new("two-tokens", Behaviour::default(), timeouts(Some(8000)));
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        client.start_call(
            10,
            &sleep,
            json!({ "seconds": 3, "progress_every_ms": 200 }),
            Some("token-long"),
        );
        client.start_call(
            11,
            &sleep,
            json!({ "seconds": 1.2, "progress_every_ms": 200 }),
            Some("token-short"),
        );
        client.start_call(
            12,
            &sleep,
            json!({ "seconds": 1.2, "progress_every_ms": 200 }),
            None,
        );
        for id in [10, 11, 12] {
            let (_, reply) = client.response(&json!(id), ANSWER_WITHIN);
            assert!(reply.get("result").is_some(), "{reply}");
        }
        let progressed = |token: &str| -> Vec<i64> {
            client
                .inbox
                .iter()
                .filter(|(_, message)| {
                    message["method"] == "notifications/progress"
                        && message["params"]["progressToken"] == token
                })
                .filter_map(|(_, message)| message["params"]["progress"].as_i64())
                .collect()
        };
        let long = progressed("token-long");
        let short = progressed("token-short");
        assert!(!long.is_empty() && !short.is_empty(), "{:?}", client.inbox);
        assert!(
            short.iter().all(|ms| *ms <= 1700),
            "progress of the 1.2 s call carries only its own time: {short:?}"
        );
        assert!(long.iter().any(|ms| *ms > 2000), "{long:?}");
        let tokens: Vec<_> = case
            .received_calls("sleep")
            .iter()
            .map(|call| call["token"].as_str().unwrap_or_default().to_string())
            .collect();
        let distinct: std::collections::HashSet<_> = tokens.iter().collect();
        assert_eq!(distinct.len(), 3, "each call has its own token: {tokens:?}");
    }

    #[test]
    fn test07_longcall_a_second_call_to_a_concurrent_server_answers_during_the_first() {
        let _guard = case_lock();
        let case = Case::new(
            "parallel",
            Behaviour::default(),
            Timeouts {
                request_ms: Some(10_000),
                ..Default::default()
            },
        );
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let echo = client.tool_named("echo");
        let sleep_sent = client.start_call(10, &sleep, json!({ "seconds": 3 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the sleep reached the server"
        );
        let echo_sent = client.start_call(11, &echo, json!({ "text": "hi" }), None);
        let (echo_at, echo_reply) = client.response(&json!(11), ANSWER_WITHIN);
        let (sleep_at, sleep_reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(text_of(&echo_reply["result"]), "hi", "{echo_reply}");
        assert_eq!(
            text_of(&sleep_reply["result"]),
            "slept 3 s",
            "{sleep_reply}"
        );
        let echo_took = elapsed_ms(echo_sent, echo_at);
        let sleep_took = elapsed_ms(sleep_sent, sleep_at);
        assert!(
            echo_took < 1500 && sleep_took >= 2900,
            "stdio calls to one server already run side by side: echo {echo_took} ms, sleep {sleep_took} ms"
        );
    }

    #[test]
    fn test08_longcall_a_serial_server_makes_the_second_call_wait_for_the_first() {
        let _guard = case_lock();
        let case = Case::new(
            "serial",
            Behaviour {
                serial: true,
                ..Behaviour::default()
            },
            Timeouts {
                request_ms: Some(10_000),
                ..Default::default()
            },
        );
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let echo = client.tool_named("echo");
        client.start_call(10, &sleep, json!({ "seconds": 2 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the sleep reached the server"
        );
        let echo_sent = client.start_call(11, &echo, json!({ "text": "hi" }), None);
        let (echo_at, echo_reply) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(text_of(&echo_reply["result"]), "hi", "{echo_reply}");
        assert!(
            elapsed_ms(echo_sent, echo_at) >= 1200,
            "the server holds the echo until the sleep is done"
        );
        let calls = case.received("tools/call");
        assert_eq!(
            calls.len(),
            2,
            "the gateway delivered both calls without waiting: {calls:?}"
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test09_longcall_a_deadline_that_expires_cancels_the_call_at_the_server() {
        let _guard = case_lock();
        let case = Case::new("expiry", Behaviour::default(), timeouts(None));
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        client.start_call(10, &sleep, json!({ "seconds": 2 }), None);
        let (_, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert!(timed_out(&reply), "{reply}");
        assert!(
            wait_until(
                || !case.received("notifications/cancelled").is_empty(),
                Duration::from_secs(3)
            ),
            "the server is told the call is cancelled: {:?}",
            case.entries()
        );
        let cancelled = &case.received("notifications/cancelled")[0];
        assert_eq!(
            cancelled["requestId"],
            case.received_calls("sleep")[0]["id"],
            "the server's own id of the call"
        );
        assert!(
            cancelled["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("timed out")),
            "{cancelled}"
        );
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            !case.answered(&json!(3)),
            "a server that honours the cancel never answers: {:?}",
            case.entries()
        );
    }

    #[test]
    fn test10_longcall_the_late_answer_after_a_timeout_never_reaches_the_client() {
        let _guard = case_lock();
        let case = Case::new(
            "late",
            Behaviour {
                ignore_cancel: true,
                ..Default::default()
            },
            timeouts(None),
        );
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let echo = client.tool_named("echo");
        client.start_call(10, &sleep, json!({ "seconds": 2 }), None);
        let (_, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert!(timed_out(&reply), "{reply}");
        assert!(
            wait_until(|| case.answered(&json!(3)), Duration::from_secs(10)),
            "the server answered its call late: {:?}",
            case.entries()
        );
        client.listen(Duration::from_millis(500));
        assert!(
            client
                .inbox
                .iter()
                .all(|(_, message)| message.get("method").is_some()),
            "nothing but notifications after the error: {:?}",
            client.inbox
        );
        client.start_call(11, &echo, json!({ "text": "after" }), None);
        let (_, after) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(text_of(&after["result"]), "after", "{after}");
        assert_eq!(
            case.received("initialize").len(),
            1,
            "a timeout does not restart the server"
        );
    }

    #[test]
    fn test11_longcall_a_client_cancel_reaches_the_server_with_its_reason() {
        let _guard = case_lock();
        let case = Case::new(
            "cancel",
            Behaviour::default(),
            Timeouts {
                request_ms: Some(10_000),
                ..Default::default()
            },
        );
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        client.start_call(10, &sleep, json!({ "seconds": 5 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the sleep reached the server"
        );
        let server_id = case.received_calls("sleep")[0]["id"].clone();
        client.cancel(10, "user cancelled");
        assert!(
            wait_until(
                || !case.received("notifications/cancelled").is_empty(),
                Duration::from_secs(10)
            ),
            "the cancellation reached the server"
        );
        let cancelled = &case.received("notifications/cancelled")[0];
        assert_eq!(cancelled["requestId"], server_id, "{cancelled}");
        assert_eq!(cancelled["reason"], "user cancelled", "{cancelled}");
        client.listen(Duration::from_millis(800));
        assert!(
            client
                .inbox
                .iter()
                .all(|(_, message)| message.get("method").is_some()),
            "a cancelled call gets no answer: {:?}",
            client.inbox
        );
        assert!(
            !case.answered(&server_id),
            "the server stopped working on it"
        );
    }

    #[test]
    fn test12_longcall_the_answer_of_a_cancelled_call_is_dropped_when_the_server_ignores_cancel() {
        let _guard = case_lock();
        let case = Case::new(
            "cancel-ignored",
            Behaviour {
                ignore_cancel: true,
                ..Behaviour::default()
            },
            Timeouts {
                request_ms: Some(10_000),
                ..Default::default()
            },
        );
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        client.start_call(10, &sleep, json!({ "seconds": 1.5 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the sleep reached the server"
        );
        let server_id = case.received_calls("sleep")[0]["id"].clone();
        client.cancel(10, "user cancelled");
        assert!(
            wait_until(|| case.answered(&server_id), Duration::from_secs(10)),
            "the server answered anyway"
        );
        client.listen(Duration::from_millis(800));
        assert!(
            client
                .inbox
                .iter()
                .all(|(_, message)| message.get("method").is_some()),
            "the late answer is not passed on: {:?}",
            client.inbox
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test13_longcall_a_client_that_disconnects_mid_call_cancels_the_call_at_the_server() {
        let _guard = case_lock();
        let case = Case::new(
            "disconnect",
            Behaviour::default(),
            Timeouts {
                request_ms: Some(30_000),
                ..Default::default()
            },
        );
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        client.start_call(10, &sleep, json!({ "seconds": 25 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the sleep reached the server"
        );
        client.close_stdin();
        assert!(
            wait_until(
                || !case.received("notifications/cancelled").is_empty(),
                Duration::from_secs(15)
            ),
            "the server is told when the client is gone: {:?}",
            case.entries()
        );
        let cancelled = &case.received("notifications/cancelled")[0];
        assert_eq!(
            cancelled["requestId"],
            case.received_calls("sleep")[0]["id"]
        );
    }

    fn warmed_lazy_client(case: &Case) -> Client {
        let mut client = case.lazy_client();
        client.initialize();
        client.start_meta_call(1, "slow__echo", json!({ "text": "warm" }), None);
        let (_, warm) = client.response(&json!(1), ANSWER_WITHIN);
        assert_eq!(text_of(&warm["result"]), "warm", "{warm}");
        client
    }

    #[test]
    fn test14_longcall_through_toolport_call_tool_a_second_call_answers_during_the_first() {
        let _guard = case_lock();
        let case = Case::new(
            "lazy-parallel",
            Behaviour::default(),
            Timeouts {
                request_ms: Some(10_000),
                ..Default::default()
            },
        );
        let mut client = warmed_lazy_client(&case);
        client.start_meta_call(10, "slow__sleep", json!({ "seconds": 3 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the sleep reached the server"
        );
        let echo_sent = client.start_meta_call(11, "slow__echo", json!({ "text": "hi" }), None);
        let (echo_at, echo_reply) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(text_of(&echo_reply["result"]), "hi", "{echo_reply}");
        assert!(
            elapsed_ms(echo_sent, echo_at) < 1500,
            "the meta-tool path does not serialise either"
        );
        let (_, sleep_reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(
            text_of(&sleep_reply["result"]),
            "slept 3 s",
            "{sleep_reply}"
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test15_longcall_through_toolport_call_tool_the_clients_token_extends_the_deadline() {
        let _guard = case_lock();
        let case = Case::new("lazy-token", Behaviour::default(), timeouts(Some(8000)));
        let mut client = warmed_lazy_client(&case);
        let sent = client.start_meta_call(
            10,
            "slow__sleep",
            json!({ "seconds": 2.5, "progress_every_ms": 300 }),
            Some("client-token"),
        );
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "slept 2.5 s", "{reply}");
        assert!(elapsed_ms(sent, at) >= 2400, "ran past the 1 s window");
        assert!(
            client.progress_seen("client-token") > 0,
            "progress reaches the client: {:?}",
            client.inbox
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test16_longcall_through_toolport_call_tool_without_a_client_token_a_minted_token_extends_the_call(
    ) {
        let _guard = case_lock();
        let case = Case::new("lazy-notoken", Behaviour::default(), timeouts(Some(8000)));
        let mut client = warmed_lazy_client(&case);
        let sent = client.start_meta_call(
            10,
            "slow__sleep",
            json!({ "seconds": 2.5, "progress_every_ms": 300 }),
            None,
        );
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "slept 2.5 s", "{reply}");
        assert!(elapsed_ms(sent, at) >= 2400, "ran past the 1 s window");
        assert!(
            case.received_calls("sleep")[0]["token"]
                .as_str()
                .is_some_and(|token| token.starts_with("tpx-")),
            "the server is told a minted token"
        );
    }

    fn serial_timeouts() -> Timeouts {
        Timeouts {
            request_ms: Some(WINDOW_MS),
            serial: true,
            ..Default::default()
        }
    }

    fn answered_at(case: &Case, call: &Value) -> u64 {
        case.entries()
            .iter()
            .find(|entry| entry["dir"] == "out" && entry["id"] == call["id"])
            .and_then(|entry| entry["t"].as_u64())
            .unwrap_or_else(|| panic!("the server never answered {call}"))
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test20_longcall_a_serial_server_gets_one_call_at_a_time_and_the_queue_does_not_run_down_the_deadline(
    ) {
        let _guard = case_lock();
        let case = Case::new("gate", Behaviour::default(), serial_timeouts());
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let sent = client.start_call(10, &sleep, json!({ "seconds": 0.7 }), None);
        client.start_call(11, &sleep, json!({ "seconds": 0.7 }), None);
        let (_, first) = client.response(&json!(10), ANSWER_WITHIN);
        let (second_at, second) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(text_of(&first["result"]), "slept 0.7 s", "{first}");
        assert_eq!(
            text_of(&second["result"]),
            "slept 0.7 s",
            "the second call outlived the 1 s window in total but not after it was sent: {second}"
        );
        assert!(
            elapsed_ms(sent, second_at) >= 1350,
            "it ran after the first: {} ms, {:?}",
            elapsed_ms(sent, second_at),
            case.entries()
        );
        let calls = case.received_calls("sleep");
        assert_eq!(calls.len(), 2);
        assert!(
            calls[1]["t"].as_u64().unwrap() >= answered_at(&case, &calls[0]),
            "the second call was sent only after the first was answered: {:?}",
            case.entries()
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test21_longcall_a_call_cancelled_while_it_queues_is_never_sent_and_the_one_behind_it_runs() {
        let _guard = case_lock();
        let case = Case::new("gate-cancel", Behaviour::default(), serial_timeouts());
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        let echo = client.tool_named("echo");
        client.start_call(10, &sleep, json!({ "seconds": 0.8 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the first sleep reached the server"
        );
        client.start_call(11, &sleep, json!({ "seconds": 5 }), None);
        client.start_call(12, &echo, json!({ "text": "behind" }), None);
        std::thread::sleep(Duration::from_millis(250));
        client.cancel(11, "no longer needed");
        let (_, echo_reply) = client.response(&json!(12), ANSWER_WITHIN);
        assert_eq!(text_of(&echo_reply["result"]), "behind", "{echo_reply}");
        let sleeps = case.received_calls("sleep");
        assert_eq!(
            sleeps.len(),
            1,
            "the cancelled call never reached the server"
        );
        assert!(
            case.received("notifications/cancelled").is_empty(),
            "a call that was never sent needs no cancel: {:?}",
            case.entries()
        );
        let echoes = case.received_calls("echo");
        assert!(
            echoes[0]["t"].as_u64().unwrap() >= answered_at(&case, &sleeps[0]),
            "the echo waited for the running call"
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test22_longcall_through_toolport_call_tool_a_serial_server_queues_the_calls_too() {
        let _guard = case_lock();
        let case = Case::new("gate-lazy", Behaviour::default(), serial_timeouts());
        let mut client = warmed_lazy_client(&case);
        client.start_meta_call(10, "slow__sleep", json!({ "seconds": 0.7 }), None);
        client.start_meta_call(11, "slow__sleep", json!({ "seconds": 0.7 }), None);
        let (_, first) = client.response(&json!(10), ANSWER_WITHIN);
        let (_, second) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(text_of(&first["result"]), "slept 0.7 s", "{first}");
        assert_eq!(text_of(&second["result"]), "slept 0.7 s", "{second}");
        let calls = case.received_calls("sleep");
        assert!(
            calls[1]["t"].as_u64().unwrap() >= answered_at(&case, &calls[0]),
            "{:?}",
            case.entries()
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test23_longcall_progress_another_server_reports_for_a_calls_token_does_not_extend_it() {
        let _guard = case_lock();
        let case = Case::pair(
            "cross",
            Behaviour::default(),
            timeouts(Some(8000)),
            Timeouts {
                request_ms: Some(10_000),
                ..Default::default()
            },
        );
        let mut client = started_client(&case);
        let sleep = client.tool_of(SERVER_ID, "sleep");
        let emit = client.tool_of(PEER_ID, "emit_progress");
        let sent = client.start_call(10, &sleep, json!({ "seconds": 4 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the sleep reached its server"
        );
        let token = case.received_calls("sleep")[0]["token"].clone();
        assert!(token.is_string(), "the call carries a token: {token}");
        client.start_call(
            11,
            &emit,
            json!({ "token": token, "every_ms": 200, "count": 20 }),
            None,
        );
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert!(timed_out(&reply), "{reply}");
        let waited = elapsed_ms(sent, at);
        assert!(
            (WINDOW_MS - 100..WINDOW_MS + 1500).contains(&waited),
            "cut at its own window although another server reported progress for its token: {waited} ms"
        );
        let reported = Case::entries_of(&case.peer_log)
            .iter()
            .filter(|entry| entry["dir"] == "out" && entry["method"] == "notifications/progress")
            .count();
        assert!(
            reported >= 3,
            "the other server did report progress: {reported}"
        );
    }
}
