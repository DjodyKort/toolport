//! The attention list: what needs the person right now, computed fresh on every call from the
//! stores each area keeps (login status, the vault, task runs, the library's sync state). Nothing
//! is stored here, so a row is gone as soon as its cause is.
//!
//! One feed per area: [`feed_credentials`] (missing secrets, logins, the vault), [`feed_tasks`]
//! and [`feed_library`]. A feed is a pure function over plain inputs that [`gather`] reads through
//! the host; a feed that cannot be read is reported next to the rows instead of showing nothing.

pub mod feed_credentials;
pub mod feed_library;
pub mod feed_tasks;
mod gather;
mod model;

use std::cmp::Reverse;
use std::collections::BTreeSet;

use toolport_plus_api::Host;

pub use model::{Action, Check, Item, Screen, Snapshot, Urgency};

use feed_credentials::CredentialInputs;
use feed_library::LibraryInputs;
use feed_tasks::TaskInputs;

pub struct Inputs {
    pub now: i64,
    pub credentials: Result<CredentialInputs, String>,
    pub tasks: Result<TaskInputs, String>,
    pub library: Result<LibraryInputs, String>,
}

pub fn read(host: &dyn Host) -> Inputs {
    Inputs {
        now: host.now_unix(),
        credentials: gather::credentials(host),
        tasks: gather::tasks(host),
        library: gather::library(host),
    }
}

/// Needs-you rows before worth-a-look rows, newest first inside each, a row without a time
/// last, and one row per id (the first of a duplicate wins).
pub fn order(mut items: Vec<Item>) -> Vec<Item> {
    items.sort_by(|a, b| {
        (a.urgency, Reverse(a.since), &a.id).cmp(&(b.urgency, Reverse(b.since), &b.id))
    });
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.id.clone()));
    items
}

pub fn compute(inputs: &Inputs) -> Snapshot {
    let mut items = Vec::new();
    let mut checks = Vec::new();
    let mut check = |name: &'static str, found: Result<Vec<Item>, &String>| match found {
        Ok(found) => {
            items.extend(found);
            checks.push(Check { name, error: None });
        }
        Err(error) => checks.push(Check {
            name,
            error: Some(error.clone()),
        }),
    };
    check(
        "credentials",
        inputs.credentials.as_ref().map(feed_credentials::items),
    );
    check(
        "tasks",
        inputs
            .tasks
            .as_ref()
            .map(|tasks| feed_tasks::items(tasks, inputs.now)),
    );
    check("library", inputs.library.as_ref().map(feed_library::items));
    Snapshot {
        checked_at: inputs.now,
        checks,
        items: order(items),
    }
}

pub fn snapshot(host: &dyn Host) -> Snapshot {
    compute(&read(host))
}

pub fn human(snapshot: &Snapshot) -> String {
    let mut out = String::new();
    for (urgency, heading) in [
        (Urgency::NeedsYou, "Needs you"),
        (Urgency::WorthALook, "Worth a look"),
    ] {
        let rows: Vec<&Item> = snapshot
            .items
            .iter()
            .filter(|item| item.urgency == urgency)
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("{heading} ({})\n", rows.len()));
        for item in rows {
            out.push_str(&format!("  {}  [{}]\n", item.title, item.source));
            if !item.detail.is_empty() {
                out.push_str(&format!("    {}\n", item.detail));
            }
            if let Some(command) = item.action.as_ref().and_then(|a| a.command.as_ref()) {
                out.push_str(&format!("    fix: {}\n", command.join(" ")));
            }
        }
    }
    let unchecked: Vec<String> = snapshot
        .checks
        .iter()
        .filter_map(|c| c.error.as_ref().map(|e| format!("{} ({e})", c.name)))
        .collect();
    if snapshot.items.is_empty() && unchecked.is_empty() {
        out.push_str("Nothing needs you.\n");
    }
    if !unchecked.is_empty() {
        out.push_str(&format!("Not checked: {}\n", unchecked.join("; ")));
    }
    out
}

#[cfg(test)]
mod tests;
