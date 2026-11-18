//! The attention list: what needs the person right now, computed fresh on every call from the
//! stores each area keeps (login status, the vault, task runs, the library's sync state). Nothing
//! is stored here, so a row is gone as soon as its cause is; the one exception is the ledger the
//! Access check keeps in [`store`] to tell a newly added server from an old one.
//!
//! One feed per area: [`feed_credentials`] (missing secrets, logins, the vault), [`feed_updates`],
//! [`feed_access`] (servers no access set reaches), [`feed_tasks`] and [`feed_library`]. A feed is a pure function over plain inputs that
//! [`gather`] reads through the host; a feed that cannot be read is reported next to the rows
//! instead of showing nothing. What the person snoozed, skipped or switched off lives in [`store`].

mod access;
pub mod feed_access;
pub mod feed_credentials;
pub mod feed_library;
pub mod feed_repo_heads;
pub mod feed_tasks;
pub mod feed_updates;
mod gather;
mod model;
pub mod notify;
pub mod ops;
pub mod store;
pub mod test_toast;
pub mod updates;

pub use access::observe as observe_access;

use std::cmp::Reverse;
use std::collections::BTreeSet;

use toolport_plus_api::Host;

pub use model::{Action, Check, Hidden, Item, Screen, Snapshot, Urgency};

use feed_access::AccessInputs;
use feed_credentials::CredentialInputs;
use feed_library::LibraryInputs;
use feed_tasks::TaskInputs;
use feed_updates::UpdateInputs;
use store::Store;

pub struct Inputs {
    pub now: i64,
    pub store: Result<Store, String>,
    pub updates: Result<UpdateInputs, String>,
    pub access: Result<AccessInputs, String>,
    pub credentials: Result<CredentialInputs, String>,
    pub tasks: Result<TaskInputs, String>,
    pub library: Result<LibraryInputs, String>,
}

pub fn read(host: &dyn Host) -> Inputs {
    let store = gather::store(host);
    Inputs {
        now: host.now_unix(),
        updates: gather::updates(host, store.as_ref().ok()),
        access: gather::access(host, store.as_ref().ok()),
        store,
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
    let default = Store::default();
    let store = inputs.store.as_ref().unwrap_or(&default);
    let mut found = Vec::new();
    let mut checks = Vec::new();
    let mut check = |name: &'static str,
                     on: bool,
                     checked_at: Option<i64>,
                     rows: Result<Vec<Item>, &String>| {
        if !on {
            checks.push(Check {
                name,
                error: None,
                checked_at,
                off: true,
            });
            return;
        }
        match rows {
            Ok(rows) => {
                found.extend(rows);
                checks.push(Check {
                    name,
                    error: None,
                    checked_at,
                    off: false,
                });
            }
            Err(error) => checks.push(Check {
                name,
                error: Some(error.clone()),
                checked_at,
                off: false,
            }),
        }
    };
    let on = |name: &str| store.settings.check_on(name);
    check(
        "credentials",
        on("credentials"),
        None,
        inputs.credentials.as_ref().map(feed_credentials::items),
    );
    check(
        "updates",
        on("updates"),
        store.updates.checked_at,
        inputs.updates.as_ref().map(feed_updates::items),
    );
    check(
        "access",
        on("access"),
        None,
        inputs.access.as_ref().map(feed_access::items),
    );
    check(
        "tasks",
        on("tasks"),
        None,
        inputs
            .tasks
            .as_ref()
            .map(|tasks| feed_tasks::items(tasks, inputs.now)),
    );
    check(
        "library",
        on("library"),
        None,
        inputs.library.as_ref().map(feed_library::items),
    );
    if let Err(error) = &inputs.store {
        checks.push(Check {
            name: "hidden",
            error: Some(error.clone()),
            checked_at: None,
            off: false,
        });
    }
    let (mut items, mut hidden) = (Vec::new(), Vec::new());
    for item in order(found) {
        match store.hiding(&item, inputs.now) {
            Some(hide) => hidden.push(Hidden {
                hide: hide.clone(),
                item,
            }),
            None => items.push(item),
        }
    }
    Snapshot {
        checked_at: inputs.now,
        checks,
        items,
        hidden,
    }
}

pub fn snapshot(host: &dyn Host) -> Snapshot {
    compute(&read(host))
}

pub fn human_hidden(snapshot: &Snapshot) -> String {
    if snapshot.hidden.is_empty() {
        return "Nothing is hidden.\n".into();
    }
    let mut out = format!("Hidden ({})\n", snapshot.hidden.len());
    for hidden in &snapshot.hidden {
        let until = hidden
            .hide
            .until
            .map(|at| format!(" until {}", crate::tasks::cron::rfc3339(at)))
            .unwrap_or_default();
        out.push_str(&format!(
            "  {}  [{}] {}{until}\n    restore: toolportctl attention restore {}\n",
            hidden.item.title,
            hidden.item.source,
            hidden.hide.kind.word(),
            hidden.item.id
        ));
    }
    out
}

pub fn human_settings(settings: &serde_json::Value) -> String {
    let word = |on: &serde_json::Value| if on == true { "on" } else { "off" };
    let mut out = format!("Notifications: {}\n", word(&settings["notify"]["enabled"]));
    for (heading, rows) in [
        ("Checks", &settings["checks"]),
        ("Notify for", &settings["notify"]["categories"]),
    ] {
        out.push_str(&format!("{heading}:\n"));
        for row in rows.as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {}  {}\n",
                row["name"].as_str().unwrap_or(""),
                word(&row["on"])
            ));
        }
    }
    out
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

#[cfg(test)]
mod tests_access;

#[cfg(test)]
mod tests_updates;

#[cfg(test)]
mod tests_notify;
