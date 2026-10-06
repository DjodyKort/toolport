//! MIG-UPD-5: a fork's checkout is compared against the remote branch it follows (a normal,
//! fast-forwardable update) and, separately, against `upstream/branch`; being ahead of upstream
//! is the fork's normal state and every configured remote is fetched.

use super::exec::{CmdOutput, GitRunner};
use super::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

fn git(dir: &Path, args: &[&str]) -> String {
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

fn commit(dir: &Path, file: &str) {
    std::fs::write(dir.join(file), file).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", &format!("add {file}")]);
}

struct World {
    base: PathBuf,
    work: PathBuf,
    fork_pusher: PathBuf,
    upstream_pusher: PathBuf,
}

impl World {
    /// `work` is cloned from the fork (`origin`) with `upstream` added; the fork already carries
    /// one commit of its own that upstream lacks, so the checkout starts ahead of upstream.
    fn new(tag: &str) -> World {
        let base = std::env::temp_dir().join(format!("plus-fork-status-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let seed = base.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init", "-q"]);
        commit(&seed, "README.md");
        let bare = |name: &str| {
            let dest = base.join(name);
            git(&seed, &["clone", "-q", "--bare", ".", dest.to_str().unwrap()]);
            dest
        };
        let upstream = bare("upstream.git");
        let fork = bare("fork.git");
        let clone = |src: &Path, name: &str| {
            let dest = base.join(name);
            git(&base, &["clone", "-q", src.to_str().unwrap(), dest.to_str().unwrap()]);
            dest
        };
        let fork_pusher = clone(&fork, "fork-pusher");
        let upstream_pusher = clone(&upstream, "upstream-pusher");
        commit(&fork_pusher, "FORK-OWN.md");
        git(&fork_pusher, &["push", "-q", "origin", "HEAD:main"]);
        let work = clone(&fork, "work");
        git(&work, &["remote", "add", "upstream", upstream.to_str().unwrap()]);
        git(&work, &["fetch", "-q", "upstream"]);
        World {
            base,
            work,
            fork_pusher,
            upstream_pusher,
        }
    }

    fn push_fork(&self, file: &str, branch: &str) {
        commit(&self.fork_pusher, file);
        git(&self.fork_pusher, &["push", "-q", "origin", &format!("HEAD:{branch}")]);
    }

    fn push_upstream(&self, file: &str) {
        commit(&self.upstream_pusher, file);
        git(&self.upstream_pusher, &["push", "-q", "origin", "HEAD:main"]);
    }

    fn entry(&self, meta: Option<Value>) -> ServerEntry {
        let mut v = json!({
            "id": "g", "name": "g", "transport": "stdio",
            "command": "node", "args": ["x.js"], "cwd": self.work.to_str().unwrap(),
        });
        if let Some(meta) = meta {
            v["mcpmSource"] = meta;
        }
        serde_json::from_value(v).unwrap()
    }

    fn env(&self) -> Env {
        let mut env = Env::system();
        env.home = Some(self.base.clone());
        env
    }

    fn check(&self, entry: ServerEntry) -> ServerReport {
        let mut entries = vec![entry];
        run(&self.env(), &mut entries, &Options::new(Mode::Check))
            .unwrap()
            .servers
            .remove(0)
    }

    fn stored(&self, remote: &str, branch: &str, upstream: Option<(&str, &str)>) -> ServerEntry {
        let mut meta = json!({
            "type": "git", "path": self.work.to_str().unwrap(),
            "remote": remote, "branch": branch,
        });
        if let Some((r, b)) = upstream {
            meta["upstream"] = json!({"remote": r, "branch": b});
        }
        self.entry(Some(meta))
    }

    fn stored_fork(&self) -> ServerEntry {
        self.stored("origin", "main", Some(("upstream", "main")))
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn upstream_of(rep: &ServerReport) -> (u32, u32) {
    let u = rep.upstream.as_ref().expect("upstream report");
    (u.behind, u.ahead)
}

#[test]
fn behind_the_fork_only_is_a_normal_update_and_upstream_is_level() {
    let w = World::new("fork-only");
    w.push_fork("FORK-NEW.md", "main");
    let rep = w.check(w.stored_fork());
    assert_eq!(rep.status, Status::UpdateAvailable, "{}", rep.message);
    assert_eq!((rep.behind, rep.ahead), (Some(1), Some(0)));
    assert_eq!(rep.latest.as_deref(), Some("origin/main"));
    assert_eq!(upstream_of(&rep), (0, 1), "the fork's own commit sits on top of upstream");
    assert!(rep.message.contains("1 commit(s) behind origin/main"), "{}", rep.message);
    assert!(rep.message.contains("contains all of upstream/main"), "{}", rep.message);
    assert!(rep.plan.iter().any(|p| p.contains("merge --ff-only origin/main")));
}

#[test]
fn behind_upstream_only_leaves_the_checkout_up_to_date_with_its_fork() {
    let w = World::new("upstream-only");
    w.push_upstream("UPSTREAM-1.md");
    w.push_upstream("UPSTREAM-2.md");
    let rep = w.check(w.stored_fork());
    assert_eq!(rep.status, Status::UpToDate, "{}", rep.message);
    assert_eq!((rep.behind, rep.ahead), (Some(0), Some(0)));
    let up = rep.upstream.as_ref().unwrap();
    assert_eq!(up.remote_ref, "upstream/main");
    assert_eq!((up.behind, up.ahead), (2, 1));
    assert_eq!(up.summaries.len(), 2);
    assert!(
        rep.message.contains("2 commit(s) behind upstream/main (1 own commit(s) on top)"),
        "{}",
        rep.message
    );
}

#[test]
fn behind_both_reports_two_independent_numbers() {
    let w = World::new("both");
    w.push_fork("FORK-NEW.md", "main");
    w.push_upstream("UPSTREAM-1.md");
    let rep = w.check(w.stored_fork());
    assert_eq!(rep.status, Status::UpdateAvailable, "{}", rep.message);
    assert_eq!((rep.behind, rep.ahead), (Some(1), Some(0)));
    assert_eq!(upstream_of(&rep), (1, 1));
    assert!(rep.message.contains("1 commit(s) behind origin/main"), "{}", rep.message);
    assert!(rep.message.contains("1 commit(s) behind upstream/main"), "{}", rep.message);
}

#[test]
fn ahead_only_fork_is_not_divergence() {
    let w = World::new("ahead-only");
    commit(&w.work, "LOCAL-ONLY.md");
    git(&w.work, &["push", "-q", "origin", "HEAD:main"]);
    let rep = w.check(w.stored_fork());
    assert_eq!(rep.status, Status::UpToDate, "{}", rep.message);
    assert!(!rep.message.contains("diverged"), "{}", rep.message);
    assert_eq!((rep.behind, rep.ahead), (Some(0), Some(0)));
    assert_eq!(upstream_of(&rep), (0, 2));
}

#[test]
fn unpushed_commits_over_a_clean_remote_are_not_reported_as_divergence() {
    let w = World::new("unpushed");
    commit(&w.work, "LOCAL-ONLY.md");
    let rep = w.check(w.stored_fork());
    assert_eq!(rep.status, Status::UpToDate, "{}", rep.message);
    assert_eq!((rep.behind, rep.ahead), (Some(0), Some(1)));
    assert_eq!(upstream_of(&rep), (0, 2));
}

#[test]
fn a_real_divergence_from_the_followed_branch_is_still_skipped() {
    let w = World::new("diverged");
    w.push_fork("FORK-NEW.md", "main");
    commit(&w.work, "LOCAL-ONLY.md");
    let rep = w.check(w.stored_fork());
    assert_eq!(rep.status, Status::Skipped, "{}", rep.message);
    assert!(rep.message.contains("diverged"), "{}", rep.message);
}

/// The Mac's google-docs checkout: on `djody/custom-tools`, tracking `upstream/main`, while the
/// fork's own branch on `origin` has moved on by three commits.
fn google_docs_shape(tag: &str) -> World {
    let w = World::new(tag);
    git(&w.work, &["checkout", "-q", "-b", "djody/custom-tools", "--track", "upstream/main"]);
    commit(&w.work, "CUSTOM-1.md");
    git(&w.work, &["push", "-q", "origin", "HEAD:djody/custom-tools"]);
    git(&w.work, &["fetch", "-q", "origin"]);
    git(&w.fork_pusher, &["fetch", "-q", "origin"]);
    git(
        &w.fork_pusher,
        &["checkout", "-q", "-B", "djody/custom-tools", "origin/djody/custom-tools"],
    );
    for n in 0..3 {
        w.push_fork(&format!("CUSTOM-NEW-{n}.md"), "djody/custom-tools");
    }
    w
}

#[test]
fn google_docs_shape_stored_meta_reports_the_fork_branch_it_is_behind() {
    let w = google_docs_shape("gd-stored");
    let rep = w.check(w.stored("origin", "djody/custom-tools", Some(("upstream", "main"))));
    assert_eq!(rep.status, Status::UpdateAvailable, "{}", rep.message);
    assert_eq!(rep.latest.as_deref(), Some("origin/djody/custom-tools"));
    assert_eq!((rep.behind, rep.ahead), (Some(3), Some(0)));
    assert_eq!(rep.current.as_deref(), Some("djody/custom-tools"));
    assert_eq!(upstream_of(&rep), (0, 1), "own work on top of upstream, nothing missing");
}

#[test]
fn google_docs_shape_detection_follows_the_fork_branch_not_the_tracked_upstream() {
    let w = google_docs_shape("gd-detected");
    w.push_upstream("UPSTREAM-1.md");
    let rep = w.check(w.entry(None));
    assert_eq!(rep.kind, "git");
    assert_eq!(rep.status, Status::UpdateAvailable, "{}", rep.message);
    assert_eq!(rep.latest.as_deref(), Some("origin/djody/custom-tools"));
    assert_eq!(rep.behind, Some(3));
    let up = rep.upstream.as_ref().expect("upstream report");
    assert_eq!(up.remote_ref, "upstream/main");
    assert_eq!(up.behind, 1);
}

#[test]
fn applying_the_fork_update_fast_forwards_without_touching_upstream_commits() {
    let w = google_docs_shape("gd-apply");
    w.push_upstream("UPSTREAM-1.md");
    let env = w.env();
    let mut entries = vec![w.stored("origin", "djody/custom-tools", Some(("upstream", "main")))];
    let done = run(&env, &mut entries, &Options::new(Mode::Apply)).unwrap();
    assert_eq!(done.servers[0].status, Status::Updated, "{}", done.servers[0].message);
    assert!(w.work.join("CUSTOM-NEW-2.md").exists());
    assert!(!w.work.join("UPSTREAM-1.md").exists());
    let again = run(&env, &mut entries, &Options::new(Mode::Check)).unwrap();
    assert_eq!(again.servers[0].status, Status::UpToDate);
    assert_eq!(upstream_of(&again.servers[0]), (1, 4));
}

struct FetchSpy(Arc<Mutex<Vec<String>>>);

impl GitRunner for FetchSpy {
    fn git(&self, repo: &Path, args: &[&str], t: std::time::Duration) -> Result<CmdOutput, String> {
        if args.first() == Some(&"fetch") {
            self.0.lock().unwrap().push(args.last().unwrap().to_string());
        }
        exec::SystemGit.git(repo, args, t)
    }
}

#[test]
fn every_configured_remote_is_fetched() {
    let w = World::new("fetch-all");
    let third = w.base.join("third.git");
    git(&w.work, &["clone", "-q", "--bare", ".", third.to_str().unwrap()]);
    git(&w.work, &["remote", "add", "mirror", third.to_str().unwrap()]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let status = gitops::check_target(
        &FetchSpy(seen.clone()),
        &w.work,
        &gitops::Target {
            remote: Some("origin"),
            branch: Some("main"),
            upstream: Some(("upstream", "main")),
        },
    )
    .unwrap();
    let mut fetched = seen.lock().unwrap().clone();
    fetched.sort();
    assert_eq!(fetched, ["mirror", "origin", "upstream"]);
    assert!(status.warnings.is_empty(), "{:?}", status.warnings);
}

#[test]
fn an_unreachable_upstream_does_not_fail_the_check_and_is_named_in_the_message() {
    let w = World::new("upstream-down");
    let gone = w.base.join("gone.git");
    git(&w.work, &["remote", "set-url", "upstream", gone.to_str().unwrap()]);
    let rep = w.check(w.stored_fork());
    assert_eq!(rep.status, Status::UpToDate, "{}", rep.message);
    assert!(rep.upstream.is_none());
    assert!(rep.message.contains("upstream: git fetch failed"), "{}", rep.message);
}

#[test]
fn a_failing_followed_remote_is_still_an_error() {
    let w = World::new("origin-down");
    let gone = w.base.join("gone.git");
    git(&w.work, &["remote", "set-url", "origin", gone.to_str().unwrap()]);
    let rep = w.check(w.stored_fork());
    assert_eq!(rep.status, Status::Error, "{}", rep.message);
}

#[test]
fn a_stored_remote_other_than_origin_is_the_one_compared_against() {
    let w = World::new("stored-remote");
    git(&w.work, &["remote", "rename", "origin", "djody"]);
    w.push_fork("FORK-NEW.md", "main");
    let rep = w.check(w.stored("djody", "main", Some(("upstream", "main"))));
    assert_eq!(rep.latest.as_deref(), Some("djody/main"), "{}", rep.message);
    assert_eq!(rep.behind, Some(1));
}

#[test]
fn legacy_metadata_without_a_branch_still_uses_the_tracking_branch() {
    let w = World::new("legacy");
    w.push_fork("FORK-NEW.md", "main");
    let rep = w.check(w.entry(Some(json!({"type": "git", "path": w.work.to_str().unwrap()}))));
    assert_eq!(rep.latest.as_deref(), Some("origin/main"));
    assert_eq!(rep.behind, Some(1));
    assert!(rep.upstream.is_none());
}
