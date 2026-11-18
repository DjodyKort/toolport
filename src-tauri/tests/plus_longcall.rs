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

    #[test]
    fn test04_longcall_without_a_client_token_the_call_is_not_extended() {
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
        assert!(timed_out(&reply), "{reply}");
        assert!(elapsed_ms(sent, at) < WINDOW_MS + 1500);
        let call = &case.received_calls("sleep")[0];
        assert!(
            call.get("token").is_none(),
            "the gateway mints no token of its own for the server: {call}"
        );
        assert_eq!(client.progress_seen("client-token"), 0);
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

    #[test]
    fn test06_longcall_the_listing_keeps_the_budget_and_the_call_ignores_it() {
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
        assert!(timed_out(&reply), "{reply}");
        assert!(
            elapsed_ms(sent, at) < WINDOW_MS + 1500,
            "the 120 s budget is not the window"
        );
    }

    #[test]
    fn test07_longcall_a_second_call_to_a_concurrent_server_answers_during_the_first() {
        let _guard = case_lock();
        let case = Case::new(
            "parallel",
            Behaviour::default(),
            Timeouts {
                request_ms: Some(10_000),
                max_request_ms: None,
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
                max_request_ms: None,
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

    #[test]
    fn test09_longcall_a_deadline_that_expires_does_not_cancel_the_call_at_the_server() {
        let _guard = case_lock();
        let case = Case::new("expiry", Behaviour::default(), timeouts(None));
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        client.start_call(10, &sleep, json!({ "seconds": 2 }), None);
        let (_, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert!(timed_out(&reply), "{reply}");
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            case.received("notifications/cancelled").is_empty(),
            "no cancellation is sent when the deadline expires: {:?}",
            case.entries()
        );
    }

    #[test]
    fn test10_longcall_the_late_answer_after_a_timeout_never_reaches_the_client() {
        let _guard = case_lock();
        let case = Case::new("late", Behaviour::default(), timeouts(None));
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
                max_request_ms: None,
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
                max_request_ms: None,
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

    #[test]
    fn test13_longcall_a_client_that_disconnects_mid_call_does_not_cancel_at_the_server() {
        let _guard = case_lock();
        let case = Case::new(
            "disconnect",
            Behaviour::default(),
            Timeouts {
                request_ms: Some(10_000),
                max_request_ms: None,
            },
        );
        let mut client = started_client(&case);
        let sleep = client.tool_named("sleep");
        client.start_call(10, &sleep, json!({ "seconds": 4 }), None);
        assert!(
            wait_until(
                || !case.received_calls("sleep").is_empty(),
                Duration::from_secs(10)
            ),
            "the sleep reached the server"
        );
        client.close_stdin();
        assert!(
            !wait_until(
                || !case.received("notifications/cancelled").is_empty(),
                Duration::from_secs(3)
            ),
            "no cancellation follows a disconnect: {:?}",
            case.entries()
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
                max_request_ms: None,
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

    #[test]
    fn test16_longcall_through_toolport_call_tool_without_a_client_token_the_call_is_not_extended()
    {
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
        assert!(timed_out(&reply), "{reply}");
        assert!(elapsed_ms(sent, at) < WINDOW_MS + 1500);
        assert!(
            case.received_calls("sleep")[0].get("token").is_none(),
            "the server is told no token"
        );
    }
}
