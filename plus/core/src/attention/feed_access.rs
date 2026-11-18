use super::model::{Action, Item, Screen, Urgency};
use crate::reach::{self, AccessRow};

const SOURCE: &str = "Access";

pub struct UnreachedServer {
    pub id: String,
    pub name: String,
    /// When the server first appeared, set only when that was after the access sets last changed.
    pub added: Option<i64>,
}

pub struct AccessInputs {
    /// Servers that are on and that no access set reaches.
    pub unreached: Vec<UnreachedServer>,
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
    out.extend(
        inputs
            .folders
            .iter()
            .cloned()
            .map(|folder| row(folder, Urgency::WorthALook, None, "this folder rule")),
    );
    out
}
