//! The deadline of one stdio call that a server keeps alive by reporting progress.
//!
//! A `notifications/progress` for the call's own `progressToken` moves the deadline to the
//! progress time plus the call's window, never past an absolute cap. Only the server the call
//! was sent to can extend it: a token that another server's connection reports means nothing.
//! Time the gateway spends waiting for a client to answer a server's request is charged to
//! neither.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Cap on one progress-extended call of a server without `maxRequestTimeoutMs`.
pub const DEFAULT_CAP: Duration = Duration::from_secs(60 * 60);
/// Longest `maxRequestTimeoutMs` a surface accepts; `cap_from` clamps a stored value to it.
pub const MAX_CAP_MS: u64 = 24 * 60 * 60 * 1000;
/// How often a call that carries a progress token looks at the ledger while it waits.
const POLL_SLICE: Duration = Duration::from_millis(100);

/// The cap for a server, given its `maxRequestTimeoutMs` if it has one.
pub fn cap_from(max_request_timeout_ms: Option<u64>) -> Duration {
    max_request_timeout_ms.map_or(DEFAULT_CAP, |ms| {
        Duration::from_millis(ms.clamp(1, MAX_CAP_MS))
    })
}

/// Names the connection to one server process; progress counts only on the connection the call
/// was sent on.
pub type Producer = usize;

fn ledger_key(producer: Producer, token: &str) -> String {
    format!("{producer}/{token}")
}

/// The key of a progress token in a lease or a call's window.
pub fn token_key(token: &Value) -> String {
    token.to_string()
}

/// The token a request asks its server to report progress under.
pub fn request_token(params: &Value) -> Option<String> {
    params
        .get("_meta")?
        .get("progressToken")
        .filter(|token| !token.is_null())
        .map(token_key)
}

/// The key under which a tool's listing states how long a call is expected to take, in seconds.
pub const BUDGET_META_KEY: &str = "odh.dev/budget";
/// Prefix of the progress tokens the gateway mints for calls whose client sent none.
const MINTED_PREFIX: &str = "tpx-";
static MINTED: AtomicU64 = AtomicU64::new(0);
static WINDOWS: Mutex<Option<HashMap<String, Duration>>> = Mutex::new(None);

/// What one server's settings say about the calls to it.
pub struct CallSettings<'a> {
    pub stdio: bool,
    pub max_request_timeout_ms: Option<u64>,
    /// The server's `toolBudgets`: seconds per original tool name.
    pub tool_budgets: Option<&'a Value>,
}

/// A call as the gateway sends it: the `_meta` to relay in place of the client's, and the
/// window the call waits for its response and for each progress.
pub struct PreparedCall {
    pub meta: Option<Value>,
    pub lease: Option<WindowLease>,
}

/// Keeps a call's window known to its clock; dropping it forgets the call's token.
pub struct WindowLease {
    key: String,
}

impl Drop for WindowLease {
    fn drop(&mut self) {
        let mut guard = WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(windows) = guard.as_mut() {
            windows.remove(&self.key);
        }
    }
}

/// The window the call that sends `params` was prepared with, if it was given one.
pub fn leased_window(token: &str) -> Option<Duration> {
    let guard = WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
    guard.as_ref()?.get(token).copied()
}

fn seconds(value: &Value) -> Option<Duration> {
    let secs = value
        .as_f64()
        .filter(|secs| secs.is_finite() && *secs > 0.0)?;
    Some(Duration::from_secs_f64(
        secs.min(MAX_CAP_MS as f64 / 1000.0),
    ))
}

/// The window a tool's call is given instead of the server's `requestTimeoutMs`: its
/// `toolBudgets` entry, else the budget in its listing, never beyond the cap.
fn budget_window(
    settings: &CallSettings<'_>,
    tool: &str,
    listed: Option<&Value>,
) -> Option<Duration> {
    let configured = settings
        .tool_budgets
        .and_then(|budgets| budgets.get(tool))
        .and_then(seconds);
    let listed = || {
        listed
            .and_then(|definition| definition.get("_meta")?.get(BUDGET_META_KEY))
            .and_then(seconds)
    };
    let window = configured.or_else(listed)?;
    Some(window.min(cap_from(settings.max_request_timeout_ms)))
}

