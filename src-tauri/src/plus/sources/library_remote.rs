//! `remote-library`: where the skills repository clone stands against its remote, and which other
//! clones of the same remote exist under the configured roots (D-063). Everything here reads; the
//! network is touched only when `fetch` is asked for (a read-only `git ls-remote`, then a fetch),
//! and credentials come from the existing git and gh configuration without ever being read or
//! printed (only the remote URL, with embedded credentials redacted, appears in the output).

use super::{fsx, gitx, library, ScanCtx};
use crate::plus::context::{ContextConfig, Roots};
use crate::plus::exec::{git_command, run_command};
use crate::plus::op::OpError;
use crate::plus::skills::taps::redact;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(20);
const NET_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default)]
pub struct LocalState {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub head: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub uncommitted: u64,
    pub url: Option<String>,
}

pub(super) fn git_out(repo: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = git_command(Some(repo), args);
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    let out = run_command(cmd, READ_TIMEOUT).ok()?;
    out.ok().then(|| out.stdout.trim().to_string())
}

pub fn is_git(repo: &Path) -> bool {
    repo.join(".git").exists()
}

pub fn local_state(repo: &Path) -> LocalState {
    let branch = git_out(repo, &["symbolic-ref", "--short", "-q", "HEAD"]).filter(|s| !s.is_empty());
    let upstream = git_out(
        repo,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .filter(|s| !s.is_empty())
    .or_else(|| {
        let name = format!("origin/{}", branch.as_deref()?);
        git_out(repo, &["rev-parse", "--verify", "-q", &format!("refs/remotes/{name}")])
            .filter(|s| !s.is_empty())
            .map(|_| name)
    });
    let (ahead, behind) = upstream
        .as_deref()
        .and_then(|u| {
            let text = git_out(
                repo,
                &["rev-list", "--left-right", "--count", &format!("HEAD...{u}")],
            )?;
            let mut parts = text.split_whitespace();
            Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
        })
        .unwrap_or((0, 0));
    let uncommitted = git_out(repo, &["status", "--porcelain", "--untracked-files=all"])
        .map_or(0, |t| t.lines().filter(|l| !l.trim().is_empty()).count() as u64);
    LocalState {
        branch,
        upstream,
        head: git_out(repo, &["rev-parse", "--verify", "-q", "HEAD"]).filter(|s| !s.is_empty()),
        ahead,
        behind,
        uncommitted,
        url: git_out(repo, &["config", "--get", "remote.origin.url"])
            .filter(|s| !s.is_empty())
            .map(|u| redact(&u)),
    }
}

fn is_local(url: &str) -> bool {
    url.starts_with("file://") || (!url.contains("://") && !url.contains('@'))
}

/// How git reaches the remote: ssh for `git@host:path` and `ssh://`, gh when the configured
/// credential helper is `gh auth git-credential`, otherwise whatever other helper git has.
/// The flag says whether any credential source is configured at all.
fn auth_method(repo: &Path, url: &str) -> (&'static str, bool) {
    if url.starts_with("ssh://") || (!url.contains("://") && url.contains('@') && url.contains(':'))
    {
        return ("ssh", true);
    }
    let helpers = git_out(repo, &["config", "--get-regexp", r"^credential\..*helper$"])
        .unwrap_or_default();
    if helpers.lines().any(|l| l.contains("gh auth git-credential")) {
        return ("gh", true);
    }
    ("git-credential", !helpers.trim().is_empty() || is_local(url))
}

fn classify(stderr: &str) -> &'static str {
    let text = stderr.to_lowercase();
    if ["authentication", "could not read username", "permission denied", "403", "401"]
        .iter()
        .any(|k| text.contains(k))
    {
        "authentication failed"
    } else if ["could not resolve", "unable to access", "connection", "timed out"]
        .iter()
        .any(|k| text.contains(k))
    {
        "network unreachable"
    } else if text.contains("not found") || text.contains("does not appear to be a git repository")
    {
        "repository not found"
    } else {
        "git could not reach the remote"
    }
}

fn network(repo: &Path, args: &[&str]) -> Result<(), String> {
    let out = run_command(git_command(Some(repo), args), NET_TIMEOUT)
        .map_err(|_| "git did not run".to_string())?;
    if out.ok() {
        Ok(())
    } else {
        Err(classify(&out.stderr).to_string())
    }
}

/// A read-only `git ls-remote`; the failure is a classified phrase, never git's own text.
pub fn probe(repo: &Path) -> Result<(), String> {
    network(repo, &["ls-remote", "--heads", "origin"])
}

pub fn fetch(repo: &Path) -> Result<(), String> {
    network(repo, &["fetch", "--quiet", "origin"])
}

