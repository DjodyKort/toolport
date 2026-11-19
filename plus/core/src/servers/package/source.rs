//! What each server's code comes from and whether a newer one is known: one answer per server
//! for the Source row, read from the registry, the last update check and git as they are on
//! disk. Nothing here asks the network or a gateway.

use serde_json::{json, Value};
use toolport_plus_api::{Host, ServerRecord};

use super::{automatic, changed, pinned, shown_version};
use crate::attention::store::{SourceCheck, Store};
use crate::op::OpError;
use crate::servers::repo::{self, state::Verdict};
use crate::servers::{find, read_servers};
use crate::tasks::cron::rfc3339;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    UpToDate,
    UpdateAvailable,
    /// A pin that has not been looked up yet.
    Unchecked,
    /// The last lookup failed.
    Error,
    /// The runner fetches the newest version on every start.
    Automatic,
    /// A checkout that cannot be updated as it stands (local changes, diverged, detached).
    Blocked,
    /// Nothing says where this server's code comes from, so there is nothing to check.
    Unknown,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::UpToDate => "up_to_date",
            State::UpdateAvailable => "update_available",
            State::Unchecked => "unchecked",
            State::Error => "error",
            State::Automatic => "automatic",
            State::Blocked => "blocked",
            State::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Row {
    pub server: String,
    pub id: String,
    /// `npm`, `pypi`, `docker`, `github`, `git` or `none`.
    pub kind: &'static str,
    pub state: State,
    pub package: Option<String>,
    /// The tag a digest pin follows.
    pub tracks: Option<String>,
    pub pinned: Option<String>,
    pub latest: Option<String>,
    pub checked_at: Option<i64>,
    pub error: Option<String>,
    /// Why Toolport cannot update it itself; the update is made by hand.
    pub manual: Option<String>,
    /// For a checkout: its branch, what it tracks and how many commits wait.
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub behind: u64,
    pub root: Option<String>,
}

impl Row {
    fn new(server: &ServerRecord, kind: &'static str, state: State) -> Self {
        Self {
            server: server.name.clone(),
            id: server.id.clone(),
            kind,
            state,
            package: None,
            tracks: None,
            pinned: None,
            latest: None,
            checked_at: None,
            error: None,
            manual: None,
            branch: None,
            upstream: None,
            behind: 0,
            root: None,
        }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "server": self.server,
            "id": self.id,
            "kind": self.kind,
            "state": self.state.word(),
            "package": self.package,
            "tracks": self.tracks,
            "pinned": self.pinned,
            "latest": self.latest,
            "checkedAt": self.checked_at.map(rfc3339),
            "error": self.error,
            "manual": self.manual,
            "branch": self.branch,
            "upstream": self.upstream,
            "behind": self.behind,
            "root": self.root,
        })
    }

    pub fn human(&self) -> String {
        let shown = |v: &Option<String>| v.as_deref().map(shown_version).unwrap_or_default();
        let what = match (self.kind, self.state) {
            (_, State::Unknown) => "no update source known".to_string(),
            (_, State::Automatic) => {
                format!("{}, the newest version on each start", self.kind)
            }
            ("git", _) => format!(
                "git {} -> {}, {} commit(s) behind",
                self.branch.as_deref().unwrap_or("?"),
                self.upstream.as_deref().unwrap_or("?"),
                self.behind
            ),
            _ => format!(
                "{} {} {}",
                self.kind,
                self.package.as_deref().unwrap_or(""),
                shown(&self.pinned)
            ),
        };
        let note = match self.state {
            State::UpdateAvailable if self.kind != "git" => {
                format!(", {} is out", shown(&self.latest))
            }
            State::Error => format!(
                ", last check failed: {}",
                self.error.as_deref().unwrap_or("")
            ),
            State::Unchecked => ", not checked yet".to_string(),
            _ => String::new(),
        };
        let by_hand = self
            .manual
            .as_ref()
            .map(|reason| format!(" (by hand: {reason})"))
            .unwrap_or_default();
        format!("{:<20} {}{note}{by_hand}", self.server, what)
    }
}

fn checked(store: &Store, source: &str, row: &mut Row) {
    let Some(check): Option<&SourceCheck> = store.updates.sources.get(source) else {
        return;
    };
    row.checked_at = Some(check.at);
    row.error = check.error.clone();
    row.latest = check.latest.clone();
}

fn package_row(server: &ServerRecord, store: &Store) -> Option<Row> {
    let pin = pinned(server)?;
    let mut row = Row::new(server, pin.target.kind.word(), State::Unchecked);
    row.package = Some(pin.target.name.clone());
    row.tracks = pin.target.followed_tag().map(String::from);
    row.pinned = Some(pin.version.clone());
    row.manual = pin.manual.clone();
    checked(store, &pin.target.source(), &mut row);
    row.state = match (&row.error, &row.latest) {
        (Some(_), _) => State::Error,
        (None, None) if row.checked_at.is_none() => State::Unchecked,
        (None, Some(latest))
            if changed(pin.target.followed_tag().is_some(), latest, &pin.version) =>
        {
            State::UpdateAvailable
        }
        _ => State::UpToDate,
    };
    Some(row)
}

fn git_row(server: &ServerRecord, repos: &[repo::Repo]) -> Option<Row> {
    let found = repos
        .iter()
        .find(|repo| repo.servers.iter().any(|(id, _)| *id == server.id))?;
    let state = &found.state;
    let mut row = Row::new(
        server,
        "git",
        match state.verdict() {
            Verdict::Behind => State::UpdateAvailable,
            Verdict::Dirty | Verdict::Diverged | Verdict::Busy | Verdict::Detached => {
                State::Blocked
            }
            _ => State::UpToDate,
        },
    );
    row.branch = state.branch.clone();
    row.upstream = state.upstream.clone();
    row.behind = state.behind;
    row.root = Some(state.root.to_string_lossy().into_owned());
    if let Some(check) = &found.checked {
        row.checked_at = Some(check.at);
        row.error = check.error.clone();
    }
    Some(row)
}

/// One row per local server, or the one `key` names. A remote server has no code to update and
/// no row.
pub fn rows(host: &dyn Host, key: Option<&str>) -> Result<Vec<Row>, OpError> {
    let servers = read_servers(host)?;
    let wanted: Vec<&ServerRecord> =
        match key {
            Some(key) => vec![find(&servers, key)
                .ok_or_else(|| OpError::not_found(format!("no server '{key}'")))?],
            None => servers.iter().collect(),
        };
    let store = match host.data_dir() {
        Some(base) => Store::read(&base).map_err(|e| OpError::failed("server_update_store", e))?,
        None => Store::default(),
    };
    let repos = repo::repos(host).unwrap_or_default();
    let mut out = Vec::new();
    for server in wanted {
        if super::super::is_remote(server) {
            if key.is_some() {
                return Err(OpError::not_found(format!(
                    "{} runs remotely, so it has no code of its own to update",
                    server.name
                )));
            }
            continue;
        }
        let row = package_row(server, &store)
            .or_else(|| git_row(server, &repos))
            .or_else(|| {
                automatic(server).map(|kind| {
                    let mut row = Row::new(server, kind.word(), State::Automatic);
                    row.package = None;
                    row
                })
            })
            .unwrap_or_else(|| Row::new(server, "none", State::Unknown));
        out.push(row);
    }
    out.sort_by_key(|row| row.server.to_lowercase());
    Ok(out)
}