/// Gets a `tools/call` ready: a stdio call without a progress token is given one the gateway
/// minted, so the server's progress can keep it alive, and a tool with a budget is given that
/// window. `meta` is the `_meta` the call would otherwise carry; `meta` of the result is `None`
/// when it stays as it is.
pub fn prepare_call(
    settings: &CallSettings<'_>,
    tool: &str,
    listed: Option<&Value>,
    meta: Option<&Value>,
) -> PreparedCall {
    if !settings.stdio {
        return PreparedCall {
            meta: None,
            lease: None,
        };
    }
    let carried = meta
        .and_then(|meta| meta.get("progressToken"))
        .filter(|token| !token.is_null());
    let (token, relayed) = match carried {
        Some(token) => (token.clone(), None),
        None => {
            let token = Value::String(format!(
                "{MINTED_PREFIX}{}",
                MINTED.fetch_add(1, Ordering::Relaxed) + 1
            ));
            let mut relayed = meta
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()));
            if let Some(object) = relayed.as_object_mut() {
                object.insert("progressToken".to_string(), token.clone());
            }
            (token, Some(relayed))
        }
    };
    let lease = budget_window(settings, tool, listed).map(|window| {
        let key = token_key(&token);
        let mut guard = WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
        guard
            .get_or_insert_with(HashMap::new)
            .insert(key.clone(), window);
        WindowLease { key }
    });
    PreparedCall {
        meta: relayed,
        lease,
    }
}

/// Default `requestTimeoutMs` of a server without one.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(30);
/// Slack between a server's deadline and the adapter's, so the server's own timeout text arrives.
pub const ADAPTER_GRACE: Duration = Duration::from_secs(60);
/// A caller-set timeout below this is a deliberate short one (tests, probes) and is kept.
pub const EXTENDABLE_FROM: Duration = Duration::from_secs(600);
const ADAPTER_METHODS: [&str; 3] = ["tools/call", "resources/read", "prompts/get"];

/// What one server allows a call: its `requestTimeoutMs`, and for a stdio server also the cap that
/// progress can stretch it to. Only a stdio server's deadline is re-armed by progress.
pub struct ServerTimeouts {
    pub request_timeout_ms: Option<u64>,
    pub max_request_timeout_ms: Option<u64>,
    pub stdio: bool,
}

impl ServerTimeouts {
    pub fn ceiling(&self) -> Duration {
        let window = self
            .request_timeout_ms
            .map_or(DEFAULT_WINDOW, |ms| Duration::from_millis(ms.max(1)));
        if self.stdio {
            cap_from(self.max_request_timeout_ms).max(window)
        } else {
            window
        }
    }
}

/// The longest the stdio adapter lets a request stay open. A request that reaches a server
/// (`tools/call`, `resources/read`, `prompts/get`) gets the longest ceiling any configured server
/// can hold plus a minute, never less than `base`; every other request keeps `base`.
pub fn adapter_ceiling(
    base: Duration,
    method: &str,
    servers: impl IntoIterator<Item = ServerTimeouts>,
) -> Duration {
    if base < EXTENDABLE_FROM || !ADAPTER_METHODS.contains(&method) {
        return base;
    }
    let longest = servers.into_iter().map(|s| s.ceiling()).max();
    longest.map_or(base, |longest| base.max(longest + ADAPTER_GRACE))
}

static LEDGER: Mutex<Option<HashMap<String, Watched>>> = Mutex::new(None);

struct Watched {
    calls: usize,
    last: Option<Instant>,
}

fn with_ledger<R>(use_ledger: impl FnOnce(&mut HashMap<String, Watched>) -> R) -> R {
    let mut guard = LEDGER.lock().unwrap_or_else(PoisonError::into_inner);
    use_ledger(guard.get_or_insert_with(HashMap::new))
}

