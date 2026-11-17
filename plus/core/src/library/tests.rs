use super::{pull, remote, sync};
use crate::fsx::{enter, Paths};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn secret() -> String {
    format!("{}{}_{}", "gh", "p", "CANARY0123456789".repeat(3))
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn put(path: PathBuf, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn skill(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill\n---\n# {name}\nBody.\n")
}

struct World {
    base: PathBuf,
    home: PathBuf,
    lib: PathBuf,
    other: PathBuf,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "library-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let bare = base.join("remote.git");
        let other = base.join("other");
        let lib = home.join("lib/skills");
        for dir in [&home, &bare, &other] {
            std::fs::create_dir_all(dir).unwrap();
        }
        git(&bare, &["init", "-q", "--bare", "-b", "main"]);
        git(&other, &["init", "-q", "-b", "main"]);
        put(other.join("skills/review/SKILL.md"), &skill("review"));
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-q", "-m", "Add review"]);
        git(&other, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(&other, &["push", "-q", "-u", "origin", "main"]);
        git(
            &home,
            &["clone", "-q", bare.to_str().unwrap(), lib.to_str().unwrap()],
        );
        put(
            home.join(".config/mcpm/skills_sync.json"),
            &format!("{{\"local_path\": {:?}}}", lib.to_str().unwrap()),
        );
        Self {
            base,
            home,
            lib,
            other,
        }
    }

    fn enter(&self) -> crate::fsx::PathsGuard {
        enter(Paths {
            home: Some(self.home.clone()),
            data_dir: None,
        })
    }

    fn remote_commit(&self, name: &str) {
        put(
            self.other.join("skills").join(name).join("SKILL.md"),
            &skill(name),
        );
        git(&self.other, &["add", "-A"]);
        git(&self.other, &["commit", "-q", "-m", &format!("Add {name}")]);
        git(&self.other, &["push", "-q", "origin", "main"]);
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[test]
fn status_reports_the_clone_and_a_sibling_clone_of_the_same_remote() {
    let world = World::new("status");
    let twin = world.home.join("lib/skills-copy");
    git(
        &world.home,
        &[
            "clone",
            "-q",
            world.base.join("remote.git").to_str().unwrap(),
            twin.to_str().unwrap(),
        ],
    );
    let _paths = world.enter();
    let data = remote::status(false).unwrap();
    assert_eq!(data["repo"], world.lib.to_string_lossy().as_ref());
    assert_eq!(data["fetch"]["requested"], false);
    assert_eq!(data["duplicateClones"][0]["skills"], 1);
    assert_eq!(
        data["duplicateClones"][0]["path"],
        twin.to_string_lossy().as_ref()
    );
}

#[test]
fn the_remote_url_shows_no_embedded_credential() {
    let world = World::new("redact");
    git(
        &world.lib,
        &[
            "remote",
            "set-url",
            "origin",
            "https://user:FAKE-credential@example.invalid/o/skills.git",
        ],
    );
    let _paths = world.enter();
    let data = remote::status(false).unwrap();
    assert_eq!(data["remote"], "https://***@example.invalid/o/skills.git");
}

#[test]
fn a_pull_refuses_over_uncommitted_files_and_fast_forwards_when_clean() {
    let world = World::new("pull");
    world.remote_commit("notes");
    let _paths = world.enter();
    put(world.lib.join("scratch.md"), "draft\n");
    let refused = pull(&world.lib, false, false).unwrap_err();
    assert_eq!(refused.kind.code(), "refused");
    assert!(refused.message.contains("uncommitted"), "{refused:?}");
    std::fs::remove_file(world.lib.join("scratch.md")).unwrap();
    let data = pull(&world.lib, false, false).unwrap();
    assert_eq!(data["pulled"], true);
    assert!(world.lib.join("skills/notes/SKILL.md").exists());
    assert!(data.get("syncPlan").is_none());
}

#[test]
fn review_adds_the_sync_plan_after_a_pull_and_syncs_nothing() {
    let world = World::new("review");
    world.remote_commit("notes");
    let _paths = world.enter();
    let data = pull(&world.lib, false, true).unwrap();
    assert_eq!(data["pulled"], true);
    assert_eq!(data["syncPlan"]["dryRun"], true);
    assert!(!world.home.join(".claude/skills/notes").exists());
}

#[test]
fn a_dry_run_pull_never_carries_a_sync_plan() {
    let world = World::new("review-dry");
    world.remote_commit("notes");
    let _paths = world.enter();
    let data = pull(&world.lib, true, true).unwrap();
    assert_eq!(data["dryRun"], true);
    assert!(data.get("syncPlan").is_none());
}

#[test]
fn a_diverged_clone_is_refused_and_left_alone() {
    let world = World::new("diverged");
    world.remote_commit("remote-side");
    put(world.lib.join("skills/local/SKILL.md"), &skill("local"));
    git(&world.lib, &["add", "-A"]);
    git(&world.lib, &["commit", "-q", "-m", "Add local"]);
    let _paths = world.enter();
    let head = remote::local_state(&world.lib).head;
    let refused = pull(&world.lib, false, false).unwrap_err();
    assert!(refused.message.contains("diverged"), "{refused:?}");
    assert_eq!(remote::local_state(&world.lib).head, head);
}

#[test]
fn a_secret_in_a_commit_or_the_working_tree_makes_push_refuse() {
    let world = World::new("secret");
    std::env::set_var("TOOLPORT_GITLEAKS_BIN", "/nonexistent/gitleaks");
    let _paths = world.enter();
    put(
        world.lib.join("skills/leaky/SKILL.md"),
        &format!("{}token = {}\n", skill("leaky"), secret()),
    );
    let refused = sync::push(&world.lib, None, false).unwrap_err();
    assert_eq!(refused.kind.code(), "refused");
    assert!(refused.message.contains("github-token"), "{refused:?}");
    assert!(
        !refused.message.contains(&secret()),
        "a finding names a rule, never the matched text"
    );
    let preview = sync::push(&world.lib, None, true).unwrap();
    assert_eq!(preview["pushed"], false);
    assert!(preview["plan"]["warnings"][0]
        .as_str()
        .unwrap()
        .contains("would be refused"));
}

#[test]
fn a_clean_push_commits_pending_files_and_sends_them() {
    let world = World::new("push");
    std::env::set_var("TOOLPORT_GITLEAKS_BIN", "/nonexistent/gitleaks");
    let _paths = world.enter();
    put(world.lib.join("skills/extra/SKILL.md"), &skill("extra"));
    let data = sync::push(&world.lib, Some("Add extra"), false).unwrap();
    assert_eq!(data["pushed"], true);
    let state = remote::local_state(&world.lib);
    assert_eq!((state.ahead, state.uncommitted), (0, 0));
}
