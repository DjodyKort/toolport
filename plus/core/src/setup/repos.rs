//! Git repositories under the folders Toolport searches: where each stands (branch, upstream,
//! commits nobody else has, changes, stashes), its linked worktrees and submodules. Only local
//! git commands run; nothing is fetched and nothing is written.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::scan;
use crate::skills::gitexec::{git_command, run_command};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const FIND_DEPTH: usize = 2;
const SKIPPED_DIRS: [&str; 5] = ["node_modules", "target", "vendor", "dist", "build"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchAhead {
    pub branch: String,
    pub ahead: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submodule {
    pub path: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub name: String,
    /// Home-relative (`~/code/web-app`) when the repository is under the home folder.
    pub path: String,
    pub remotes: Vec<Remote>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub head: Option<String>,
    pub default_branch: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub dirty: u64,
    pub untracked: u64,
    pub stashes: u64,
    /// Branches with commits that are not on their upstream.
    pub unpushed: Vec<BranchAhead>,
    /// Branches without an upstream whose commits are on no remote branch.
    pub never_pushed: Vec<String>,
    pub no_remote: bool,
    pub submodules: Vec<Submodule>,
    pub worktrees: Vec<Worktree>,
    /// Ignored files whose names look like secrets (names only; the check lists them).
    #[serde(default)]
    pub ignored_names: Vec<String>,
    /// Why the repository could not be read; such a repository is never counted clean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip)]
    pub abs: PathBuf,
}

impl Repo {
    pub fn origin_url(&self) -> Option<&str> {
        self.remotes
            .iter()
            .find(|r| r.name == "origin")
            .or_else(|| self.remotes.first())
            .map(|r| r.url.as_str())
    }

    /// The commit count nobody else has: ahead of the upstream on every branch.
    pub fn unpushed_commits(&self) -> u64 {
        self.unpushed.iter().map(|b| b.ahead).sum()
    }
}

pub fn tilde(home: &Path, path: &Path) -> String {
    let real = std::fs::canonicalize(home).ok();
    for base in std::iter::once(home).chain(real.as_deref()) {
        match path.strip_prefix(base) {
            Ok(rest) if rest.as_os_str().is_empty() => return "~".to_string(),
            Ok(rest) => return format!("~/{}", rest.to_string_lossy()),
            Err(_) => {}
        }
    }
    path.to_string_lossy().into_owned()
}

pub fn untilde(home: &Path, text: &str) -> PathBuf {
    match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if text == "~" => home.to_path_buf(),
        None => PathBuf::from(text),
    }
}

/// A remote URL as it may be written to a file: no password, no user for an http(s) URL, no query
/// or fragment. An ssh user (`git@host:path`) stays, because the clone needs it.
pub fn clean_url(url: &str) -> String {
    let url = url.trim();
    let without_tail = url.split(['?', '#']).next().unwrap_or(url);
    let Some((scheme, rest)) = without_tail.split_once("://") else {
        return without_tail.to_string();
    };
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let host = match authority.rsplit_once('@') {
        Some((user, host)) if matches!(scheme, "ssh" | "git+ssh") && !user.contains(':') => {
            format!("{user}@{host}")
        }
        Some((_, host)) => host.to_string(),
        None => authority.to_string(),
    };
    if path.is_empty() {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}/{path}")
    }
}

fn run(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = git_command(Some(dir), args);
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    let out = run_command(cmd, GIT_TIMEOUT)?;
    if out.ok() {
        Ok(out.stdout)
    } else {
        Err(out.first_error_line())
    }
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim_end).filter(|l| !l.is_empty())
}

