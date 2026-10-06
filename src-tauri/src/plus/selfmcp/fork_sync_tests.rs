use crate::plus::selfmcp::state_tests::{call, kind};
use crate::plus::selfmcp::tests::Fixture;
use crate::plus::selfmcp::wired_tests::{git, git_identity};
use crate::plus::selfmcp::ToolError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct Fork {
    dir: PathBuf,
    work: PathBuf,
    fork_remote: PathBuf,
    other: PathBuf,
}

impl Fork {
    fn new(fixture: &Fixture, local: (&str, &str), upstream: (&str, &str)) -> Self {
        let dir = fixture.dir.clone();
        let upstream_remote = dir.join("upstream.git");
        let fork_remote = dir.join("fork.git");
        for bare in [&upstream_remote, &fork_remote] {
            std::fs::create_dir_all(bare).unwrap();
            git(bare, &["init", "-q", "--bare", "-b", "main"]);
        }
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git_identity(&work);
        std::fs::write(work.join("a.txt"), "a\n").unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-q", "-m", "base"]);
        git(&work, &["remote", "add", "origin", fork_remote.to_str().unwrap()]);
        git(&work, &["remote", "add", "upstream", upstream_remote.to_str().unwrap()]);
        git(&work, &["push", "-q", "-u", "origin", "main"]);
        git(&work, &["push", "-q", "upstream", "main"]);
        std::fs::write(work.join(local.0), local.1).unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-q", "-m", "local change"]);

        let other = dir.join("other");
        git(
            &dir,
            &["clone", "-q", upstream_remote.to_str().unwrap(), other.to_str().unwrap()],
        );
        git_identity(&other);
        std::fs::write(other.join(upstream.0), upstream.1).unwrap();
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-q", "-m", "upstream change"]);
        git(&other, &["push", "-q", "origin", "main"]);

        let registry_path = dir.join("registry.json");
        let mut reg: Value =
            serde_json::from_str(&std::fs::read_to_string(&registry_path).unwrap()).unwrap();
        reg["servers"].as_array_mut().unwrap().push(json!({
            "id": "srv-fork", "name": "forked", "transport": "stdio", "command": "forked-mcp",
            "args": [],
            "mcpmSource": {"type": "git", "path": work.to_string_lossy(), "branch": "main"}
        }));
        std::fs::write(&registry_path, reg.to_string()).unwrap();
        Self { dir, work, fork_remote, other }
    }

    fn clean(fixture: &Fixture) -> Self {
        Self::new(fixture, ("local.txt", "mine\n"), ("b.txt", "b\n"))
    }

    fn conflicting(fixture: &Fixture) -> Self {
        Self::new(fixture, ("a.txt", "mine\n"), ("a.txt", "theirs\n"))
    }

    fn sync(&self, extra: Value) -> Result<Value, ToolError> {
        let mut args = json!({"name": "forked", "confirm": true});
        for (key, value) in extra.as_object().unwrap() {
            args[key] = value.clone();
        }
        call("servers_fork_sync", args)
    }

    fn branches(&self) -> String {
        git(&self.work, &["branch", "--format=%(refname:short)"])
    }

    fn worktrees(&self) -> usize {
        git(&self.work, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count()
    }

    fn remote_main(&self) -> String {
        git(&self.fork_remote, &["rev-parse", "refs/heads/main"])
    }
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn a_clean_sync_updates_the_tracked_branch_without_new_branch_names() {
    let fixture = Fixture::new("fork-clean");
    let fork = Fork::clean(&fixture);
    let before = head(&fork.work);

    assert_eq!(kind(call("servers_fork_sync", json!({"name": "forked"}))), "refused");
    assert_eq!(head(&fork.work), before);

    let done = fork.sync(json!({})).unwrap();
    assert_eq!(done["synced"], true);
    assert_eq!(done["branch"], "main");
    assert_eq!(done["mode"], "rebase");
    assert_eq!(done["upstream"], "upstream/main");
    assert_eq!(done["head"], head(&fork.work));
    assert_ne!(head(&fork.work), before);
    assert_eq!(git(&fork.work, &["branch", "--show-current"]), "main");
    assert!(fork.work.join("b.txt").exists());
    assert!(fork.work.join("local.txt").exists());
    assert_eq!(fork.branches(), "main");
    assert_eq!(fork.worktrees(), 1);
    assert_eq!(done["staleBranches"], json!([]));
    assert!(done.get("push").is_none());
    assert_eq!(fork.sync(json!({})).unwrap()["upToDate"], true);
}

#[test]
fn a_conflict_keeps_the_worktree_and_leaves_the_live_checkout_untouched() {
    let fixture = Fixture::new("fork-conflict");
    let fork = Fork::conflicting(&fixture);
    let before = head(&fork.work);

    let stopped = fork.sync(json!({})).unwrap();
    assert_eq!(stopped["synced"], false);
    assert_eq!(stopped["conflict"], true);
    assert_eq!(stopped["branch"], "main");
    assert_eq!(stopped["conflictedPaths"], json!(["a.txt"]));
    let worktree = PathBuf::from(stopped["worktree"].as_str().unwrap());
    assert!(worktree.join("a.txt").exists());
    assert!(worktree.starts_with(&fork.work));
    assert_eq!(head(&fork.work), before);
    assert_eq!(git(&fork.work, &["status", "--porcelain"]), "");
    assert_eq!(std::fs::read_to_string(fork.work.join("a.txt")).unwrap(), "mine\n");
    assert_eq!(git(&fork.work, &["branch", "--show-current"]), "main");
    assert_eq!(fork.branches(), "main");
    assert_eq!(fork.worktrees(), 2);
}

#[test]
fn push_is_opt_in_and_goes_to_the_fork_remote() {
    let fixture = Fixture::new("fork-push");
    let fork = Fork::clean(&fixture);
    let remote_before = fork.remote_main();

    let local = fork.sync(json!({})).unwrap();
    assert_eq!(local["synced"], true);
    assert_eq!(fork.remote_main(), remote_before);

    git(&fork.other, &["pull", "-q", "origin", "main"]);
    std::fs::write(fork.other.join("c.txt"), "c\n").unwrap();
    git(&fork.other, &["add", "-A"]);
    git(&fork.other, &["commit", "-q", "-m", "more upstream"]);
    git(&fork.other, &["push", "-q", "origin", "main"]);

    let pushed = fork.sync(json!({"push": true})).unwrap();
    assert_eq!(pushed["synced"], true);
    assert_eq!(pushed["push"], json!({"pushed": true, "remote": "origin"}));
    assert_eq!(fork.remote_main(), head(&fork.work));
    assert_ne!(fork.remote_main(), remote_before);
    assert_eq!(fork.branches(), "main");
}

#[test]
fn merge_mode_keeps_history_and_the_stored_upstream_is_the_default() {
    let fixture = Fixture::new("fork-merge");
    let fork = Fork::clean(&fixture);
    git(&fork.work, &["remote", "rename", "upstream", "vendor"]);
    let registry_path = fork.dir.join("registry.json");
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&registry_path).unwrap()).unwrap();
    reg["servers"][2]["mcpmSource"]["upstream"] = json!({"remote": "vendor", "branch": "main"});
    std::fs::write(&registry_path, reg.to_string()).unwrap();

