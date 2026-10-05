#![allow(dead_code)]

//! MIG-UPD-4: a working checkout wired with three git remotes -- `upstream`, `fork` (standing in
//! for the user's own origin) and `local`, a third bystander clone -- for testing branch/remote/
//! upstream detection and drift. Everything is built from real `git` commands into a tempdir;
//! nothing here is a binary fixture committed to the tree.
//!
//! Shared by the lib tests (`#[path]`) the same way `tests/common/sources_world.rs` is.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct ThreeRemotes {
    pub base: PathBuf,
    /// Remotes `fork` (tracked, branch `main`), `upstream` (one commit ahead, not yet merged)
    /// and `local` (a stale mirror at the shared base commit). HEAD carries a commit neither
    /// remote has (a fork is usually ahead of upstream by its own work, not only behind). A
    /// local branch `feature-x` tracks `fork/feature-x` but is not checked out.
    pub work: PathBuf,
    /// A second clone of the same `fork`, with only `fork` (tracked) and `local` registered --
    /// no remote literally named `upstream` -- to exercise the fallback half of the heuristic.
    pub work_local_only: PathBuf,
    /// A linked worktree of `work` on a fresh local branch with no upstream configured.
    pub worktree: PathBuf,
    pub upstream: PathBuf,
    pub fork: PathBuf,
    pub local: PathBuf,
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(["-c", "protocol.file.allow=always"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", &format!("add {file}")]);
}

fn bare_clone_of(src: &Path, dest: &Path) {
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    git(src, &["clone", "-q", "--bare", ".", dest.to_str().unwrap()]);
}

pub fn build(base: &Path) -> ThreeRemotes {
    let _ = std::fs::remove_dir_all(base);
    std::fs::create_dir_all(base).unwrap();

    let seed = base.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "-q"]);
    commit(&seed, "README.md", "base\n");

    let remotes = base.join("remotes");
    let upstream = remotes.join("upstream.git");
    let fork = remotes.join("fork.git");
    let local = remotes.join("local.git");
    bare_clone_of(&seed, &upstream);
    bare_clone_of(&seed, &fork);
    bare_clone_of(&seed, &local);

    // upstream moves on with a commit the fork has not merged yet.
    let upstream_work = base.join("upstream-work");
    git(
        &remotes,
        &[
            "clone",
            "-q",
            upstream.to_str().unwrap(),
            upstream_work.to_str().unwrap(),
        ],
    );
    commit(&upstream_work, "UPSTREAM.md", "upstream work\n");
    git(&upstream_work, &["push", "-q", "origin", "HEAD:main"]);

    // the fork gets its own commit (ahead of the shared base, as a fork normally is), plus a
    // second branch to prove branch-fill reads the real checkout, not origin's default.
    let fork_work = base.join("fork-work");
    git(
        &remotes,
        &["clone", "-q", fork.to_str().unwrap(), fork_work.to_str().unwrap()],
    );
    commit(&fork_work, "FORK.md", "fork work\n");
    git(&fork_work, &["push", "-q", "origin", "HEAD:main"]);
    git(&fork_work, &["checkout", "-q", "-b", "feature-x"]);
    commit(&fork_work, "FEATURE.md", "feature work\n");
    git(&fork_work, &["push", "-q", "origin", "HEAD:feature-x"]);

    // the server's actual checkout: cloned from the fork (picking up main and feature-x), the
    // default remote renamed to `fork`, then `upstream` and `local` added and fetched.
    let work = base.join("work");
    git(
        &remotes,
        &["clone", "-q", fork.to_str().unwrap(), work.to_str().unwrap()],
    );
    git(&work, &["remote", "rename", "origin", "fork"]);
    git(&work, &["remote", "add", "upstream", upstream.to_str().unwrap()]);
    git(&work, &["fetch", "-q", "upstream"]);
    git(&work, &["remote", "add", "local", local.to_str().unwrap()]);
    git(&work, &["fetch", "-q", "local"]);
    git(
        &work,
        &["checkout", "-q", "-b", "feature-x", "--track", "fork/feature-x"],
    );
    git(&work, &["checkout", "-q", "main"]);

    let work_local_only = base.join("work-local-only");
    git(
        &remotes,
        &[
            "clone",
            "-q",
            fork.to_str().unwrap(),
            work_local_only.to_str().unwrap(),
        ],
    );
    git(&work_local_only, &["remote", "rename", "origin", "fork"]);
    git(&work_local_only, &["remote", "add", "local", local.to_str().unwrap()]);
    git(&work_local_only, &["fetch", "-q", "local"]);

    let worktree = base.join("work-wt");
    git(
        &work,
        &["worktree", "add", "-q", "-b", "wt-branch", worktree.to_str().unwrap()],
    );

    ThreeRemotes {
        base: base.to_path_buf(),
        work,
        work_local_only,
        worktree,
        upstream,
        fork,
        local,
    }
}