/// The git repositories under `roots`: a root that is one, and the repositories one or two folders
/// below a root that is not. A folder with a `.git` file is a linked worktree and is not a
/// repository of its own.
pub fn find(roots: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, found: &mut BTreeSet<PathBuf>) {
        let git = dir.join(".git");
        if git.is_dir() {
            found.insert(dir.to_path_buf());
            return;
        }
        if git.exists() || depth == 0 {
            return;
        }
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_str()) {
                continue;
            }
            let path = entry.path();
            if path.is_dir() && !entry.file_type().is_ok_and(|t| t.is_symlink()) {
                walk(&path, depth - 1, found);
            }
        }
    }
    let mut found = BTreeSet::new();
    for root in roots {
        walk(root, FIND_DEPTH, &mut found);
    }
    found.into_iter().collect()
}

fn read_remotes(dir: &Path) -> Vec<Remote> {
    let Ok(text) = run(dir, &["remote", "-v"]) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    lines(&text)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (name, url, kind) = (parts.next()?, parts.next()?, parts.next()?);
            (kind == "(fetch)" && seen.insert(name.to_string())).then(|| Remote {
                name: name.into(),
                url: clean_url(url),
            })
        })
        .collect()
}

fn read_branches(dir: &Path, has_remote: bool) -> (Vec<BranchAhead>, Vec<String>) {
    let Ok(text) = run(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname:short)%09%(upstream:short)%09%(upstream:track,nobracket)",
            "refs/heads",
        ],
    ) else {
        return (Vec::new(), Vec::new());
    };
    let mut unpushed = Vec::new();
    let mut never = Vec::new();
    for line in lines(&text) {
        let mut parts = line.split('\t');
        let Some(branch) = parts.next() else {
            continue;
        };
        let upstream = parts.next().unwrap_or("");
        let track = parts.next().unwrap_or("");
        if upstream.is_empty() {
            if has_remote
                && run(dir, &["rev-list", "--count", branch, "--not", "--remotes"])
                    .ok()
                    .and_then(|n| n.trim().parse::<u64>().ok())
                    .is_some_and(|n| n > 0)
            {
                never.push(branch.to_string());
            }
            continue;
        }
        let ahead = track
            .split(',')
            .find_map(|part| part.trim().strip_prefix("ahead ")?.parse::<u64>().ok())
            .unwrap_or(0);
        if ahead > 0 {
            unpushed.push(BranchAhead {
                branch: branch.to_string(),
                ahead,
            });
        }
    }
    (unpushed, never)
}

fn read_worktrees(dir: &Path, home: &Path) -> Vec<Worktree> {
    let Ok(text) = run(dir, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for block in text.split("\n\n").skip(1) {
        let mut tree = Worktree::default();
        for line in block.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                tree.path = tilde(home, Path::new(path));
            } else if let Some(head) = line.strip_prefix("HEAD ") {
                tree.head = Some(head.to_string());
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                tree.branch = Some(branch.to_string());
            }
        }
        if !tree.path.is_empty() {
            out.push(tree);
        }
    }
    out
}

fn read_submodules(dir: &Path) -> Vec<Submodule> {
    if !dir.join(".gitmodules").is_file() {
        return Vec::new();
    }
    let Ok(text) = run(
        dir,
        &[
            "config",
            "--file",
            ".gitmodules",
            "--get-regexp",
            r"^submodule\..*\.(path|url)$",
        ],
    ) else {
        return Vec::new();
    };
    let mut by_name: std::collections::BTreeMap<String, Submodule> = Default::default();
    for line in lines(&text) {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let Some(rest) = key.strip_prefix("submodule.") else {
            continue;
        };
        if let Some(name) = rest.strip_suffix(".path") {
            by_name.entry(name.into()).or_default().path = value.to_string();
        } else if let Some(name) = rest.strip_suffix(".url") {
            by_name.entry(name.into()).or_default().url = clean_url(value);
        }
    }
    by_name
        .into_values()
        .filter(|s| !s.path.is_empty())
        .map(|mut s| {
            s.commit = run(dir, &["ls-tree", "HEAD", "--", &s.path])
                .ok()
                .and_then(|t| {
                    let mut fields = t.split_whitespace();
                    (fields.next() == Some("160000")).then(|| fields.nth(1).map(String::from))?
                });
            s
        })
        .collect()
}

