//! Where the skills library clone stands against its remote, and which other clones of the same
//! remote exist next to it. Everything here reads; the network is touched only when `fetch` is
//! asked for (a read-only `git ls-remote`, then a fetch). Credentials come from the existing git
//! and gh configuration without ever being read or printed: only the remote URL, with embedded
//! credentials redacted, appears in the output.

use crate::op::OpError;
use crate::skills::clock::Instant;
use crate::skills::gitexec::{git_command, run_command};
use crate::skills::taps::redact;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

const READ_TIMEOUT: Duration = Duration::from_secs(20);
const NET_TIMEOUT: Duration = Duration::from_secs(60);
const SYNC_CONFIG: &str = "skills_sync.json";
const CONTENT_DIRS: [&str; 6] = [
    "skills", "rules", "agents", "commands", "styles", "profiles",
];

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

pub(crate) fn git_out(repo: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = git_command(Some(repo), args);
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    let out = run_command(cmd, READ_TIMEOUT).ok()?;
    out.ok().then(|| out.stdout.trim().to_string())
}

pub fn is_git(repo: &Path) -> bool {
    repo.join(".git").exists()
}

fn nonempty(text: Option<String>) -> Option<String> {
    text.filter(|s| !s.is_empty())
}

pub fn local_state(repo: &Path) -> LocalState {
    let branch = nonempty(git_out(repo, &["symbolic-ref", "--short", "-q", "HEAD"]));
    let upstream = nonempty(git_out(
        repo,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    ))
    .or_else(|| {
        let name = format!("origin/{}", branch.as_deref()?);
        nonempty(git_out(
            repo,
            &[
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/remotes/{name}"),
            ],
        ))
        .map(|_| name)
    });
    let (ahead, behind) = upstream
        .as_deref()
        .and_then(|u| {
            let text = git_out(
                repo,
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("HEAD...{u}"),
                ],
            )?;
            let mut parts = text.split_whitespace();
            Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
        })
        .unwrap_or((0, 0));
    let uncommitted = git_out(repo, &["status", "--porcelain", "--untracked-files=all"])
        .map_or(0, |t| {
            t.lines().filter(|l| !l.trim().is_empty()).count() as u64
        });
    LocalState {
        branch,
        upstream,
        head: nonempty(git_out(repo, &["rev-parse", "--verify", "-q", "HEAD"])),
        ahead,
        behind,
        uncommitted,
        url: nonempty(git_out(repo, &["config", "--get", "remote.origin.url"])).map(|u| redact(&u)),
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
    let helpers =
        git_out(repo, &["config", "--get-regexp", r"^credential\..*helper$"]).unwrap_or_default();
    if helpers
        .lines()
        .any(|l| l.contains("gh auth git-credential"))
    {
        return ("gh", true);
    }
    (
        "git-credential",
        !helpers.trim().is_empty() || is_local(url),
    )
}

fn classify(stderr: &str) -> &'static str {
    let text = stderr.to_lowercase();
    if [
        "authentication",
        "could not read username",
        "permission denied",
        "403",
        "401",
    ]
    .iter()
    .any(|k| text.contains(k))
    {
        "authentication failed"
    } else if [
        "could not resolve",
        "unable to access",
        "connection",
        "timed out",
    ]
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

fn last_fetch(repo: &Path) -> Option<String> {
    let secs = std::fs::metadata(repo.join(".git").join("FETCH_HEAD"))
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(
        Instant {
            unix_secs: secs as i64,
            micros: 0,
        }
        .zulu(),
    )
}

