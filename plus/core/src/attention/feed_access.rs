use super::model::{Action, Item, Screen, Urgency};
use crate::reach::{self, AccessRow};

const SOURCE: &str = "Access";

pub struct UnreachedServer {
    pub id: String,
    pub name: String,
    /// When the server first appeared, set only when that was after the access sets last changed.
    pub added: Option<i64>,
}

/// A server that is on and reached by an access set but has had no successful call for a while.
pub struct StaleServer {
    pub id: String,
    pub name: String,
    pub idle_since: i64,
}

pub struct AccessInputs {
    /// Servers that are on and that no access set reaches.
    pub unreached: Vec<UnreachedServer>,
    /// Servers in use on paper that no call has succeeded on for the configured number of days.
    pub stale: Vec<StaleServer>,
    pub stale_days: u32,
    /// One row per mapped folder that outranks the own set of a client.
    pub folders: Vec<AccessRow>,
}

fn row(row: AccessRow, urgency: Urgency, since: Option<i64>, skip: &str) -> Item {
    Item {
        action: Some(Action::open(row.action_label, Screen::Servers)),
        id: row.id,
        urgency,
        title: row.title,
        detail: row.detail,
        source: SOURCE,
        since,
        skip: (urgency == Urgency::WorthALook).then(|| skip.to_string()),
    }
}

fn named(servers: &[&UnreachedServer]) -> Vec<(String, String)> {
    let mut named: Vec<(String, String)> = servers
        .iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect();
    named.sort();
    named
}

fn unreached_row(servers: &[&UnreachedServer], added: bool) -> AccessRow {
    let named = named(servers);
    let refs: Vec<(&str, &str)> = named
        .iter()
        .map(|(id, name)| (id.as_str(), name.as_str()))
        .collect();
    reach::unreached_row(&refs, added)
}

fn names(servers: &[&StaleServer]) -> String {
    const SHOWN: usize = 5;
    let mut names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    let more = names.len().saturating_sub(SHOWN);
    names.truncate(SHOWN);
    match more {
        0 => names.join(", "),
        more => format!("{} and {more} more", names.join(", ")),
    }
}

fn stale_row(servers: &[&StaleServer], days: u32) -> Item {
    let mut ids: Vec<&str> = servers.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    let (title, verb, them) = match servers {
        [one] => (
            format!("{} has not worked for {days} days", one.name),
            "is",
            "it",
        ),
        _ => (
            format!("{} servers have not worked for {days} days", servers.len()),
            "are",
            "them",
        ),
    };
    Item {
        id: format!("access:stale:{}", ids.join(",")),
        urgency: Urgency::WorthALook,
        title,
        detail: format!(
            "{} {verb} on and an access set reaches {them}, but no call succeeded in that time. \
             Switch {them} off or take {them} out of the sets if nobody uses {them}; the row goes \
             with the next successful call.",
            names(servers),
        ),
        source: SOURCE,
        since: None,
        action: Some(Action::open("Review", Screen::Servers)),
        skip: Some("these servers".into()),
    }
}

/// A server that appeared after the sets last changed is the failure this check exists for: the
/// agent never sees its tools and nothing says so, so it needs the person. Servers that have
/// always been unreached, and folders whose own set wins, are deliberate more often than not and
/// stay calm; both ids carry what they are about, so a skip ends when that changes.
pub fn items(inputs: &AccessInputs) -> Vec<Item> {
    let (added, others): (Vec<&UnreachedServer>, Vec<&UnreachedServer>) = inputs
        .unreached
        .iter()
        .partition(|server| server.added.is_some());
    let mut out = Vec::new();
    if !added.is_empty() {
        let since = added.iter().filter_map(|s| s.added).min();
        out.push(row(
            unreached_row(&added, true),
            Urgency::NeedsYou,
            since,
            "",
        ));
    }
    if !others.is_empty() {
        out.push(row(
            unreached_row(&others, false),
            Urgency::WorthALook,
            None,
            "these servers",
        ));
    }
    if !inputs.stale.is_empty() {
        let stale: Vec<&StaleServer> = inputs.stale.iter().collect();
        out.push(stale_row(&stale, inputs.stale_days));
    }
    out.extend(
        inputs
            .folders
            .iter()
            .cloned()
            .map(|folder| row(folder, Urgency::WorthALook, None, "this folder rule")),
    );
    out
}
