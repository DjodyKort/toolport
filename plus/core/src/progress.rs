//! The deadline of one stdio call that a server keeps alive by reporting progress.
//!
//! A `notifications/progress` for the call's own `progressToken` moves the deadline to the
//! progress time plus the call's window, never past an absolute cap. Time the gateway spends
//! waiting for a client to answer a server's request is charged to neither.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Cap on one progress-extended call of a server without `maxRequestTimeoutMs`.
pub const DEFAULT_CAP: Duration = Duration::from_secs(60 * 60);
const MAX_CAP_MS: u64 = 24 * 60 * 60 * 1000;
/// How often a call that carries a progress token looks at the ledger while it waits.
const POLL_SLICE: Duration = Duration::from_millis(100);

/// The cap for a server, given its `maxRequestTimeoutMs` if it has one.
pub fn cap_from(max_request_timeout_ms: Option<u64>) -> Duration {
    max_request_timeout_ms.map_or(DEFAULT_CAP, |ms| {
        Duration::from_millis(ms.clamp(1, MAX_CAP_MS))
    })
}

/// The ledger key of a progress token.
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
pub fn note(notification: &Value) {
    note_at(notification, Instant::now());
}

fn note_at(notification: &Value, at: Instant) {
    let Some(token) = notification
        .get("params")
        .and_then(|params| params.get("progressToken"))
    else {
        return;
    };
    let key = token_key(token);
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
    /// `window` is how long the call waits for its response, and for the next progress.
    pub fn start(window: Duration, token: Option<String>) -> Self {
        let started = Instant::now();
        Self {
            started,
            window,
            deadline: started + window,
            hard: None,
            rearmed: false,
            seen: None,
            watch: token.map(Watch::new),
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
        let mut clock = Clock::start(Duration::from_millis(500), None);
        let wait = clock.next_wait(|| unreachable!("no progress, no cap lookup"));
        assert!(wait > Duration::from_millis(400) && wait <= Duration::from_millis(500));
        assert!(!clock.expired());
        std::thread::sleep(Duration::from_millis(520));
        assert!(clock.expired());
        assert_eq!(clock.cap_note(), None);
    }

    #[test]
    fn test02_progress_for_the_calls_token_moves_the_deadline_to_progress_plus_window() {
        let mut clock = Clock::start(Duration::from_millis(400), Some(token_key(&json!("t-02"))));
        let first = clock.deadline;
        let at = after(&clock, Duration::from_millis(300));
        note_at(&progress("t-02"), at);
        clock.next_wait(|| Duration::from_secs(60));
        assert_eq!(clock.deadline, at + Duration::from_millis(400));
        assert!(clock.deadline > first);
        assert_eq!(clock.cap_note(), None);
    }

    #[test]
    fn test03_progress_under_another_token_does_not_extend_the_call() {
        let mut clock = Clock::start(Duration::from_millis(400), Some(token_key(&json!("t-03"))));
        let first = clock.deadline;
        note_at(
            &progress("t-03-other"),
            after(&clock, Duration::from_millis(300)),
        );
        clock.next_wait(|| unreachable!("another call's progress"));
        assert_eq!(clock.deadline, first);
    }

    #[test]
    fn test04_the_cap_ends_a_call_that_keeps_reporting_progress_and_says_so() {
        let mut clock = Clock::start(Duration::from_millis(400), Some(token_key(&json!("t-04"))));
        note_at(&progress("t-04"), after(&clock, Duration::from_millis(900)));
        clock.next_wait(|| Duration::from_millis(1000));
        assert_eq!(clock.deadline, clock.started + Duration::from_millis(1000));
        assert!(clock.cap_note().unwrap().contains("maxRequestTimeoutMs"));
    }

    #[test]
    fn test05_a_cap_below_the_window_never_shortens_the_call() {
        let mut clock = Clock::start(Duration::from_millis(400), Some(token_key(&json!("t-05"))));
        note_at(&progress("t-05"), after(&clock, Duration::from_millis(100)));
        clock.next_wait(|| Duration::from_millis(10));
        assert_eq!(clock.deadline, clock.started + Duration::from_millis(400));
        assert_eq!(clock.cap_note(), None);
    }

    #[test]
    fn test06_time_spent_waiting_for_the_client_is_charged_to_neither_deadline_nor_cap() {
        let mut clock = Clock::start(Duration::from_millis(400), Some(token_key(&json!("t-06"))));
        note_at(&progress("t-06"), after(&clock, Duration::from_millis(100)));
        clock.next_wait(|| Duration::from_secs(10));
        let (deadline, hard) = (clock.deadline, clock.hard.unwrap());
        clock.charge_back(Duration::from_secs(3));
        assert_eq!(clock.deadline, deadline + Duration::from_secs(3));
        assert_eq!(clock.hard.unwrap(), hard + Duration::from_secs(3));
    }

    #[test]
    fn test07_the_ledger_forgets_a_token_when_its_call_ends() {
        let clock = Clock::start(Duration::from_millis(400), Some(token_key(&json!("t-07"))));
        note_at(&progress("t-07"), Instant::now());
        assert!(latest("\"t-07\"").is_some());
        drop(clock);
        assert!(latest("\"t-07\"").is_none());
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
}