/// A server reported progress under `notification`'s token. Only tokens a running call watches
/// are kept; any other progress costs one lookup.
pub fn note(notification: &Value, producer: Producer) {
    note_at(notification, Instant::now(), producer);
}

fn note_at(notification: &Value, at: Instant, producer: Producer) {
    let Some(token) = notification
        .get("params")
        .and_then(|params| params.get("progressToken"))
    else {
        return;
    };
    let key = ledger_key(producer, &token_key(token));
    with_ledger(|ledger| {
        if let Some(watched) = ledger.get_mut(&key) {
            watched.last = Some(at);
        }
    });
}

fn latest(key: &str) -> Option<Instant> {
    with_ledger(|ledger| ledger.get(key).and_then(|watched| watched.last))
}

struct Watch {
    key: String,
}

impl Watch {
    fn new(key: String) -> Self {
        with_ledger(|ledger| {
            ledger
                .entry(key.clone())
                .or_insert(Watched {
                    calls: 0,
                    last: None,
                })
                .calls += 1;
        });
        Self { key }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        with_ledger(|ledger| {
            if let Some(watched) = ledger.get_mut(&self.key) {
                watched.calls -= 1;
                if watched.calls == 0 {
                    ledger.remove(&self.key);
                }
            }
        });
    }
}

pub struct Clock {
    started: Instant,
    window: Duration,
    deadline: Instant,
    hard: Option<Instant>,
    rearmed: bool,
    seen: Option<Instant>,
    watch: Option<Watch>,
}

impl Clock {
    /// `window` is how long the call waits for its response, and for the next progress; `producer`
    /// is the connection the call is sent on.
    pub fn start(window: Duration, token: Option<String>, producer: Producer) -> Self {
        let started = Instant::now();
        Self {
            started,
            window,
            deadline: started + window,
            hard: None,
            rearmed: false,
            seen: None,
            watch: token.map(|token| Watch::new(ledger_key(producer, &token))),
        }
    }

    /// How long to wait for the next message: the time left, in short slices while the call has a
    /// token so progress is noticed. `cap` is asked once, on the call's first progress.
    pub fn next_wait(&mut self, cap: impl FnOnce() -> Duration) -> Duration {
        if let Some(watch) = &self.watch {
            if let Some(at) = latest(&watch.key).filter(|at| Some(*at) > self.seen) {
                self.seen = Some(at);
                self.rearm(at, cap);
            }
        }
        let left = self.deadline.saturating_duration_since(Instant::now());
        if self.watch.is_some() {
            left.min(POLL_SLICE)
        } else {
            left
        }
    }

    fn rearm(&mut self, at: Instant, cap: impl FnOnce() -> Duration) {
        let started = self.started;
        let window = self.window;
        let hard = *self.hard.get_or_insert_with(|| started + cap().max(window));
        let extended = (at + window).min(hard);
        if extended > self.deadline {
            self.deadline = extended;
            self.rearmed = true;
        }
    }