fn read_ignored_names(dir: &Path) -> Vec<String> {
    let Ok(text) = run(
        dir,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ],
    ) else {
        return Vec::new();
    };
    let mut names: Vec<String> = lines(&text)
        .filter(|name| !name.ends_with('/') && scan::secret_file_name(name))
        .map(String::from)
        .collect();
    names.sort();
    names
}

/// Reads one repository. A failure of the first git call becomes `error` and the rest is left empty.
pub fn read(dir: &Path, home: &Path, with_ignored_names: bool) -> Repo {
    let mut repo = Repo {
        name: dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        path: tilde(home, dir),
        abs: dir.to_path_buf(),
        ..Repo::default()
    };
    if let Err(why) = run(dir, &["rev-parse", "--git-dir"]) {
        repo.error = Some(if why.is_empty() {
            "git could not read it".into()
        } else {
            why
        });
        return repo;
    }
    repo.remotes = read_remotes(dir);
    repo.no_remote = repo.remotes.is_empty();
    repo.branch = run(dir, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty());
    repo.head = run(dir, &["rev-parse", "--verify", "-q", "HEAD"])
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty());
    repo.upstream = run(
        dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
    .map(|u| u.trim().to_string())
    .filter(|u| !u.is_empty());
    if let Some(upstream) = &repo.upstream {
        if let Some((ahead, behind)) = run(
            dir,
            &[
                "rev-list",
                "--left-right",
                "--count",
                &format!("HEAD...{upstream}"),
            ],
        )
        .ok()
        .and_then(|text| {
            let mut nums = text.split_whitespace();
            Some((nums.next()?.parse().ok()?, nums.next()?.parse().ok()?))
        }) {
            repo.ahead = ahead;
            repo.behind = behind;
        }
    }
    repo.default_branch = run(
        dir,
        &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .and_then(|b| b.trim().strip_prefix("origin/").map(String::from));
    if let Ok(status) = run(
        dir,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    ) {
        for line in lines(&status) {
            if line.starts_with("??") {
                repo.untracked += 1;
            } else if !line.starts_with("!!") {
                repo.dirty += 1;
            }
        }
    }
    repo.stashes = run(dir, &["stash", "list"]).map_or(0, |t| lines(&t).count() as u64);
    (repo.unpushed, repo.never_pushed) = read_branches(dir, !repo.no_remote);
    repo.worktrees = read_worktrees(dir, home);
    repo.submodules = read_submodules(dir);
    if with_ignored_names {
        repo.ignored_names = read_ignored_names(dir);
    }
    repo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_lose_passwords_queries_and_http_users_but_keep_an_ssh_user() {
        for (given, kept) in [
            (
                "https://user:pw@github.com/acme/web.git",
                "https://github.com/acme/web.git",
            ),
            (
                "https://github.com/acme/web.git?token=abc#frag",
                "https://github.com/acme/web.git",
            ),
            ("git@github.com:acme/web.git", "git@github.com:acme/web.git"),
            (
                "ssh://git@host.example/acme/web.git",
                "ssh://git@host.example/acme/web.git",
            ),
            (
                "ssh://deploy:secret@host.example/a.git",
                "ssh://host.example/a.git",
            ),
            ("/srv/git/web.git", "/srv/git/web.git"),
        ] {
            assert_eq!(clean_url(given), kept, "{given}");
        }
    }

    #[test]
    fn paths_go_home_relative_and_come_back() {
        let home = Path::new("/Users/me");
        assert_eq!(tilde(home, Path::new("/Users/me/code/a")), "~/code/a");
        assert_eq!(tilde(home, Path::new("/opt/a")), "/opt/a");
        assert_eq!(
            untilde(Path::new("/Users/you"), "~/code/a"),
            Path::new("/Users/you/code/a")
        );
    }
}
