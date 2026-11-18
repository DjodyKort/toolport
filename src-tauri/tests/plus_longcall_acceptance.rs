//! Acceptance of the long-call work: ODH's `odoo_job_wait` behind the real gateway binary.
//!
//! The request: `odoo_job_wait` with `timeout_s=600` on a job that ends after 5 minutes returns its
//! result while a second `odoo_*` call to the same server answers within seconds; the client sends
//! no progress token and the server's per-call window is 30 s.
//!
//! Scale: the cases run 20 times faster than that. The registry's `requestTimeoutMs` is 1.5 s (the
//! 30 s window), `timeout_s` is 30 (the 600), a job lasts 6 s (the window times four, not the full
//! 5 min, to keep the file under a minute) and reports progress every 0.5 s (a third of the
//! window). What the cases prove is the ratio: a call several windows long, progress arriving at a
//! fraction of the window, a quick call answering while it runs.
#![cfg(feature = "plus")]

mod longcall_support;

mod acceptance {
    use super::longcall_support::*;
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};

    const WINDOW_MS: u64 = 1500;
    const JOB_S: u64 = 6;
    const TIMEOUT_S: u64 = 30;
    const ANSWER_WITHIN: Duration = Duration::from_secs(25);
    const QUICK: Duration = Duration::from_millis(1000);

    fn timeouts(max_request_ms: u64) -> Timeouts {
        Timeouts {
            request_ms: Some(WINDOW_MS),
            max_request_ms: Some(max_request_ms),
            ..Default::default()
        }
    }

    fn ready(case: &Case) -> (Client, String, String) {
        let mut client = case.client();
        client.initialize();
        let wait = client.tool_named("odoo_job_wait");
        let status = client.tool_named("odoo_status");
        (client, wait, status)
    }

    fn job(job_s: u64) -> Value {
        json!({ "timeout_s": TIMEOUT_S, "job_s": job_s })
    }

    fn since(start: Instant, at: Instant) -> Duration {
        at.duration_since(start)
    }

    fn answered_before(client: &mut Client, id: i64) -> bool {
        client.listen(Duration::from_millis(50));
        client
            .inbox
            .iter()
            .any(|(_, message)| message.get("method").is_none() && message["id"] == json!(id))
    }

    fn cancelled_requests(case: &Case) -> Vec<Value> {
        case.received("notifications/cancelled")
    }

    #[test]
    fn test_acceptance_01_job_wait_outlives_the_window_while_odoo_status_answers() {
        let _guard = case_lock();
        let case = Case::new("accept-main", Behaviour::default(), timeouts(60_000));
        let (mut client, wait, status) = ready(&case);

        let long = client.start_call(10, &wait, job(JOB_S), None);
        std::thread::sleep(Duration::from_millis(300));
        let second = client.start_call(11, &wait, job(3), None);
        let quick = client.start_call(12, &status, json!({}), None);
        let (at, reply) = client.response(&json!(12), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "odoo is up", "{reply}");
        assert!(since(quick, at) < QUICK, "a quick call answers at once");

        std::thread::sleep(Duration::from_millis(WINDOW_MS + 500));
        assert!(
            !answered_before(&mut client, 10),
            "the long job is still running past the window"
        );
        let late = client.start_call(13, &status, json!({}), None);
        let (at, reply) = client.response(&json!(13), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "odoo is up", "{reply}");
        assert!(
            since(late, at) < QUICK,
            "odoo_status answers within a second while odoo_job_wait runs past its window"
        );

        let (at, reply) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(
            text_of(&reply["result"]),
            "job finished after 3 s",
            "{reply}"
        );
        assert!(since(second, at) >= Duration::from_millis(2800));
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(
            text_of(&reply["result"]),
            "job finished after 6 s",
            "{reply}"
        );
        assert!(
            reply["result"]["isError"] != json!(true),
            "the job's real result, not a timeout: {reply}"
        );
        assert!(
            since(long, at) >= Duration::from_millis(JOB_S * 1000 - 200),
            "the call lasted the whole job: {:?}",
            since(long, at)
        );
        assert!(
            since(long, at) > Duration::from_millis(WINDOW_MS * 3),
            "several windows long"
        );
        assert!(
            cancelled_requests(&case).is_empty(),
            "nothing was cancelled"
        );
        let tokens: Vec<Value> = case
            .received_calls("odoo_job_wait")
            .iter()
            .map(|call| call["token"].clone())
            .collect();
        assert!(
            tokens
                .iter()
                .all(|token| token.as_str().is_some_and(|t| t.starts_with("tpx-"))),
            "the gateway gave each call a progress token of its own: {tokens:?}"
        );
        assert_ne!(tokens[0], tokens[1], "one token per call");
    }

    #[test]
    fn test_acceptance_02_the_same_through_lazy_discovery() {
        let _guard = case_lock();
        let case = Case::new("accept-lazy", Behaviour::default(), timeouts(60_000));
        let mut client = case.lazy_client();
        client.initialize();
        let (wait, status) = ("slow__odoo_job_wait", "slow__odoo_status");
        client.start_meta_call(1, status, json!({}), None);
        client.response(&json!(1), ANSWER_WITHIN);
        let long = client.start_meta_call(10, wait, job(4), None);
        std::thread::sleep(Duration::from_millis(WINDOW_MS + 300));
        let quick = client.start_meta_call(11, status, json!({}), None);
        let (at, reply) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "odoo is up", "{reply}");
        assert!(since(quick, at) < QUICK);
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(
            text_of(&reply["result"]),
            "job finished after 4 s",
            "{reply}"
        );
        assert!(since(long, at) >= Duration::from_millis(3800));
    }

    #[test]
    fn test_acceptance_03_a_listed_budget_carries_a_job_that_reports_no_progress() {
        let _guard = case_lock();
        let case = Case::new(
            "accept-budget",
            Behaviour {
                job_budget_s: 10,
                ..Default::default()
            },
            timeouts(60_000),
        );
        let (mut client, wait, status) = ready(&case);
        let silent = json!({ "timeout_s": TIMEOUT_S, "job_s": 5, "progress_every_ms": 0 });
        let long = client.start_call(10, &wait, silent, None);
        std::thread::sleep(Duration::from_millis(WINDOW_MS + 500));
        let quick = client.start_call(11, &status, json!({}), None);
        let (at, _) = client.response(&json!(11), ANSWER_WITHIN);
        assert!(since(quick, at) < QUICK);
        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(
            text_of(&reply["result"]),
            "job finished after 5 s",
            "{reply}"
        );
        assert!(since(long, at) >= Duration::from_millis(4800));
    }

    #[test]
    fn test_acceptance_04_a_job_past_the_cap_times_out_and_the_server_is_told_to_cancel() {
        let _guard = case_lock();
        let case = Case::new("accept-cap", Behaviour::default(), timeouts(3000));
        let (mut client, wait, status) = ready(&case);
        let long = client.start_call(10, &wait, json!({ "timeout_s": 600, "job_s": 60 }), None);
        std::thread::sleep(Duration::from_millis(500));
        let quick = client.start_call(11, &status, json!({}), None);
        let (at, _) = client.response(&json!(11), ANSWER_WITHIN);
        assert!(since(quick, at) < QUICK);

        let (at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        let error = error_text(&reply);
        assert!(
            error.contains("timed out waiting for 'tools/call' response")
                && error.contains("absolute cap"),
            "{reply}"
        );
        let waited = since(long, at);
        assert!(
            (Duration::from_millis(2800)..Duration::from_millis(6000)).contains(&waited),
            "cut at the cap, not at the window or the job: {waited:?}"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while cancelled_requests(&case).is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let cancelled = cancelled_requests(&case);
        assert_eq!(cancelled.len(), 1, "{:?}", case.entries());
        assert_eq!(
            cancelled[0]["requestId"],
            case.received_calls("odoo_job_wait")[0]["id"]
        );
    }

    #[test]
    fn test_acceptance_05_a_serial_server_queues_the_second_call() {
        let _guard = case_lock();
        let case = Case::new(
            "accept-serial",
            Behaviour::default(),
            Timeouts {
                serial: true,
                ..timeouts(60_000)
            },
        );
        let (mut client, wait, status) = ready(&case);
        let long = client.start_call(10, &wait, job(3), None);
        std::thread::sleep(Duration::from_millis(300));
        let queued = client.start_call(11, &status, json!({}), None);
        let (status_at, reply) = client.response(&json!(11), ANSWER_WITHIN);
        assert_eq!(text_of(&reply["result"]), "odoo is up", "{reply}");
        let (wait_at, reply) = client.response(&json!(10), ANSWER_WITHIN);
        assert_eq!(
            text_of(&reply["result"]),
            "job finished after 3 s",
            "{reply}"
        );
        assert!(
            since(queued, status_at) > Duration::from_millis(2000),
            "odoo_status waited for the job: {:?}",
            since(queued, status_at)
        );
        assert!(status_at >= wait_at, "it ran after the job was answered");
        assert!(since(long, wait_at) >= Duration::from_millis(2800));
    }
}