    /// Time spent on something the server is not accountable for is given back to the call.
    pub fn charge_back(&mut self, waited: Duration) {
        self.deadline += waited;
        self.hard = self.hard.map(|hard| hard + waited);
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    /// Said when the call ended at its cap while the server still reported progress.
    pub fn cap_note(&self) -> Option<&'static str> {
        (self.rearmed && self.hard == Some(self.deadline)).then_some(
            ": the call reached its absolute cap (maxRequestTimeoutMs) while the server was still reporting progress",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn progress(token: &str) -> Value {
        json!({ "method": "notifications/progress", "params": { "progressToken": token, "progress": 1 } })
    }

    fn after(clock: &Clock, offset: Duration) -> Instant {
        clock.started + offset
    }

    #[test]
    fn test01_a_call_without_a_token_keeps_its_fixed_deadline() {
        let mut clock = Clock::start(Duration::from_millis(500), None, 1);
        let wait = clock.next_wait(|| unreachable!("no progress, no cap lookup"));
        assert!(wait > Duration::from_millis(400) && wait <= Duration::from_millis(500));
        assert!(!clock.expired());
        std::thread::sleep(Duration::from_millis(520));
        assert!(clock.expired());
        assert_eq!(clock.cap_note(), None);
    }

    #[test]
    fn test02_progress_for_the_calls_token_moves_the_deadline_to_progress_plus_window() {
        let mut clock = Clock::start(
            Duration::from_millis(400),
            Some(token_key(&json!("t-02"))),
            1,
        );
        let first = clock.deadline;
        let at = after(&clock, Duration::from_millis(300));
        note_at(&progress("t-02"), at, 1);
        clock.next_wait(|| Duration::from_secs(60));
        assert_eq!(clock.deadline, at + Duration::from_millis(400));
        assert!(clock.deadline > first);
        assert_eq!(clock.cap_note(), None);
    }

    #[test]
    fn test03_progress_under_another_token_does_not_extend_the_call() {
        let mut clock = Clock::start(
            Duration::from_millis(400),
            Some(token_key(&json!("t-03"))),
            1,
        );
        let first = clock.deadline;
        note_at(
            &progress("t-03-other"),
            after(&clock, Duration::from_millis(300)),
            1,
        );
        clock.next_wait(|| unreachable!("another call's progress"));
        assert_eq!(clock.deadline, first);
    }

    #[test]
    fn test18_progress_for_the_calls_token_from_another_connection_does_not_extend_it() {
        let mut clock = Clock::start(
            Duration::from_millis(400),
            Some(token_key(&json!("t-18"))),
            1,
        );
        let first = clock.deadline;
        note_at(
            &progress("t-18"),
            after(&clock, Duration::from_millis(300)),
            2,
        );
        clock.next_wait(|| unreachable!("another connection's progress"));
        assert_eq!(clock.deadline, first);
        note_at(
            &progress("t-18"),
            after(&clock, Duration::from_millis(300)),
            1,
        );
        clock.next_wait(|| Duration::from_secs(60));
        assert!(
            clock.deadline > first,
            "the connection the call is on still extends it"
        );
    }

    #[test]
    fn test04_the_cap_ends_a_call_that_keeps_reporting_progress_and_says_so() {
        let mut clock = Clock::start(
            Duration::from_millis(400),
            Some(token_key(&json!("t-04"))),
            1,
        );
        note_at(
            &progress("t-04"),
            after(&clock, Duration::from_millis(900)),
            1,
        );
        clock.next_wait(|| Duration::from_millis(1000));
        assert_eq!(clock.deadline, clock.started + Duration::from_millis(1000));
        assert!(clock.cap_note().unwrap().contains("maxRequestTimeoutMs"));
    }

    #[test]
    fn test05_a_cap_below_the_window_never_shortens_the_call() {
        let mut clock = Clock::start(
            Duration::from_millis(400),
            Some(token_key(&json!("t-05"))),
            1,
        );
        note_at(
            &progress("t-05"),
            after(&clock, Duration::from_millis(100)),
            1,
        );
        clock.next_wait(|| Duration::from_millis(10));
        assert_eq!(clock.deadline, clock.started + Duration::from_millis(400));
        assert_eq!(clock.cap_note(), None);
    }

    #[test]
    fn test06_time_spent_waiting_for_the_client_is_charged_to_neither_deadline_nor_cap() {
        let mut clock = Clock::start(
            Duration::from_millis(400),
            Some(token_key(&json!("t-06"))),
            1,
        );
        note_at(
            &progress("t-06"),
            after(&clock, Duration::from_millis(100)),
            1,
        );
        clock.next_wait(|| Duration::from_secs(10));
        let (deadline, hard) = (clock.deadline, clock.hard.unwrap());
        clock.charge_back(Duration::from_secs(3));
        assert_eq!(clock.deadline, deadline + Duration::from_secs(3));
        assert_eq!(clock.hard.unwrap(), hard + Duration::from_secs(3));
    }

    #[test]
    fn test07_the_ledger_forgets_a_token_when_its_call_ends() {
        let clock = Clock::start(
            Duration::from_millis(400),
            Some(token_key(&json!("t-07"))),
            1,
        );
        note_at(&progress("t-07"), Instant::now(), 1);
        assert!(latest(&ledger_key(1, "\"t-07\"")).is_some());
        drop(clock);
        assert!(latest(&ledger_key(1, "\"t-07\"")).is_none());
    }

    #[test]
    fn test08_the_cap_is_the_registry_value_clamped_with_one_hour_by_default() {
        assert_eq!(cap_from(None), DEFAULT_CAP);
        assert_eq!(cap_from(Some(90_000)), Duration::from_secs(90));
        assert_eq!(cap_from(Some(0)), Duration::from_millis(1));
        assert_eq!(cap_from(Some(u64::MAX)), Duration::from_secs(86_400));
    }

    #[test]
    fn test09_the_request_token_is_the_one_in_meta() {
        assert_eq!(
            request_token(&json!({ "_meta": { "progressToken": "tp-9" } })),
            Some("\"tp-9\"".into())
        );
        assert_eq!(
            request_token(&json!({ "_meta": { "progressToken": null } })),
            None
        );
        assert_eq!(request_token(&json!({})), None);
    }

    fn stdio(window: Option<u64>, cap: Option<u64>) -> ServerTimeouts {
        ServerTimeouts {
            request_timeout_ms: window,
            max_request_timeout_ms: cap,
            stdio: true,
        }
    }

    const TEN_MINUTES: Duration = Duration::from_secs(600);

    #[test]
    fn test10_a_progress_extended_call_outlives_ten_minutes_without_waiting_for_them() {
        let mut clock = Clock::start(Duration::from_secs(30), Some(token_key(&json!("t-10"))), 1);
        for minute in 1..=12u64 {
            let at = after(&clock, Duration::from_secs(minute * 60 - 5));
            note_at(&progress("t-10"), at, 1);
            clock.next_wait(|| Duration::from_secs(3600));
            assert_eq!(
                clock.deadline,
                at + Duration::from_secs(30),
                "minute {minute}"
            );
        }
        assert!(clock.deadline > clock.started + TEN_MINUTES);
    }

    #[test]
    fn test11_the_adapter_ceiling_follows_the_longest_stdio_cap_plus_a_minute() {
        let ceiling = |servers| adapter_ceiling(TEN_MINUTES, "tools/call", servers);
        assert_eq!(
            ceiling(vec![stdio(None, None)]),
            Duration::from_secs(3660),
            "the default cap is one hour"
        );
        assert_eq!(
            ceiling(vec![
                stdio(None, Some(5_400_000)),
                stdio(None, Some(120_000))
            ]),
            Duration::from_secs(5460)
        );
        assert_eq!(
            ceiling(vec![stdio(Some(7_200_000), None)]),
            Duration::from_secs(7260)
        );
        assert_eq!(
            ceiling(vec![stdio(None, Some(120_000))]),
            TEN_MINUTES,
            "never below the base"
        );
        assert_eq!(ceiling(vec![]), TEN_MINUTES);
    }

    #[test]
    fn test12_a_remote_servers_deadline_is_not_stretched_by_a_cap() {
        let remote = ServerTimeouts {
            request_timeout_ms: Some(900_000),
            max_request_timeout_ms: Some(10_800_000),
            stdio: false,
        };
        assert_eq!(
            adapter_ceiling(TEN_MINUTES, "tools/call", vec![remote]),
            Duration::from_secs(960)
        );
    }

    #[test]
    fn test13_only_requests_that_reach_a_server_and_default_base_timeouts_are_extended() {
        let servers = || vec![stdio(None, None)];
        for method in ["tools/call", "resources/read", "prompts/get"] {
            assert!(
                adapter_ceiling(TEN_MINUTES, method, servers()) > TEN_MINUTES,
                "{method}"
            );
        }
        for method in ["tools/list", "initialize", "ping"] {
            assert_eq!(
                adapter_ceiling(TEN_MINUTES, method, servers()),
                TEN_MINUTES,
                "{method}"
            );
        }
        let short = Duration::from_millis(300);
        assert_eq!(adapter_ceiling(short, "tools/call", servers()), short);
    }

    fn settings<'a>(budgets: Option<&'a Value>, cap_ms: Option<u64>) -> CallSettings<'a> {
        CallSettings {
            stdio: true,
            max_request_timeout_ms: cap_ms,
            tool_budgets: budgets,
        }
    }