fn expand_user(home: &Path, raw: &str) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    match raw.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(raw),
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn sync_path(dir: &Path) -> Option<PathBuf> {
    read_json(&dir.join(SYNC_CONFIG))?
        .get("local_path")?
        .as_str()
        .map(PathBuf::from)
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn child_dirs(path: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// The clones that could be the library, the one the sync config names first: the data
/// directory's, then the mcpm config directory's, then its default location.
fn candidates(home: &Path) -> Vec<PathBuf> {
    let config_dir = home.join(".config/mcpm");
    let mut raw: Vec<PathBuf> = Vec::new();
    if let Some(data) = crate::fsx::data_dir() {
        raw.extend(sync_path(&data).map(|p| expand_user(home, &p.to_string_lossy())));
    }
    raw.extend(sync_path(&config_dir).map(|p| expand_user(home, &p.to_string_lossy())));
    raw.push(config_dir.join("skills_repo"));
    let mut out: Vec<PathBuf> = Vec::new();
    for path in raw {
        let canon = canonical(&path);
        if path.is_dir() && !out.iter().any(|p| canonical(p) == canon) {
            out.push(path);
        }
    }
    out
}

pub fn locate() -> Result<PathBuf, OpError> {
    let home = crate::fsx::home()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let root = candidates(&home)
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

fn looks_like_skills_repo(dir: &Path) -> bool {
    CONTENT_DIRS.iter().any(|d| dir.join(d).is_dir())
}

fn normalize_url(url: &str) -> String {
    let mut text = url.trim().to_lowercase();
    if let Some((_, rest)) = text.split_once("://") {
        text = rest.to_string();
        if let Some((_, after)) = text.split_once('@') {
            text = after.to_string();
        }
    } else if let Some((_, rest)) = text.split_once('@') {
        text = rest.replacen(':', "/", 1);
    }
    text.trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string()
}

fn same_remote(dir: &Path, remote: &str) -> bool {
    git_out(dir, &["config", "--get", "remote.origin.url"])
        .is_some_and(|url| normalize_url(&url) == remote)
}

fn configured_roots(home: &Path) -> Vec<PathBuf> {
    read_json(&home.join(".config/mcpm/context.json"))
        .and_then(|doc| doc.get("sourceRoots").cloned())
        .and_then(|roots| roots.as_array().cloned())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(|raw| expand_user(home, raw)))
        .collect()
}

/// Clones of the same remote: other candidates, the configured source folders and their children,
/// and the folders next to the library.
fn duplicates(home: &Path, root: &Path, remote: &str) -> Vec<PathBuf> {
    let own = canonical(root);
    let mut pool: Vec<PathBuf> = candidates(home);
    for dir in configured_roots(home) {
        if dir.is_dir() {
            pool.extend(child_dirs(&dir));
            pool.push(dir);
        }
    }
    if let Some(parent) = root.parent() {
        pool.extend(child_dirs(parent));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in pool {
        let canon = canonical(&dir);
        if canon == own
            || found.iter().any(|p| canonical(p) == canon)
            || !looks_like_skills_repo(&dir)
            || !same_remote(&dir, remote)
        {
            continue;
        }
        found.push(dir);
    }
    found
}

fn skill_count(repo: &Path) -> usize {
    child_dirs(&repo.join("skills"))
        .iter()
        .filter(|dir| dir.join("SKILL.md").is_file())
        .count()
}

fn duplicate_clones(home: &Path, root: &Path, url: &str) -> Vec<Value> {
    duplicates(home, root, &normalize_url(url))
        .into_iter()
        .map(|twin| {
            let state = local_state(&twin);
            json!({
                "path": twin.to_string_lossy(),
                "skills": skill_count(&twin),
                "ahead": state.ahead,
                "behind": state.behind,
                "sameRemote": true,
            })
        })
        .collect()
}

pub fn status(fetch_remote: bool) -> Result<Value, OpError> {
    let home = crate::fsx::home()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let root = locate()?;
    let before = local_state(&root);
    let url = before.url.clone();
    let (method, configured) = url
        .as_deref()
        .map_or(("git-credential", false), |u| auth_method(&root, u));
    let mut fetch_result = json!({"requested": fetch_remote, "ok": null, "error": null});
    let mut auth_ok = configured;
    if fetch_remote && url.is_some() {
        let outcome = probe(&root).and_then(|()| fetch(&root));
        auth_ok = outcome.is_ok();
        fetch_result["ok"] = json!(outcome.is_ok());
        fetch_result["error"] = json!(outcome.err());
    }
    let state = if fetch_remote {
        local_state(&root)
    } else {
        before
    };
    let twins = url
        .as_deref()
        .map(|u| duplicate_clones(&home, &root, u))
        .unwrap_or_default();
    Ok(json!({
        "repo": root.to_string_lossy(),
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
