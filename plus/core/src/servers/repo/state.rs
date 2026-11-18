//! What git says about one checkout: the branch, where it is headed, how far apart the two are
//! and whether the work tree is clean. Reads only; the network is never touched here.

use std::path::{Path, PathBuf};

use crate::library::remote::git_out;
use crate::skills::taps::redact;

const OPERATION_MARKERS: [(&str, &str); 5] = [
    ("MERGE_HEAD", "a merge"),
    ("rebase-merge", "a rebase"),
    ("rebase-apply", "a rebase"),
    ("CHERRY_PICK_HEAD", "a cherry-pick"),
    ("REVERT_HEAD", "a revert"),
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoState {
    pub root: PathBuf,
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    pub head: Option<String>,
    pub remote: Option<String>,
    /// The remote's URL with any credential removed.
    pub url: Option<String>,
    /// The tracked branch as `<remote>/<branch>`.
    pub upstream: Option<String>,
    /// The tracked branch's commit as this clone last saw it.
    pub upstream_head: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    /// Tracked files with changes, staged or not; untracked files do not count.
    pub dirty: u64,
    /// A merge, rebase, cherry-pick or revert that is still open.
    pub busy: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    UpToDate,
    Behind,
    /// Behind, and tracked files have local changes.
    Dirty,
    Diverged,
    Detached,
    NoUpstream,
    Busy,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::UpToDate => "up_to_date",
            Verdict::Behind => "behind",
            Verdict::Dirty => "dirty",
            Verdict::Diverged => "diverged",
            Verdict::Detached => "detached",
            Verdict::NoUpstream => "no_upstream",
            Verdict::Busy => "busy",
        }
    }
}

fn number(text: Option<String>) -> u64 {
    text.and_then(|t| t.parse().ok()).unwrap_or(0)
}

fn nonempty(text: Option<String>) -> Option<String> {
    text.filter(|t| !t.is_empty())
}

/// The remote the checked-out branch tracks; `None` for a detached HEAD, a branch without one
/// and a branch that tracks another local branch.
pub fn tracked_remote(root: &Path) -> Option<String> {
    let branch = nonempty(git_out(root, &["symbolic-ref", "--short", "-q", "HEAD"]))?;
    nonempty(git_out(
        root,
        &["config", "--get", &format!("branch.{branch}.remote")],
    ))
    .filter(|remote| remote != ".")
}

fn open_operation(root: &Path) -> Option<&'static str> {
    let dir = PathBuf::from(git_out(root, &["rev-parse", "--absolute-git-dir"])?);
    OPERATION_MARKERS
        .iter()
        .find(|(marker, _)| dir.join(marker).exists())
        .map(|(_, what)| *what)
}

pub fn read(root: &Path) -> RepoState {
    let branch = nonempty(git_out(root, &["symbolic-ref", "--short", "-q", "HEAD"]));
    let remote = tracked_remote(root);
    let upstream = remote.as_ref().and_then(|_| {
        nonempty(git_out(
            root,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        ))
    });
    let upstream_head = upstream.as_deref().and_then(|u| {
        nonempty(git_out(
            root,
            &["rev-parse", "--verify", "-q", &format!("{u}^{{commit}}")],
        ))
    });
    let (ahead, behind) = upstream_head
        .as_deref()
        .and_then(|u| {
            let text = git_out(
                root,
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("HEAD...{u}"),
                ],
            )?;
            let mut parts = text.split_whitespace();
            Some((
                number(parts.next().map(String::from)),
                number(parts.next().map(String::from)),
            ))
        })
        .unwrap_or((0, 0));
    let url = remote.as_ref().and_then(|remote| {
        nonempty(git_out(
            root,
            &["config", "--get", &format!("remote.{remote}.url")],
        ))
        .map(|url| redact(&url))
    });
    let dirty = git_out(root, &["status", "--porcelain", "--untracked-files=no"])
        .map_or(0, |text| {
            text.lines().filter(|l| !l.trim().is_empty()).count() as u64
        });
    RepoState {
        root: root.to_path_buf(),
        branch,
        head: nonempty(git_out(root, &["rev-parse", "--verify", "-q", "HEAD"])),
        remote,
        url,
        upstream,
        upstream_head,
        ahead,
        behind,
        dirty,
        busy: open_operation(root),
    }
}

impl RepoState {
    pub fn verdict(&self) -> Verdict {
        if self.branch.is_none() {
            Verdict::Detached
        } else if self.upstream_head.is_none() {
            Verdict::NoUpstream
        } else if self.busy.is_some() {
            Verdict::Busy
        } else if self.behind == 0 {
            Verdict::UpToDate
        } else if self.ahead > 0 {
            Verdict::Diverged
        } else if self.dirty > 0 {
            Verdict::Dirty
        } else {
            Verdict::Behind
        }
    }

    /// The commit ids as 8 characters, for rows and ids.
    pub fn short(commit: Option<&str>) -> String {
        commit.unwrap_or("").chars().take(8).collect()
    }
}
