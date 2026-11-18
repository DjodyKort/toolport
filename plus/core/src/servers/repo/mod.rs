//! Local servers that run from a git checkout: where each checkout stands against the branch it
//! tracks, a fetch of that remote, and the update itself (see [`update`]). The same reads feed
//! the attention list's update rows.

pub mod discover;
pub mod peers;
pub mod state;
pub mod store;
pub mod switch;
pub mod update;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toolport_plus_api::{Host, ServerRepoHead};

use self::discover::discover;
use self::state::{RepoState, Verdict};
use super::read_servers;
use crate::attention::store::{SourceCheck, Store};
use crate::attention::updates as checks;
use crate::library::remote::{self, git_out};
use crate::op::OpError;
use crate::tasks::cron::rfc3339;

/// One checkout and every server that runs from it.
#[derive(Clone, Debug)]
pub struct Repo {
    /// Id and name of each server, by name.
    pub servers: Vec<(String, String)>,
    pub state: RepoState,
    pub checked: Option<SourceCheck>,
}

impl Repo {
    pub fn first_name(&self) -> &str {
        self.servers.first().map_or("", |(_, name)| name.as_str())
    }

    pub fn source(&self) -> String {
        source_name(self.first_name())
    }
}

/// The name a checkout's fetch is recorded under in the update check.
pub fn source_name(server: &str) -> String {
    format!("server:{server}")
}

pub fn group(found: Vec<discover::Found>) -> Vec<(PathBuf, Vec<(String, String)>)> {
    let mut by_root: BTreeMap<PathBuf, Vec<(String, String)>> = BTreeMap::new();
    for item in found {
        by_root
            .entry(item.root)
            .or_default()
            .push((item.server_id, item.server_name));
    }
    let mut groups: Vec<_> = by_root.into_iter().collect();
    for (_, servers) in &mut groups {
        servers.sort_by_key(|server| server.1.to_lowercase());
    }
    groups.sort_by(|a, b| {
        let name = |g: &(PathBuf, Vec<(String, String)>)| g.1[0].1.to_lowercase();
        name(a).cmp(&name(b))
    });
    groups
}

/// The checkouts the registry's servers run from, read from git as it is on disk.
pub fn repos(host: &dyn Host) -> Result<Vec<Repo>, OpError> {
    let servers = read_servers(host)?;
    let found = discover(&servers, crate::fsx::home().as_deref());
    let store = host
        .data_dir()
        .and_then(|base| Store::read(&base).ok())
        .unwrap_or_default();
    Ok(group(found)
        .into_iter()
        .map(|(root, servers)| {
            let mut repo = Repo {
                servers,
                state: state::read(&root),
                checked: None,
            };
            repo.checked = store.updates.sources.get(&repo.source()).cloned();
            repo
        })
        .collect())
}

/// [`repos`], narrowed to the checkout `key` (a server id or name) runs from.
pub fn repos_for(host: &dyn Host, key: Option<&str>) -> Result<Vec<Repo>, OpError> {
    let mut found = repos(host)?;
    let Some(key) = key else {
        return Ok(found);
    };
    let servers = read_servers(host)?;
    let server = super::find(&servers, key)
        .ok_or_else(|| OpError::not_found(format!("no server '{key}'")))?;
    found.retain(|repo| repo.servers.iter().any(|(id, _)| *id == server.id));
    if found.is_empty() {
        return Err(OpError::not_found(format!(
            "{} does not run from a git checkout on this machine",
            server.name
        )));
    }
    Ok(found)
}

/// The state of one checkout in the shape another machine can compare.
pub fn head_records(repos: &[Repo]) -> Vec<ServerRepoHead> {
    let mut out = Vec::new();
    for repo in repos {
        let state = &repo.state;
        let (Some(branch), Some(head)) = (&state.branch, &state.head) else {
            continue;
        };
        for (_, name) in &repo.servers {
            out.push(ServerRepoHead {
                server: name.clone(),
                remote: state.url.clone().unwrap_or_default(),
                branch: branch.clone(),
                head: head.clone(),
                pushed: state.upstream_head.is_some() && state.ahead == 0,
                extra: Default::default(),
            });
        }
    }
    out
}

fn row(repo: &Repo, id: &str, name: &str) -> Value {
    let state = &repo.state;
    json!({
        "server": name,
        "id": id,
        "root": state.root.to_string_lossy(),
        "branch": state.branch,
        "head": state.head,
        "remote": state.remote,
        "url": state.url,
        "upstream": state.upstream,
        "upstreamHead": state.upstream_head,
        "ahead": state.ahead,
        "behind": state.behind,
        "dirty": state.dirty,
        "unpushed": state.ahead,
        "state": state.verdict().word(),
        "checkedAt": repo.checked.as_ref().map(|c| rfc3339(c.at)),
        "error": repo.checked.as_ref().and_then(|c| c.error.clone()),
    })
}