pub fn last_fetch(repo: &Path) -> Option<String> {
    fsx::mtime_secs(&repo.join(".git").join("FETCH_HEAD")).map(fsx::zulu)
}

/// The library clone: the first candidate (the sync config's, then the default location).
pub fn locate(ctx: &ScanCtx) -> Result<PathBuf, OpError> {
    let root = library::candidates(ctx)
        .into_iter()
        .next()
        .ok_or_else(|| OpError::not_found("no skills library found"))?;
    if !is_git(&root) {
        return Err(OpError::not_found(format!(
            "{} is not a git repository",
            root.display()
        )));
    }
    Ok(root)
}

pub fn host_repo() -> Result<PathBuf, OpError> {
    let (roots, config) = super::host_world()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data_dir = crate::registry::conduit_dir();
    super::with_ctx(&roots, &config, data_dir.as_deref(), locate)
}

fn duplicate_clones(ctx: &ScanCtx, root: &Path, url: &str) -> Vec<Value> {
    let found = library::candidates(ctx);
    let canon = fsx::canonical(root);
    let others: Vec<PathBuf> = found
        .into_iter()
        .filter(|p| fsx::canonical(p) != canon)
        .collect();
    library::duplicates(ctx, root, &gitx::normalize_url(url), &others)
        .into_iter()
        .map(|twin| {
            let state = local_state(&twin);
            let skills = library::scan_skills_repo(&twin, ctx)
                .iter()
                .filter(|f| f.kind == "skill")
                .count();
            json!({
                "path": fsx::display(&twin),
                "skills": skills,
                "ahead": state.ahead,
                "behind": state.behind,
                "sameRemote": true,
            })
        })
        .collect()
}

pub fn status(
    roots: &Roots,
    config: &ContextConfig,
    data_dir: Option<&Path>,
    fetch_remote: bool,
) -> Result<Value, OpError> {
    super::with_ctx(roots, config, data_dir, |ctx| {
        let root = locate(ctx)?;
        let before = local_state(&root);
        let url = before.url.clone();
        let (method, configured) = url
            .as_deref()
            .map_or(("git-credential", false), |u| auth_method(&root, u));
        let mut fetch_result = json!({"requested": fetch_remote, "ok": null, "error": null});
        let mut auth_ok = configured;
        if fetch_remote && url.is_some() {
            let outcome = probe(&root).and_then(|_| fetch(&root));
            auth_ok = outcome.is_ok();
            fetch_result["ok"] = json!(outcome.is_ok());
            fetch_result["error"] = json!(outcome.err());
        }
        let state = if fetch_remote { local_state(&root) } else { before };
        let twins = url
            .as_deref()
            .map(|u| duplicate_clones(ctx, &root, u))
            .unwrap_or_default();
        Ok(json!({
            "repo": fsx::display(&root),
            "remote": url,
            "branch": state.branch,
            "upstream": state.upstream,
            "ahead": state.ahead,
            "behind": state.behind,
            "uncommitted": state.uncommitted,
            "lastFetch": last_fetch(&root),
            "fetch": fetch_result,
            "auth": {"method": method, "ok": auth_ok, "checked": fetch_remote && url.is_some()},
            "duplicateClones": twins,
        }))
    })
}

pub fn status_host(fetch_remote: bool) -> Result<Value, OpError> {
    let (roots, config) = super::host_world()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data_dir = crate::registry::conduit_dir();
    status(&roots, &config, data_dir.as_deref(), fetch_remote)
}

pub fn human(data: &Value) -> String {
    let text = |key: &str| data[key].as_str().unwrap_or("-").to_string();
    let mut out = format!(
        "{}\n  remote:  {}\n  branch:  {} (upstream {})\n  ahead {} / behind {} / uncommitted {}\n  last fetch: {}\n  auth: {} ({})\n",
        text("repo"),
        text("remote"),
        text("branch"),
        text("upstream"),
        data["ahead"],
        data["behind"],
        data["uncommitted"],
        text("lastFetch"),
        data["auth"]["method"].as_str().unwrap_or(""),
        match (data["auth"]["checked"].as_bool(), data["auth"]["ok"].as_bool()) {
            (Some(true), Some(true)) => "works",
            (Some(true), _) => "does not work",
            (_, Some(true)) => "configured, not checked",
            _ => "not configured",
        },
    );
    for twin in data["duplicateClones"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  duplicate clone: {} ({} skills, ahead {} / behind {})\n",
            twin["path"].as_str().unwrap_or(""),
            twin["skills"],
            twin["ahead"],
            twin["behind"]
        ));
    }
    out
}
