use super::{connect, pull, remote, sync};
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

fn identity(dir: &Path) {
    git(dir, &["config", "user.name", "t"]);
    git(dir, &["config", "user.email", "t@example.invalid"]);
}

/// A bare remote with one skill and a home that has no library yet; the data dir is a folder of
/// its own, as in the app.
struct Fresh {
    base: PathBuf,
    home: PathBuf,
    data: PathBuf,
    bare: PathBuf,
}

impl Fresh {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "library-fresh-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (home, data, bare, seed) = (
            base.join("home"),
            base.join("data"),
            base.join("remote.git"),
            base.join("seed"),
        );
        for dir in [&home, &data, &bare, &seed] {
            std::fs::create_dir_all(dir).unwrap();
        }
        git(&bare, &["init", "-q", "--bare", "-b", "main"]);
        git(&seed, &["init", "-q", "-b", "main"]);
        identity(&seed);
        put(seed.join("skills/review/SKILL.md"), &skill("review"));
        git(&seed, &["add", "-A"]);
        git(&seed, &["commit", "-q", "-m", "Add review"]);
        git(&seed, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(&seed, &["push", "-q", "-u", "origin", "main"]);
        Self {
            base,
            home,
            data,
            bare,
        }
    }

    fn enter(&self) -> crate::fsx::PathsGuard {
        enter(Paths {
            home: Some(self.home.clone()),
            data_dir: Some(self.data.clone()),
        })
    }

    fn url(&self) -> String {
        self.bare.to_string_lossy().into_owned()
    }
}

