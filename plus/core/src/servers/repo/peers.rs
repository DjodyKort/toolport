//! This machine's checkouts set against what the other machines published about theirs. Reads
//! git as it is on disk and the last pull's records; nothing is fetched or changed here.

use std::path::{Path, PathBuf};

use toolport_plus_api::ServerRepoHead;

use super::Repo;
use crate::library::remote::git_out;
use crate::sources::gitx::normalize_url;
use crate::sync::peers::Peers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Same,
    /// The other machine's commit descends from this one's, by this many commits.
    Newer(u64),
    Older,
    Diverged,
    /// The commit is not in this checkout (not fetched yet, or never on this remote).
    Unknown,
}

impl Relation {
    pub fn word(self) -> &'static str {
        match self {
            Relation::Same => "same",
            Relation::Newer(_) => "newer",
            Relation::Older => "older",
            Relation::Diverged => "diverged",
            Relation::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeerView {
    pub machine: String,
    pub made_at: String,
    pub seen_at: i64,
    pub server: String,
    pub server_id: String,
    pub root: PathBuf,
    pub theirs: ServerRepoHead,
    pub mine_branch: String,
    pub mine_head: String,
    pub relation: Relation,
    /// This machine fetched the checkout's remote after it read the other machine's record.
    pub fetched_since: bool,
}

fn has_commit(root: &Path, id: &str) -> bool {
    git_out(root, &["cat-file", "-e", &format!("{id}^{{commit}}")]).is_some()
}

fn is_ancestor(root: &Path, ancestor: &str, of: &str) -> bool {
    git_out(root, &["merge-base", "--is-ancestor", ancestor, of]).is_some()
}

pub fn relation(root: &Path, mine: &str, theirs: &str) -> Relation {
    if mine == theirs {
        return Relation::Same;
    }
    if !has_commit(root, theirs) {
        return Relation::Unknown;
    }
    if is_ancestor(root, mine, theirs) {
        let count = git_out(root, &["rev-list", "--count", &format!("{mine}..{theirs}")])
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        Relation::Newer(count)
    } else if is_ancestor(root, theirs, mine) {
        Relation::Older
    } else {
        Relation::Diverged
    }
}

/// One view per server both machines know (same name, same remote), per other machine.
pub fn views(repos: &[Repo], peers: &Peers) -> Vec<PeerView> {
    let mut out = Vec::new();
    for record in &peers.machines {
        for theirs in &record.heads {
            let Some(repo) = repos
                .iter()
                .find(|r| r.servers.iter().any(|(_, name)| *name == theirs.server))
            else {
                continue;
            };
            let state = &repo.state;
            let (Some(branch), Some(head)) = (&state.branch, &state.head) else {
                continue;
            };
            let same_remote = match &state.url {
                Some(url) if !theirs.remote.is_empty() => {
                    normalize_url(url) == normalize_url(&theirs.remote)
                }
                _ => true,
            };
            if !same_remote {
                continue;
            }
            let server_id = repo
                .servers
                .iter()
                .find(|(_, name)| *name == theirs.server)
                .map(|(id, _)| id.clone())
                .unwrap_or_default();
            out.push(PeerView {
                machine: record.machine.clone(),
                made_at: record.made_at.clone(),
                seen_at: peers.seen_at,
                server: theirs.server.clone(),
                server_id,
                root: state.root.clone(),
                theirs: theirs.clone(),
                mine_branch: branch.clone(),
                mine_head: head.clone(),
                relation: relation(&state.root, head, &theirs.head),
                fetched_since: repo
                    .checked
                    .as_ref()
                    .is_some_and(|c| c.error.is_none() && c.at > peers.seen_at),
            });
        }
    }
    out
}

/// Commits of this checkout that sit on no remote.
pub fn unpublished(repo: &Repo) -> u64 {
    let state = &repo.state;
    if state.head.is_none() {
        return 0;
    }
    if state.upstream_head.is_some() {
        return state.ahead;
    }
    git_out(
        &state.root,
        &["rev-list", "--count", "HEAD", "--not", "--remotes"],
    )
    .and_then(|n| n.parse().ok())
    .unwrap_or(0)
}
