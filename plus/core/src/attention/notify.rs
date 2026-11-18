//! The notifier pass: which needs-you rows are news, and what to say about them. It runs in the
//! app process, which owns the [`Notifier`]; the list it reads is the one the bell shows.

use serde_json::{json, Value};
use toolport_plus_api::{Host, Notifier};

use super::store::{Store, NOTIFY_WINDOW_SECS};
use super::{compute, read, Item, Urgency};
use crate::op::OpError;

pub const TEST_TITLE: &str = "Toolport+ notifications work";
const POINTER: &str = "Open Toolport+ to see them.";

pub struct Report {
    /// Why nothing was sent, when nothing was.
    pub reason: Option<&'static str>,
    /// Ids that were announced, or would have been had the window not had the focus.
    pub ids: Vec<String>,
    pub sent: usize,
    pub error: Option<String>,
}

impl Report {
    fn quiet(reason: &'static str) -> Self {
        Self {
            reason: Some(reason),
            ids: Vec::new(),
            sent: 0,
            error: None,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({"sent": self.sent, "ids": self.ids, "reason": self.reason, "error": self.error})
    }
}

fn category(item: &Item) -> &'static str {
    match item.source {
        "Credentials" => "credentials",
        "Tasks" => "tasks",
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
                if notifier.window_focused() {
                    Report {
                        ids,
                        ..Report::quiet("the window has the focus")
                    }
                } else {
                    let (title, body) = message(&news);
                    match notifier.notify(&title, &body) {
                        Ok(()) => Report {
                            reason: None,
                            ids,
                            sent: 1,
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

pub fn test(notifier: &dyn Notifier) -> Result<(), String> {
    notifier.notify(TEST_TITLE, "This is what a row that needs you looks like.")
}
