use super::check::{self, Request};
use super::ops::{self, Choices, Env, Options, SetArgs};
use super::store::{self, SourceSel};
use crate::plugins::claude::ClaudeRunner;
use crate::plugins::Env as PluginEnv;
use crate::skills::git::{GitRunner, SystemGit};
use crate::skills::gitexec::CmdOutput;
use crate::skills::tap_fixtures::git_in;
use crate::skills::taps::{save_taps, Tap};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use toolport_plus_api::memory::MemoryHost;

const TAP: &str = "history-tap";

struct NoClaude;

impl ClaudeRunner for NoClaude {
    fn run(&self, _: &[&str], _: Duration) -> Result<CmdOutput, String> {
        Err("not used".into())
    }

    fn present(&self) -> bool {
        false
    }
}

/// The real git, counting how often a whole history was asked for.
#[derive(Default)]
struct Counting {
    deepens: Cell<usize>,
    deepen_fails: RefCell<Option<String>>,
}

impl GitRunner for Counting {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String> {
        SystemGit.clone_repo(url, dest)
    }

    fn clone_shallow(&self, url: &str, dest: &Path) -> Result<(), String> {
        SystemGit.clone_shallow(url, dest)
    }

    fn pull(&self, repo: &Path) -> Result<String, String> {
        SystemGit.pull(repo)
    }

    fn head(&self, repo: &Path) -> Result<String, String> {
        SystemGit.head(repo)
    }

    fn deepen(&self, repo: &Path) -> Result<(), String> {
        self.deepens.set(self.deepens.get() + 1);
        if let Some(why) = self.deepen_fails.borrow().clone() {
            return Err(why);
        }
        SystemGit.deepen(repo)
    }

    fn is_ancestor(&self, repo: &Path, a: &str, d: &str) -> Result<Option<bool>, String> {
        SystemGit.is_ancestor(repo, a, d)
    }
}

struct World {
    _dir: TempDir,
    plugins: PluginEnv,
    host: MemoryHost,
    git: Counting,
    work: PathBuf,
    remote: PathBuf,
    first: String,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let (work, remote, data_dir) = (
            base.join("work"),
            base.join("remote.git"),
            base.join("data"),
        );
        std::fs::create_dir_all(&work).unwrap();
        git_in(&work, &["init", "-q"]);
        write(&work, "skills/code-review/SKILL.md", "one");
        let first = commit(&work, "first");
        git_in(
            &base,
            &["clone", "-q", "--bare", "work", &remote.to_string_lossy()],
        );
        let url = format!("file://{}", remote.display());
        save_taps(
            &data_dir,
            &[Tap {
                name: TAP.into(),
                repo: "example/history-tap".into(),
                url: url.clone(),
            }],
        )
        .unwrap();
        let clone = data_dir.join("taps").join(TAP);
        std::fs::create_dir_all(clone.parent().unwrap()).unwrap();
        SystemGit.clone_shallow(&url, &clone).unwrap();
        let claude_home = base.join("home/.claude");
        std::fs::create_dir_all(&claude_home).unwrap();
        let world = Self {
            host: MemoryHost {
                settings_paths: vec![claude_home.join("settings.json")],
                ..MemoryHost::default()
            },
            plugins: PluginEnv {
                claude_home,
                data_dir,
            },
            git: Counting::default(),
            work,
            remote,
            first,
            _dir: dir,
        };
        let args = SetArgs {
            on: vec!["code-review".into()],
            ..SetArgs::default()
        };
        ops::set(&world.env(), TAP, &args, false).unwrap();
        world.pin(&world.first.clone());
        world
    }

    fn env(&self) -> Env<'_> {
        Env {
            plugins: &self.plugins,
            git: &self.git,
            claude: &NoClaude,
            store: &self.host,
            managed: None,
            library: None,
        }
    }

    fn base(&self) -> PathBuf {
        super::base_dir(&self.plugins.data_dir)
    }

    fn pin(&self, sha: &str) {
        let mut selections = store::load(&self.base()).unwrap();
        let source: &mut SourceSel = selections.sources.get_mut(TAP).unwrap();
        source.pinned = Some(sha.to_string());
        store::save(&self.base(), &selections).unwrap();
    }

    fn push_new_commit(&self) -> String {
        write(&self.work, "skills/code-review/SKILL.md", "two");
        let sha = commit(&self.work, "second");
        git_in(
            &self.work,
            &["push", "-q", &self.remote.to_string_lossy(), "HEAD:main"],
        );
        sha
    }

    /// Replaces the first commit on the remote, as a force-push of a rebased branch does.
    fn rewrite_first_commit(&self) -> String {
        write(&self.work, "skills/code-review/SKILL.md", "rewritten");
        git_in(
            &self.work,
            &["commit", "-q", "--amend", "-a", "-m", "first, again"],
        );
        let sha = git_in(&self.work, &["rev-parse", "HEAD"]);
        git_in(
            &self.work,
            &[
                "push",
                "-q",
                "--force",
                &self.remote.to_string_lossy(),
                "HEAD:main",
            ],
        );
        sha
    }

    fn plan(&self, deepen: bool) -> Value {
        let opts = Options {
            fetch: true,
            deepen,
            ..Options::default()
        };
        ops::plan(&self.env(), TAP, opts).unwrap()
    }

    fn check(&self, deepen: bool) -> Value {
        let request = Request {
            fetch: true,
            source: None,
            deepen,
        };
        check::run(&self.env(), request).unwrap()
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!("---\nname: code-review\ndescription: A skill\n---\n{text}\n"),
    )
    .unwrap();
}