    fn listing(budget: Value) -> Value {
        json!({ "name": "wait", "_meta": { BUDGET_META_KEY: budget } })
    }

    #[test]
    fn test14_a_stdio_call_without_a_token_is_given_a_minted_one_and_a_client_token_is_kept() {
        let none = settings(None, None);
        let minted = prepare_call(&none, "wait", None, None);
        let token = minted.meta.as_ref().unwrap()["progressToken"].clone();
        assert!(token.as_str().unwrap().starts_with("tpx-"));
        let with_other_keys = json!({ "traceparent": "00-x" });
        let relayed = prepare_call(&none, "wait", None, Some(&with_other_keys))
            .meta
            .unwrap();
        assert_eq!(relayed["traceparent"], "00-x");
        assert_ne!(
            relayed["progressToken"], token,
            "every call has its own token"
        );
        let carried = json!({ "progressToken": "tp-3" });
        assert!(prepare_call(&none, "wait", None, Some(&carried))
            .meta
            .is_none());
        let null_token = json!({ "progressToken": null });
        assert!(prepare_call(&none, "wait", None, Some(&null_token))
            .meta
            .is_some());
    }

    #[test]
    fn test15_a_remote_servers_call_is_left_alone() {
        let remote = CallSettings {
            stdio: false,
            ..settings(None, None)
        };
        let prepared = prepare_call(&remote, "wait", Some(&listing(json!(30))), None);
        assert!(prepared.meta.is_none() && prepared.lease.is_none());
    }