pub fn status_value(repos: &[Repo]) -> Value {
    let rows: Vec<Value> = repos
        .iter()
        .flat_map(|repo| repo.servers.iter().map(|(id, name)| row(repo, id, name)))
        .collect();
    json!({"repos": rows})
}

pub fn status_human(repos: &[Repo]) -> String {
    if repos.is_empty() {
        return "No server runs from a git checkout on this machine.\n".into();
    }
    let mut out = String::new();
    for repo in repos {
        let state = &repo.state;
        let place = match (&state.branch, &state.upstream) {
            (Some(branch), Some(upstream)) => format!("{branch} -> {upstream}"),
            (Some(branch), None) => format!("{branch} (tracks nothing)"),
            _ => "detached HEAD".to_string(),
        };
        let say = match state.verdict() {
            Verdict::UpToDate => "up to date".to_string(),
            Verdict::Behind => format!("{} new commit(s)", state.behind),
            Verdict::Dirty => format!(
                "{} new commit(s), but local changes block the update",
                state.behind
            ),
            Verdict::Diverged => format!("diverged: {} local, {} new", state.ahead, state.behind),
            Verdict::Busy => format!("{} is open", state.busy.unwrap_or("an operation")),
            Verdict::Detached => "nothing to track".to_string(),
            Verdict::NoUpstream => "tracks no remote branch".to_string(),
        };
        let names: Vec<&str> = repo.servers.iter().map(|(_, n)| n.as_str()).collect();
        let tail = match &repo.checked {
            Some(check) => match &check.error {
                Some(error) => format!(" (last check failed: {error})"),
                None => format!(" (checked {})", rfc3339(check.at)),
            },
            None => " (never checked)".to_string(),
        };
        let unpushed = if state.ahead > 0 && state.behind == 0 {
            format!(", {} unpushed", state.ahead)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "{}  {}  {place}  {say}{unpushed}{tail}\n",
            names.join(", "),
            state.root.display()
        ));
    }
    out
}

/// Fetches the tracked remote of one checkout, after the egress list has been asked about its
/// host. Never pulls, merges or checks anything out.
pub fn fetch_tracked(root: &Path) -> Result<(), String> {
    let remote_name =
        state::tracked_remote(root).ok_or_else(|| "the branch tracks no remote".to_string())?;
    if let Some(url) = git_out(
        root,
        &["config", "--get", &format!("remote.{remote_name}.url")],
    ) {
        if let Some(host) = checks::host_of(&url) {
            crate::egress::check(&host)?;
        }
    }
    remote::fetch_remote(root, &remote_name)
}

pub struct CheckReport {
    pub checked_at: i64,
    pub repos: Vec<(String, Result<(), String>)>,
}

impl CheckReport {
    pub fn to_value(&self) -> Value {
        let repos: Vec<Value> = self
            .repos
            .iter()
            .map(|(server, result)| match result {
                Ok(()) => json!({"server": server, "ok": true}),
                Err(error) => json!({"server": server, "ok": false, "error": error}),
            })
            .collect();
        json!({"checkedAt": rfc3339(self.checked_at), "repos": repos})
    }

    pub fn human(&self) -> String {
        if self.repos.is_empty() {
            return "No server runs from a git checkout on this machine.\n".into();
        }
        let mut out = String::new();
        for (server, result) in &self.repos {
            match result {
                Ok(()) => out.push_str(&format!("{server}: fetched\n")),
                Err(error) => out.push_str(&format!("{server}: {error}\n")),
            }
        }
        out.push_str("Nothing was pulled or changed.\n");
        out
    }
}

/// Fetches every checkout that tracks a remote and records how each fetch went, so the update
/// rows and `repo status` can say when it was last looked at. `fetch` is the injectable step.
pub fn check(
    host: &dyn Host,
    now: i64,
    fetch: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<CheckReport, OpError> {
    let base = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let found = repos(host)?;
    let targets: Vec<(String, PathBuf)> = found
        .iter()
        .filter(|repo| repo.state.remote.is_some())
        .map(|repo| (repo.source(), repo.state.root.clone()))
        .collect();
    let outcomes = checks::record_fetches(&base, now, &targets, fetch)
        .map_err(|e| OpError::failed("server_repo_check", e))?;
    Ok(CheckReport {
        checked_at: now,
        repos: outcomes
            .into_iter()
            .map(|(source, error)| {
                let name = source
                    .strip_prefix("server:")
                    .unwrap_or(&source)
                    .to_string();
                (name, error.map_or(Ok(()), Err))
            })
            .collect(),
    })
}
