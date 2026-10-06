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
    pub upstream: Option<UpstreamStatus>,
    pub warnings: Vec<String>,
}

/// The branch against `upstream/branch`: `behind` counts upstream commits the branch lacks,
/// `ahead` the branch's own commits on top. Ahead is a fork's normal state, never a divergence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamStatus {
    pub remote_ref: String,
    pub ahead: u32,
    pub behind: u32,
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

/// How the checkout is compared: the stored `remote`/`branch` it follows and the optional
/// `upstream` a fork was cut from. Anything left unset falls back to what the checkout says.
#[derive(Debug, Clone, Copy, Default)]
pub struct Target<'a> {
    pub remote: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub upstream: Option<(&'a str, &'a str)>,
}

fn ahead_behind(git: &dyn GitRunner, repo: &Path, remote_ref: &str) -> Result<(u32, u32), String> {
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
    Ok((ahead, behind))
}

fn incoming_summaries(git: &dyn GitRunner, repo: &Path, remote_ref: &str) -> Vec<String> {
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
}

fn followed_remote(git: &dyn GitRunner, repo: &Path, target: &Target) -> String {
    let configured = target.remote.filter(|r| !r.is_empty()).unwrap_or("origin");
    if target.branch.is_some_and(|b| !b.is_empty()) {
        return configured.to_string();
    }
    tracking_upstream(git, repo)
        .map(|(remote, _)| remote)
        .unwrap_or_else(|| configured.to_string())
}

fn followed_ref(git: &dyn GitRunner, repo: &Path, target: &Target, remote: &str) -> String {
    let tracked = tracking_upstream(git, repo);
    match target.branch.filter(|b| !b.is_empty()) {
        Some(branch) => {
            if remote_branch_sha(git, repo, remote, branch).is_none() {
                if let Some((r, b)) = tracked {
                    return format!("{r}/{b}");
                }
            }
            format!("{remote}/{branch}")
        }
        None => match tracked {
            Some((r, b)) => format!("{r}/{b}"),
            None => {
                let branch =
                    remote_default_branch(git, repo, remote).unwrap_or_else(|| "main".into());
                format!("{remote}/{branch}")
            }
        },
    }
}

pub fn check(
    git: &dyn GitRunner,
    repo: &Path,
    branch_hint: Option<&str>,
) -> Result<GitStatus, String> {
    check_target(
        git,
        repo,
        &Target {
            branch: branch_hint,
            ..Target::default()
        },
    )
}

pub fn check_target(
    git: &dyn GitRunner,
    repo: &Path,
    target: &Target,
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
    let primary = followed_remote(git, repo, target);
    let fetched = git.git(repo, &["fetch", "--quiet", &primary], FETCH_TIMEOUT)?;
    if !fetched.ok() {
        return Err(classify_fetch_error(&fetched));
    }
    let mut warnings = Vec::new();
    let mut unreachable = Vec::new();
    let mut others = list_remotes(git, repo);
    others.retain(|r| *r != primary);
    for remote in others {
        let failure = match git.git(repo, &["fetch", "--quiet", &remote], FETCH_TIMEOUT) {
            Ok(o) if o.ok() => continue,
            Ok(o) => classify_fetch_error(&o),
            Err(e) => e,
        };
        warnings.push(format!("{remote}: {failure}"));
        unreachable.push(remote);
    }
    let remote_ref = followed_ref(git, repo, target, &primary);
    let branch = current_branch(git, repo).unwrap_or_else(|| "HEAD".into());
    let (ahead, behind) = ahead_behind(git, repo, &remote_ref)?;
    let summaries = if behind > 0 {
        incoming_summaries(git, repo, &remote_ref)
    } else {
        Vec::new()
    };
    let upstream = target.upstream.and_then(|(remote, up_branch)| {
        let up_ref = format!("{remote}/{up_branch}");
        if up_ref == remote_ref || unreachable.iter().any(|r| r == remote) {
            return None;
        }
        if remote_branch_sha(git, repo, remote, up_branch).is_none() {
            warnings.push(format!("{up_ref} not found"));
            return None;
        }
        match ahead_behind(git, repo, &up_ref) {
            Ok((own, missing)) => Some(UpstreamStatus {
                summaries: if missing > 0 {
                    incoming_summaries(git, repo, &up_ref)
                } else {
                    Vec::new()
                },
                remote_ref: up_ref,
                ahead: own,
                behind: missing,
            }),
            Err(e) => {
                warnings.push(e);
                None
            }
        }
    });
    Ok(GitStatus {
        branch,
        remote_ref,
        ahead,
        behind,
        dirty,
        summaries,
        upstream,
        warnings,
    })
}

const LS_REMOTE_TIMEOUT: Duration = Duration::from_secs(20);

pub fn looks_like_sha(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// `git ls-remote` of a bare URL, no local clone (MIG-UPD-8: a `uvx
/// --from git+URL[@ref]` package has no checkout to fetch into). `ref_name`
/// is a branch/tag name, or `"HEAD"` for the remote's default branch. `cwd`
/// only needs to exist; `ls-remote` never reads or writes it.
pub fn ls_remote_tip(
    git: &dyn GitRunner,
    cwd: &Path,
    url: &str,
    ref_name: &str,
) -> Result<Option<String>, String> {
    let out = git.git(cwd, &["ls-remote", url, ref_name], LS_REMOTE_TIMEOUT)?;
    if !out.ok() {
        return Err(format!("git ls-remote failed: {}", out.first_error_line()));
    }
    Ok(out
        .stdout
        .lines()
        .find_map(|l| l.split_whitespace().next().map(String::from)))
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