    #[test]
    fn test16_the_window_is_the_tool_budget_then_the_listing_budget_never_beyond_the_cap() {
        let budgets = json!({ "wait": 90, "bad": -1, "text": "soon" });
        let window = |settings: &CallSettings<'_>, tool: &str, listed: Option<&Value>| {
            budget_window(settings, tool, listed)
        };
        let listed = listing(json!(120));
        let own = settings(Some(&budgets), None);
        assert_eq!(
            window(&own, "wait", Some(&listed)),
            Some(Duration::from_secs(90))
        );
        assert_eq!(
            window(&settings(None, None), "wait", Some(&listed)),
            Some(Duration::from_secs(120))
        );
        assert_eq!(window(&own, "other", None), None);
        for unusable in [json!(0), json!(-5), json!("120"), json!(null)] {
            assert_eq!(
                window(&settings(None, None), "wait", Some(&listing(unusable))),
                None
            );
        }
        assert_eq!(window(&own, "bad", None), None);
        assert_eq!(window(&own, "text", None), None);
        let capped = settings(Some(&budgets), Some(60_000));
        assert_eq!(
            window(&capped, "wait", Some(&listed)),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            window(&settings(None, Some(60_000)), "wait", Some(&listed)),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn test17_a_calls_window_is_known_to_its_clock_until_the_lease_is_dropped() {
        let budgets = json!({ "wait": 90 });
        let prepared = prepare_call(&settings(Some(&budgets), None), "wait", None, None);
        let token = request_token(&json!({ "_meta": prepared.meta.clone().unwrap() })).unwrap();
        assert_eq!(leased_window(&token), Some(Duration::from_secs(90)));
        drop(prepared);
        assert_eq!(leased_window(&token), None);
    }
}
