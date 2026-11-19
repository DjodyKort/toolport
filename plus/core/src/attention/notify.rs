//! The notifier pass: which needs-you rows are news, and what to say about them. It runs in the
//! app process, which owns the [`Notifier`]; the list it reads is the one the bell shows.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use toolport_plus_api::{Host, Notifier, Permission, Toast, ToastRow};

use super::store::{Store, NOTIFY_WINDOW_SECS};
use super::{compute, notifier_state, read, Item, Urgency};
use crate::op::OpError;

pub const CHANNEL_SYSTEM: &str = "system";
pub const CHANNEL_TOAST: &str = "toast";
pub const CHANNEL_NONE: &str = "none";

pub const TEST_TITLE: &str = "Toolport+ notifications work";
const POINTER: &str = "Open Toolport+ to see them.";

pub struct Report {
    /// Why nothing was sent, when nothing was.
    pub reason: Option<&'static str>,
    /// Ids that were announced, or would have been had the window not had the focus.
    pub ids: Vec<String>,
    /// System notifications handed over.
    pub sent: usize,
    /// Which way the news went: the system, a toast in the window, or nowhere.
    pub channel: &'static str,
    pub error: Option<String>,
}

impl Report {
    fn quiet(reason: &'static str) -> Self {
        Self {
            reason: Some(reason),
            ids: Vec::new(),
            sent: 0,
            channel: CHANNEL_NONE,
            error: None,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "sent": self.sent,
            "channel": self.channel,
            "ids": self.ids,
            "reason": self.reason,
            "error": self.error,
        })
    }
}

fn category(item: &Item) -> &'static str {
    match item.source {
        "Credentials" => "credentials",
        "Tasks" => "tasks",
        "Access" => "access",
        "Updates" => "updates",
        _ => "library",
    }
}

/// The first sentence of a row's detail, unless it names a path.
fn sentence(detail: &str) -> Option<String> {
    let first = detail.split_inclusive(". ").next()?.trim();
    (!first.is_empty() && !first.contains('/')).then(|| first.to_string())
}

fn message(news: &[&Item]) -> (String, String) {
    match news {
        [one] => (
            one.title.clone(),
            match sentence(&one.detail) {
                Some(detail) => format!("{detail} Open Toolport+ to fix it."),
                None => "Open Toolport+ to see why.".into(),
            },
        ),
        many => {
            let named: Vec<&str> = many.iter().take(2).map(|i| i.title.as_str()).collect();
            let rest = many.len() - named.len();
            let list = match rest {
                0 => named.join(" and "),
                rest => format!("{} and {rest} more", named.join(", ")),
            };
            (
                format!("{} things need you", many.len()),
                format!("{list}. {POINTER}"),
            )
        }
    }
}

/// Sends one notification for the needs-you rows that appeared since the last pass, none while
/// the window has the focus, and never the same row twice inside the notification window. A
/// snoozed row counts as known, so it comes back from its snooze without a notification.
/// The first pass only records what is already open. Nothing is sent unless the master switch
/// and the row's category are on.
pub fn run(host: &dyn Host, notifier: &dyn Notifier) -> Result<Report, OpError> {
    let base = host
        .data_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "data directory could not be resolved"))?;
    let mut store = Store::read(&base).map_err(|e| OpError::failed("attention_store", e))?;
    let now = host.now_unix();
    let snapshot = compute(&read(host));
    let open: Vec<&Item> = snapshot
        .items
        .iter()
        .filter(|item| item.urgency == Urgency::NeedsYou)
        .collect();
    let hidden = snapshot
        .hidden
        .iter()
        .map(|h| &h.item)
        .filter(|item| item.urgency == Urgency::NeedsYou);
    let known: Vec<String> = open
        .iter()
        .copied()
        .chain(hidden)
        .map(|item| item.id.clone())
        .collect();
    let previous = store.announced.replace(known);
    let report = match previous {
        None => Report::quiet("first pass: what is open is in the bell"),
        Some(_) if !store.settings.notify.enabled => Report::quiet("notifications are off"),
        Some(previous) => {
            let news: Vec<&Item> = open
                .iter()
                .copied()
                .filter(|item| !previous.contains(&item.id))
                .filter(|item| store.settings.category_on(category(item)))
                .filter(|item| {
                    store
                        .notified
                        .get(&item.id)
                        .is_none_or(|at| now - at >= NOTIFY_WINDOW_SECS)
                })
                .collect();
            if news.is_empty() {
                Report::quiet("nothing new")
            } else {
                let ids: Vec<String> = news.iter().map(|i| i.id.clone()).collect();
                for id in &ids {
                    store.notified.insert(id.clone(), now);
                }
                let (title, body) = message(&news);
                if notifier.window_focused() {
                    let rows = news
                        .iter()
                        .map(|item| ToastRow {
                            id: item.id.clone(),
                            screen: screen_of(item),
                        })
                        .collect();
                    match notifier.toast(&Toast { title, body, rows }) {
                        Ok(()) => Report {
                            reason: None,
                            channel: CHANNEL_TOAST,
                            ids,
                            ..Report::quiet("")
                        },
                        Err(error) => Report {
                            error: Some(error),
                            ids,
                            ..Report::quiet("the window could not show the toast")
                        },
                    }
                } else if notifier.permission() == Permission::Blocked {
                    Report {
                        ids,
                        ..Report::quiet("the system blocks notifications from Toolport+")
                    }
                } else {
                    match notifier.notify(&title, &body) {
                        Ok(()) => Report {
                            reason: None,
                            ids,
                            sent: 1,
                            channel: CHANNEL_SYSTEM,
                            error: None,
                        },
                        Err(error) => Report {
                            error: Some(error),
                            ids,
                            ..Report::quiet("the system refused the notification")
                        },
                    }
                }
            }
        }
    };
    let live: Vec<&str> = snapshot
        .items
        .iter()
        .chain(snapshot.hidden.iter().map(|h| &h.item))
        .map(|i| i.id.as_str())
        .collect();
    store.tidy(&live, now);
    store
        .write(&base)
        .map_err(|e| OpError::failed("attention_store", e))?;
    Ok(report)
}

