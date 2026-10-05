use super::exec::{CmdOutput, GitRunner};
use std::path::Path;
use std::time::Duration;

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const LOCAL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: String,
    pub remote_ref: String,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub summaries: Vec<String>,
}

fn local(git: &dyn GitRunner, repo: &Path, args: &[&str]) -> Result<CmdOutput, String> {
    git.git(repo, args, LOCAL_TIMEOUT)
}

pub fn is_repo(git: &dyn GitRunner, repo: &Path) -> bool {
    repo.exists()
        && local(git, repo, &["rev-parse", "--is-inside-work-tree"])
            .map(|o| o.ok() && o.stdout.trim() == "true")
            .unwrap_or(false)
}

pub fn remote_url(git: &dyn GitRunner, repo: &Path) -> Option<String> {
    let out = local(git, repo, &["remote", "get-url", "origin"]).ok()?;
    let url = out.stdout.trim().to_string();
    (out.ok() && !url.is_empty()).then_some(url)
}

pub fn list_remotes(git: &dyn GitRunner, repo: &Path) -> Vec<String> {
    local(git, repo, &["remote"])
        .map(|o| {
            o.stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The branch actually checked out, or `None` when detached. Unlike `default_branch`, this
/// never looks at a remote: it is the same name MIG-UPD-4 needs for the stored `branch` field.
pub fn current_branch(git: &dyn GitRunner, repo: &Path) -> Option<String> {
    local(git, repo, &["branch", "--show-current"])
        .ok()
        .map(|o| o.stdout.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The remote and branch the current branch tracks (`@{u}`), parsed from
/// `refs/remotes/<remote>/<branch>`. `None` when the branch has no upstream configured.
pub fn tracking_upstream(git: &dyn GitRunner, repo: &Path) -> Option<(String, String)> {
    let out = local(
        git,
        repo,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
    .filter(CmdOutput::ok)?;
    let full = out.stdout.trim();
    let (remote, branch) = full.split_once('/')?;
    (!remote.is_empty() && !branch.is_empty()).then(|| (remote.to_string(), branch.to_string()))
}

pub fn remote_branch_sha(git: &dyn GitRunner, repo: &Path, remote: &str, branch: &str) -> Option<String> {
    local(
        git,
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote}/{branch}"),
        ],
    )
    .ok()
    .filter(CmdOutput::ok)
    .map(|o| o.stdout.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// Every branch a remote has, for a branch picker: the short names under `refs/remotes/<remote>`
/// with the synthetic `HEAD` ref filtered out.
pub fn remote_branches(git: &dyn GitRunner, repo: &Path, remote: &str) -> Vec<String> {
    let prefix = format!("{remote}/");
    local(
        git,
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            &format!("refs/remotes/{remote}"),
        ],
    )
    .map(|o| {
        o.stdout
            .lines()
            .filter_map(|l| l.trim().strip_prefix(&prefix))
            .filter(|b| *b != "HEAD")
            .map(String::from)
            .collect()
    })
    .unwrap_or_default()
}

/// Like `default_branch`, but for any remote, not only `origin` (a fork's upstream is rarely
/// named `origin`).
pub fn remote_default_branch(git: &dyn GitRunner, repo: &Path, remote: &str) -> Option<String> {
    if let Ok(out) = local(
        git,
        repo,
        &["symbolic-ref", "--short", &format!("refs/remotes/{remote}/HEAD")],
    ) {
        let prefix = format!("{remote}/");
        if let Some(b) = out.stdout.trim().strip_prefix(&prefix).filter(|_| out.ok()) {
            return Some(b.to_string());
        }
    }
    for candidate in ["main", "master"] {
        let r = format!("refs/remotes/{remote}/{candidate}");
        if local(git, repo, &["rev-parse", "--verify", "--quiet", &r])
            .map(|o| o.ok())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }
    None
}

pub fn head_sha(git: &dyn GitRunner, repo: &Path) -> Option<String> {
    local(git, repo, &["rev-parse", "HEAD"])
        .ok()
        .filter(CmdOutput::ok)
        .map(|o| o.stdout.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn default_branch(git: &dyn GitRunner, repo: &Path) -> Option<String> {
    if let Ok(out) = local(
        git,
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        if let Some(b) = out
            .stdout
            .trim()
            .strip_prefix("origin/")
            .filter(|_| out.ok())
        {
            return Some(b.to_string());
        }
    }
    for candidate in ["main", "master"] {
        let r = format!("refs/remotes/origin/{candidate}");
        if local(git, repo, &["rev-parse", "--verify", "--quiet", &r])
            .map(|o| o.ok())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }
    None
}

fn classify_fetch_error(out: &CmdOutput) -> String {
    let text = out.stderr.to_ascii_lowercase();
    if text.contains("permission denied") || text.contains("authentication failed") {
        "git fetch failed: authentication failed (is your credential helper or SSH agent available?)"
            .to_string()
    } else {
        format!("git fetch failed: {}", out.first_error_line())
    }
}

pub fn check(
    git: &dyn GitRunner,
    repo: &Path,
    branch_hint: Option<&str>,
) -> Result<GitStatus, String> {
    if !repo.exists() {
        return Err(format!("path not found: {}", repo.display()));
    }
    if !is_repo(git, repo) {
        return Err("not a git repository".into());
    }
    let dirty = local(git, repo, &["status", "--porcelain"])?
        .stdout
        .lines()
        .any(|l| !l.trim().is_empty());
    let fetched = git.git(repo, &["fetch", "--quiet", "origin"], FETCH_TIMEOUT)?;
    if !fetched.ok() {
        return Err(classify_fetch_error(&fetched));
    }
    let upstream = local(
        git,
        repo,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
    .filter(CmdOutput::ok)
    .map(|o| o.stdout.trim().to_string())
    .filter(|s| !s.is_empty());
    let remote_ref = match upstream {
        Some(u) => u,
        None => {
            let branch = branch_hint
                .map(String::from)
                .or_else(|| default_branch(git, repo))
                .unwrap_or_else(|| "main".into());
            format!("origin/{branch}")
        }
    };
    let branch = local(git, repo, &["branch", "--show-current"])
        .ok()
        .map(|o| o.stdout.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "HEAD".into());
    let counts = local(
        git,
        repo,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("HEAD...{remote_ref}"),
        ],
    )?;
    if !counts.ok() {
        return Err(format!(
            "could not compare with {remote_ref}: {}",
            counts.first_error_line()
        ));
    }
    let mut nums = counts.stdout.split_whitespace().map(|n| n.parse::<u32>());
    let (Some(Ok(ahead)), Some(Ok(behind))) = (nums.next(), nums.next()) else {
        return Err(format!("unexpected rev-list output for {remote_ref}"));
    };
    let summaries = if behind > 0 {
        local(
            git,
            repo,
            &[
                "log",
                "--oneline",
                "--max-count=50",
                "--reverse",
                &format!("HEAD..{remote_ref}"),
            ],
        )
        .map(|o| o.stdout.lines().map(String::from).collect())
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(GitStatus {
        branch,
        remote_ref,
        ahead,
        behind,
        dirty,
        summaries,
    })
}

pub fn fast_forward(git: &dyn GitRunner, repo: &Path, remote_ref: &str) -> Result<(), String> {
    let out = git.git(repo, &["merge", "--ff-only", remote_ref], FETCH_TIMEOUT)?;
    if out.ok() {
        return Ok(());
    }
    let text = out.stderr.to_ascii_lowercase();
    if text.contains("not possible to fast-forward") || text.contains("diverging") {
        Err("cannot fast-forward: local and remote have diverged".into())
    } else {
        Err(format!(
            "git merge --ff-only failed: {}",
            out.first_error_line()
        ))
    }
}