impl Drop for Fresh {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[test]
fn connect_clones_into_the_chosen_folder_and_points_the_library_at_it() {
    let world = Fresh::new("connect");
    let _paths = world.enter();
    let target = world.home.join("work/library");
    let preview = connect::connect(&world.url(), target.to_str(), true).unwrap();
    assert_eq!(preview["dryRun"], true);
    assert!(!target.exists());
    assert!(remote::locate().is_err());
    let data = connect::connect(&world.url(), target.to_str(), false).unwrap();
    assert_eq!(data["connected"], true);
    assert!(target.join("skills/review/SKILL.md").is_file());
    assert_eq!(remote::locate().unwrap(), target);
    let config = std::fs::read_to_string(world.data.join("skills_sync.json")).unwrap();
    assert!(config.contains(target.to_str().unwrap()));
}

#[test]
fn connect_refuses_a_second_library_a_full_folder_and_unsafe_urls() {
    let world = Fresh::new("connect-refused");
    let _paths = world.enter();
    let full = world.home.join("full");
    put(full.join("notes.txt"), "keep\n");
    let error = connect::connect(&world.url(), full.to_str(), false).unwrap_err();
    assert!(error.message.contains("is not empty"), "{}", error.message);
    for url in [
        "https://user:FAKE-credential@example.invalid/o/skills.git",
        "--upload-pack=sh",
        "ext::sh -c id",
        "",
    ] {
        let error = connect::connect(url, Some("/tmp/never-cloned-here"), true).unwrap_err();
        assert_eq!(error.kind.code(), "usage", "{url}");
        assert!(
            !error.message.contains("FAKE-credential"),
            "{}",
            error.message
        );
    }
    let target = world.home.join("library");
    connect::connect(&world.url(), target.to_str(), false).unwrap();
    let again = connect::connect(&world.url(), world.home.join("other").to_str(), false);
    assert!(again.unwrap_err().message.contains("already connected"));
}

#[test]
fn a_repository_that_cannot_be_cloned_says_why_without_git_text() {
    let world = Fresh::new("connect-missing");
    let _paths = world.enter();
    let target = world.home.join("library");
    let missing = world.base.join("nowhere.git");
    let error = connect::connect(missing.to_str().unwrap(), target.to_str(), false).unwrap_err();
    assert_eq!(error.kind.code(), "network");
    assert_eq!(error.message, "repository not found");
    assert!(remote::locate().is_err());
}

#[test]
fn remote_add_gives_a_library_without_a_remote_its_origin_once() {
    let world = World::new("remote-add");
    let _paths = world.enter();
    git(&world.lib, &["remote", "remove", "origin"]);
    let url = world.base.join("remote.git");
    let preview = connect::remote_add(&world.lib, url.to_str().unwrap(), true).unwrap();
    assert_eq!(preview["dryRun"], true);
    assert!(remote::local_state(&world.lib).url.is_none());
    connect::remote_add(&world.lib, url.to_str().unwrap(), false).unwrap();
    assert_eq!(remote::local_state(&world.lib).url.as_deref(), url.to_str());
    let again = connect::remote_add(&world.lib, url.to_str().unwrap(), false).unwrap_err();
    assert!(again.message.contains("already has a remote"));
}

#[test]
fn commit_scans_the_uncommitted_work_and_names_a_finding_by_rule_file_and_line() {
    let world = World::new("commit");
    std::env::set_var("TOOLPORT_GITLEAKS_BIN", "/nonexistent/gitleaks");
    let _paths = world.enter();
    identity(&world.lib);
    let clean = connect::commit(&world.lib, "Nothing here", false).unwrap();
    assert_eq!(clean["committed"], false);
    put(
        world.lib.join("skills/leaky/SKILL.md"),
        &format!("{}token = {}\n", skill("leaky"), secret()),
    );
    let preview = connect::commit(&world.lib, "Add leaky", true).unwrap();
    assert!(preview["plan"]["warnings"][0]
        .as_str()
        .unwrap()
        .contains("github-token in skills/leaky/SKILL.md:7"));
    let refused = connect::commit(&world.lib, "Add leaky", false).unwrap_err();
    assert_eq!(refused.kind.code(), "refused");
    assert!(refused
        .message
        .contains("github-token in skills/leaky/SKILL.md:7"));
    assert!(!refused.message.contains(&secret()));
    assert_eq!(remote::local_state(&world.lib).uncommitted, 1);
}

#[test]
fn commit_commits_everything_with_the_message_and_does_not_push() {
    let world = World::new("commit-clean");
    std::env::set_var("TOOLPORT_GITLEAKS_BIN", "/nonexistent/gitleaks");
    let _paths = world.enter();
    identity(&world.lib);
    put(world.lib.join("skills/extra/SKILL.md"), &skill("extra"));
    let empty = connect::commit(&world.lib, "  ", false).unwrap_err();
    assert_eq!(empty.kind.code(), "usage");
    let data = connect::commit(&world.lib, "Add extra", false).unwrap();
    assert_eq!(data["committed"], true);
    assert_eq!(data["files"][0], "skills/extra/SKILL.md");
    let state = remote::local_state(&world.lib);
    assert_eq!((state.ahead, state.uncommitted), (1, 0));
    let log = remote::git_out(&world.lib, &["log", "-1", "--format=%s"]).unwrap();
    assert_eq!(log, "Add extra");
}

#[test]
fn the_first_push_to_an_empty_remote_sets_the_upstream_and_sends_the_history() {
    let world = Fresh::new("first-push");
    std::env::set_var("TOOLPORT_GITLEAKS_BIN", "/nonexistent/gitleaks");
    let _paths = world.enter();
    let lib = world.home.join("library");
    std::fs::create_dir_all(&lib).unwrap();
    git(&lib, &["init", "-q", "-b", "main"]);
    identity(&lib);
    put(lib.join("skills/mine/SKILL.md"), &skill("mine"));
    git(&lib, &["add", "-A"]);
    git(&lib, &["commit", "-q", "-m", "Add mine"]);
    let empty = world.base.join("empty.git");
    std::fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-q", "--bare", "-b", "main"]);
    let none = sync::push(&lib, None, false).unwrap_err();
    assert!(none.message.contains("no remote"), "{}", none.message);
    connect::remote_add(&lib, empty.to_str().unwrap(), false).unwrap();
    let preview = sync::push(&lib, None, true).unwrap();
    assert!(preview["plan"]["summary"]
        .as_str()
        .unwrap()
        .starts_with("Push 1 commit"));
    let data = sync::push(&lib, None, false).unwrap();
    assert_eq!(data["pushed"], true);
    assert_eq!(
        remote::local_state(&lib).upstream.as_deref(),
        Some("origin/main")
    );
    let sent = remote::git_out(&empty, &["log", "-1", "--format=%s", "main"]).unwrap();
    assert_eq!(sent, "Add mine");
}