fn screen_of(item: &Item) -> String {
    item.action
        .as_ref()
        .and_then(|action| serde_json::to_value(action.screen).ok())
        .and_then(|screen| screen.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The answer of a test notification. `sent` says the system was handed it, `confirmed` that the
/// platform also reports the permission as granted: a platform that does not report it hands the
/// notification over and cannot say more.
pub struct TestOutcome {
    pub permission: Permission,
    pub sent: bool,
    pub error: Option<String>,
}

impl TestOutcome {
    pub fn confirmed(&self) -> bool {
        self.sent && self.permission == Permission::Allowed
    }

    pub fn code(&self) -> Option<&'static str> {
        match (&self.error, self.permission) {
            (None, _) => None,
            (Some(_), Permission::Blocked) => Some("notifications_blocked"),
            (Some(_), _) => Some("notify_failed"),
        }
    }
}

pub fn test(notifier: &dyn Notifier) -> TestOutcome {
    let permission = notifier.permission();
    let result = if permission == Permission::Blocked {
        Err(super::system_notifier::BLOCKED.to_string())
    } else {
        notifier.notify(TEST_TITLE, "This is what a row that needs you looks like.")
    };
    TestOutcome {
        permission,
        sent: result.is_ok(),
        error: result.err(),
    }
}

/// Leaves the permission the system reports where `attention settings` can read it.
pub fn record_permission(host: &dyn Host, notifier: &dyn Notifier) {
    if let Some(base) = host.data_dir() {
        notifier_state::record(&base, notifier.permission(), host.now_unix());
    }
}

/// How often the app looks for news, and how often for a test request from the command line.
const PASS_EVERY: Duration = Duration::from_secs(60);
const REQUEST_EVERY: Duration = Duration::from_secs(2);
const FIRST_PASS_AFTER: Duration = Duration::from_secs(20);

/// One scheduled pass. While the master switch is off nothing is computed and what was known is
/// forgotten, so switching it on starts with a silent first pass instead of announcing rows that
/// appeared in the meantime.
pub fn pass(host: &dyn Host, notifier: &dyn Notifier) -> Result<Option<Report>, OpError> {
    let _ = super::observe_access(host);
    record_permission(host, notifier);
    let base = host
        .data_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "data directory could not be resolved"))?;
    let mut store = Store::read(&base).map_err(|e| OpError::failed("attention_store", e))?;
    if store.settings.notify.enabled {
        return run(host, notifier).map(Some);
    }
    if store.announced.take().is_some() {
        store
            .write(&base)
            .map_err(|e| OpError::failed("attention_store", e))?;
    }
    Ok(None)
}

/// The app's notifier thread: answers test requests and runs a pass every minute.
pub fn spawn<H, N>(host: H, notifier: N)
where
    H: Host + Send + 'static,
    N: Notifier + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("attention-notifier".into())
        .spawn(move || {
            let mut next_pass = Instant::now() + FIRST_PASS_AFTER;
            record_permission(&host, &notifier);
            loop {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    super::test_toast::answer(&host, &notifier);
                    if Instant::now() >= next_pass {
                        next_pass = Instant::now() + PASS_EVERY;
                        let _ = pass(&host, &notifier);
                    }
                }));
                std::thread::sleep(REQUEST_EVERY);
            }
        });
}
