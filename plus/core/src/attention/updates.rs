//! The update check: the one pass that asks the network, and the local reads that turn its
//! result into rows. The pass only fetches; it never pulls, applies or installs anything, so a
//! tap or the library moves only when the person updates it.

use std::path::Path;

use serde_json::{json, Value};

use super::feed_updates::{RepoFacts, TapFacts, UpdateInputs};
use super::store::{SourceCheck, Store};
use crate::egress;
use crate::library::remote;
use crate::skills::taps::{load_taps, tap_dir, taps_root};
use crate::tasks::cron::rfc3339;

pub const LIBRARY: &str = "library";

fn tap_source(name: &str) -> String {
    format!("tap:{name}")
}

/// The host a remote URL names; `None` for a path on this machine.
pub(crate) fn host_of(url: &str) -> Option<String> {
    let authority = match url.split_once("://") {
        Some((scheme, _)) if scheme.eq_ignore_ascii_case("file") => return None,
        Some((_, rest)) => rest.split('/').next()?,
        None if url.contains('@') && url.contains(':') => url.split(':').next()?,
        None => return None,
    };
    let host = authority.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_string())
}

/// Err names the refused host; the fork never contacts the hosts the egress list names.
pub fn egress_check(repo: &Path) -> Result<(), String> {
    let url = remote::git_out(repo, &["config", "--get", "remote.origin.url"]);
    match url.as_deref().and_then(host_of) {
        Some(host) => egress::check(&host),
        None => Ok(()),
    }
}

pub fn fetch(repo: &Path) -> Result<(), String> {
    egress_check(repo)?;
    remote::fetch(repo)
}

fn has_clone(path: &Path) -> bool {
    path.join(".git").exists()
}

/// What each registered tap's clone says about its remote, from git state already on disk.
pub fn tap_facts(base: &Path) -> Vec<TapFacts> {
    let root = taps_root(base);
    load_taps(base)
        .into_iter()
        .filter_map(|tap| {
            let dir = tap_dir(&root, &tap);
            if !has_clone(&dir) {
                return None;
            }
            let state = remote::local_state(&dir);
            Some(TapFacts {
                marketplace: dir.join(".claude-plugin/marketplace.json").is_file(),
                name: tap.name,
                behind: state.behind,
                head: state.head,
                upstream_head: state.upstream_head,
            })
        })
        .collect()
}

pub fn repo_facts(repo: &crate::servers::repo::Repo) -> RepoFacts {
    let state = &repo.state;
    RepoFacts {
        server: repo.first_name().to_string(),
        also: repo
            .servers
            .iter()
            .skip(1)
            .map(|(_, name)| name.clone())
            .collect(),
        branch: state.branch.clone().unwrap_or_default(),
        upstream: state.upstream.clone().unwrap_or_default(),
        head: state.head.clone(),
        upstream_head: state.upstream_head.clone(),
        behind: state.behind,
        ahead: state.ahead,
        dirty: state.dirty,
        verdict: state.verdict(),
    }
}

pub fn inputs(base: &Path, store: &Store) -> UpdateInputs {
    UpdateInputs {
        taps: tap_facts(base),
        servers: Vec::new(),
        failures: store
            .updates
            .sources
            .iter()
            .filter_map(|(name, check)| Some((name.clone(), check.error.clone()?)))
            .collect(),
        checked: store.updates.sources.len(),
        peers: Vec::new(),
        unpublished: Vec::new(),
    }
}

pub struct Outcome {
    pub source: String,
    pub error: Option<String>,
}

pub struct Report {
    pub checked_at: i64,
    pub off: bool,
    pub sources: Vec<Outcome>,
}

impl Report {
    pub fn to_json(&self) -> Value {
        let sources: Vec<Value> = self
            .sources
            .iter()
            .map(|source| match &source.error {
                Some(error) => json!({"source": source.source, "ok": false, "error": error}),
                None => json!({"source": source.source, "ok": true}),
            })
            .collect();
        json!({"checkedAt": rfc3339(self.checked_at), "off": self.off, "sources": sources})
    }

    pub fn human(&self) -> String {
        if self.off {
            return "The updates check is switched off.\n".into();
        }
        let failed = self.sources.iter().filter(|s| s.error.is_some()).count();
        let mut out = format!(
            "Looked at {} source(s) for new upstream commits; {} could not be reached.\n",
            self.sources.len(),
            failed
        );
        for source in &self.sources {
            if let Some(error) = &source.error {
                out.push_str(&format!("  {}: {error}\n", source.source));
            }
        }
        out.push_str("Nothing was applied.\n");
        out
    }
}

/// Fetches `targets` with `fetch` and records when and how each went, leaving every other
/// source in the store as it is. Returns each source with its error, if any.
pub fn record_fetches(
    base: &Path,
    now: i64,
    targets: &[(String, std::path::PathBuf)],
    fetch: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<Vec<(String, Option<String>)>, String> {
    let mut store = Store::read(base)?;
    let outcomes = fetch_into(&mut store, now, targets, fetch);
    store.write(base)?;
    Ok(outcomes)
}

fn fetch_into(
    store: &mut Store,
    now: i64,
    targets: &[(String, std::path::PathBuf)],
    fetch: &dyn Fn(&Path) -> Result<(), String>,
) -> Vec<(String, Option<String>)> {
    targets
        .iter()
        .map(|(source, dir)| {
            let error = fetch(dir).err();
            store.updates.sources.insert(
                source.clone(),
                SourceCheck {
                    at: now,
                    error: error.clone(),
                },
            );
            (source.clone(), error)
        })
        .collect()
}

/// Fetches every tap clone and the library with `fetch` and records when and how each went.
/// A store that cannot be read stops the pass before anything is fetched.
pub fn check(
    base: &Path,
    now: i64,
    fetch: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<Report, String> {
    check_with(base, now, fetch, &[], &|_| Ok(()))
}

/// [`check`] that also fetches the checkouts local servers run from, each with `fetch_server`.
pub fn check_with(
    base: &Path,
    now: i64,
    fetch: &dyn Fn(&Path) -> Result<(), String>,
    servers: &[(String, std::path::PathBuf)],
    fetch_server: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<Report, String> {
    let mut store = Store::read(base)?;
    if !store.settings.check_on("updates") {
        return Ok(Report {
            checked_at: now,
            off: true,
            sources: Vec::new(),
        });
    }
    let root = taps_root(base);
    let mut targets: Vec<(String, std::path::PathBuf)> = load_taps(base)
        .into_iter()
        .map(|tap| (tap_source(&tap.name), tap_dir(&root, &tap)))
        .filter(|(_, dir)| has_clone(dir))
        .collect();
    if let Ok(repo) = remote::locate() {
        targets.push((LIBRARY.to_string(), repo));
    }
    let sources: Vec<Outcome> = fetch_into(&mut store, now, &targets, fetch)
        .into_iter()
        .chain(fetch_into(&mut store, now, servers, fetch_server))
        .map(|(source, error)| Outcome { source, error })
        .collect();
    let names: Vec<String> = sources.iter().map(|s| s.source.clone()).collect();
    store.updates.sources.retain(|name, _| names.contains(name));
    store.updates.checked_at = Some(now);
    store.write(base)?;
    Ok(Report {
        checked_at: now,
        off: false,
        sources,
    })
}