fn commit(work: &Path, message: &str) -> String {
    git_in(work, &["add", "-A"]);
    git_in(work, &["commit", "-q", "-m", message]);
    git_in(work, &["rev-parse", "HEAD"])
}

#[test]
fn test40_a_pinned_commit_the_remote_still_has_leaves_the_history_intact() {
    let world = World::new();
    world.push_new_commit();
    let plan = world.plan(true);
    assert_eq!(plan["history"]["state"], "intact");
    assert_eq!(plan["history"]["pinned"], json!(world.first));
    assert_eq!(world.git.deepens.get(), 1);
}

#[test]
fn test41_a_force_push_that_removed_the_pinned_commit_is_called_a_rewritten_history() {
    let world = World::new();
    let rewritten = world.rewrite_first_commit();
    let plan = world.plan(true);
    assert_eq!(plan["history"]["state"], "rewritten");
    assert_eq!(plan["ref"]["latest"], json!(rewritten));
    let warnings = plan["plan"]["warnings"].to_string();
    assert!(warnings.contains("was rewritten"), "{warnings}");
    assert_eq!(plan["waiting"], true);
}

#[test]
fn test42_nothing_deepens_unless_the_caller_asks() {
    let world = World::new();
    world.rewrite_first_commit();
    assert_eq!(world.plan(false)["history"]["state"], "unchecked");
    assert_eq!(
        world.check(false)["sources"][0]["history"]["state"],
        "unchecked"
    );
    assert_eq!(world.git.deepens.get(), 0);
    assert!(check::rewritten(&world.base()).is_empty());
}

#[test]
fn test43_a_source_already_at_the_remote_head_needs_no_history_download() {
    let world = World::new();
    assert_eq!(world.plan(true)["history"]["state"], "intact");
    assert_eq!(world.git.deepens.get(), 0);
}

#[test]
fn test44_a_history_that_cannot_be_downloaded_is_unknown_never_rewritten() {
    let world = World::new();
    world.rewrite_first_commit();
    *world.git.deepen_fails.borrow_mut() = Some("early EOF".into());
    let plan = world.plan(true);
    assert_eq!(plan["history"]["state"], "unknown");
    assert_eq!(plan["history"]["error"], "early EOF");
    assert!(!plan["plan"]["warnings"]
        .to_string()
        .contains("was rewritten"));
}

#[test]
fn test45_a_deepening_check_records_the_finding_and_a_plain_check_keeps_it() {
    let world = World::new();
    let rewritten = world.rewrite_first_commit();
    let out = world.check(true);
    assert_eq!(out["sources"][0]["history"]["state"], "rewritten");
    assert!(out["sources"][0]["history"]["checkedAt"].is_string());
    let found = check::rewritten(&world.base());
    assert_eq!(found.len(), 1);
    assert_eq!(
        (
            found[0].source.as_str(),
            found[0].pinned.as_str(),
            found[0].latest.as_str()
        ),
        (TAP, world.first.as_str(), rewritten.as_str())
    );
    let later = world.check(false);
    assert_eq!(later["sources"][0]["history"]["state"], "rewritten");
    assert_eq!(check::rewritten(&world.base()).len(), 1);
    assert_eq!(world.git.deepens.get(), 1);
}

#[test]
fn test46_pinning_to_the_new_history_ends_the_finding() {
    let world = World::new();
    let rewritten = world.rewrite_first_commit();
    world.check(true);
    world.pin(&rewritten);
    assert!(check::rewritten(&world.base()).is_empty());
    let listed = ops::ls(&world.env(), Some(TAP)).unwrap();
    assert_eq!(listed["history"]["state"], "unchecked");
}

#[test]
fn test47_the_list_of_a_source_carries_the_last_recorded_verdict() {
    let world = World::new();
    world.rewrite_first_commit();
    assert_eq!(
        ops::ls(&world.env(), Some(TAP)).unwrap()["history"]["state"],
        "unchecked"
    );
    world.check(true);
    let listed = ops::ls(&world.env(), Some(TAP)).unwrap();
    assert_eq!(listed["history"]["state"], "rewritten");
    assert_eq!(listed["history"]["pinned"], json!(world.first));
}

#[test]
fn test48_a_check_names_one_source_or_refuses_one_that_has_no_selection() {
    let world = World::new();
    world.check(false);
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(world.base().join(check::UPDATES_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(doc["sources"].as_array().unwrap().len(), 1);
    let one = check::run(
        &world.env(),
        Request {
            fetch: false,
            source: Some(TAP),
            deepen: false,
        },
    )
    .unwrap();
    assert_eq!(one["sources"].as_array().unwrap().len(), 1);
    let unknown = check::run(
        &world.env(),
        Request {
            fetch: false,
            source: Some("nothing"),
            deepen: false,
        },
    )
    .unwrap_err();
    assert!(unknown.message.contains("has no selection"));
}

#[test]
fn test49_apply_never_deepens_even_after_a_rewritten_finding() {
    let world = World::new();
    world.rewrite_first_commit();
    let hash = world.plan(true)["planHash"].as_str().unwrap().to_string();
    let before = world.git.deepens.get();
    let _ = ops::apply(
        &world.env(),
        TAP,
        &hash,
        Options {
            fetch: true,
            ..Options::default()
        },
        &Choices::default(),
    );
    assert_eq!(world.git.deepens.get(), before);
}