    let done = fork.sync(json!({"mode": "merge"})).unwrap();
    assert_eq!(done["synced"], true);
    assert_eq!(done["upstream"], "vendor/main");
    assert_eq!(git(&fork.work, &["rev-list", "--parents", "-n1", "HEAD"]).split(' ').count(), 3);
    assert!(fork.work.join("b.txt").exists());
    assert_eq!(fork.branches(), "main");
}

#[test]
fn onto_author_replays_one_authors_commits_in_the_worktree() {
    let fixture = Fixture::new("fork-author");
    let fork = Fork::clean(&fixture);
    let done = fork
        .sync(json!({"mode": "onto-author", "author_email": "tester@example.invalid"}))
        .unwrap();
    assert_eq!(done["synced"], true);
    assert_eq!(done["picked"], 1);
    assert!(fork.work.join("b.txt").exists());
    assert!(fork.work.join("local.txt").exists());
    assert_eq!(fork.branches(), "main");
    assert_eq!(fork.worktrees(), 1);
}

#[test]
fn old_synced_branches_are_listed_and_never_deleted() {
    let fixture = Fixture::new("fork-stale");
    let fork = Fork::clean(&fixture);
    git(&fork.work, &["branch", "main-synced-20260101"]);
    let done = fork.sync(json!({})).unwrap();
    assert_eq!(done["staleBranches"], json!(["main-synced-20260101"]));
    assert!(fork.branches().contains("main-synced-20260101"));
}

#[test]
fn a_dirty_tracked_checkout_refuses_but_another_branch_may_be_dirty() {
    let fixture = Fixture::new("fork-dirty");
    let fork = Fork::clean(&fixture);
    std::fs::write(fork.work.join("a.txt"), "edited\n").unwrap();
    assert_eq!(kind(fork.sync(json!({}))), "conflict");

    git(&fork.work, &["checkout", "-q", "--", "a.txt"]);
    git(&fork.work, &["checkout", "-q", "-b", "scratch"]);
    std::fs::write(fork.work.join("a.txt"), "edited\n").unwrap();
    let main_before = git(&fork.work, &["rev-parse", "main"]);
    let done = fork.sync(json!({})).unwrap();
    assert_eq!(done["synced"], true);
    assert_ne!(git(&fork.work, &["rev-parse", "main"]), main_before);
    assert_eq!(git(&fork.work, &["branch", "--show-current"]), "scratch");
    assert_eq!(std::fs::read_to_string(fork.work.join("a.txt")).unwrap(), "edited\n");
}

#[test]
fn a_failing_update_command_keeps_the_worktree_and_the_branch() {
    let fixture = Fixture::new("fork-post");
    let fork = Fork::clean(&fixture);
    let registry_path = fork.dir.join("registry.json");
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&registry_path).unwrap()).unwrap();
    reg["servers"][2]["mcpmSource"]["post_update"] = json!("exit 3");
    std::fs::write(&registry_path, reg.to_string()).unwrap();
    let before = head(&fork.work);

    let stopped = fork.sync(json!({"run_post_update": true})).unwrap();
    assert_eq!(stopped["synced"], false);
    assert_eq!(stopped["conflict"], false);
    assert_eq!(stopped["postUpdate"]["ok"], false);
    assert!(Path::new(stopped["worktree"].as_str().unwrap()).exists());
    assert_eq!(head(&fork.work), before);
}
