use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Map, Value};
use toolport_plus_api::{
    GatewayRouter, MemoryGateway, MemoryHost, MemoryRegistry, MemorySecrets, ServerRecord,
};

use super::discover::{candidates, discover, in_package_cache};
use super::screen;
use super::state::{self, Verdict};
use super::update::{self, RestartChoice, UpdateArgs};
use super::*;
use crate::fsx::{enter, Paths, PathsGuard};
use crate::servers::restart::{self, Timing};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const FAST: Timing = Timing {
    poll: Duration::from_millis(15),
    settle: Duration::from_millis(120),
    ack_wait: Duration::from_millis(300),
};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

struct World {
    base: PathBuf,
    data: PathBuf,
    /// Another clone that pushes new commits to the remote.
    other: PathBuf,
    /// The checkout the server runs from.
    work: PathBuf,
    _paths: PathsGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir()
            .join(format!(
                "plus-repo-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ))
            .canonicalize_or_self();
        let (data, other, work) = (base.join("data"), base.join("other"), base.join("work"));
        std::fs::create_dir_all(base.join("home")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        git(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(&base, &["clone", "-q", "remote.git", "other"]);
        std::fs::write(other.join("README.md"), "one").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "one"]);
        git(&other, &["push", "-q", "origin", "HEAD:main"]);
        git(&base, &["clone", "-q", "remote.git", "work"]);
        Self {
            _paths: enter(Paths {
                home: Some(base.join("home")),
                data_dir: Some(data.clone()),
            }),
            base,
            data,
            other,
            work,
        }
    }

    /// A commit pushed from the other clone.
    fn upstream_commit(&self, file: &str, subject: &str) {
        git(&self.other, &["pull", "-q", "--ff-only", "origin", "main"]);
        std::fs::write(self.other.join(file), subject).unwrap();
        git(&self.other, &["add", "."]);
        git(&self.other, &["commit", "-q", "-m", subject]);
        git(&self.other, &["push", "-q", "origin", "HEAD:main"]);
    }

    fn local_commit(&self, file: &str, subject: &str) {
        std::fs::write(self.work.join(file), subject).unwrap();
        git(&self.work, &["add", "."]);
        git(&self.work, &["commit", "-q", "-m", subject]);
    }

    fn fetch(&self) {
        fetch_tracked(&self.work).unwrap();
    }

    fn head(&self) -> String {
        git(&self.work, &["rev-parse", "HEAD"])
    }

    fn server(&self, id: &str, name: &str) -> ServerRecord {
        let mut extra = Map::new();
        extra.insert("transport".into(), json!("stdio"));
        extra.insert("command".into(), json!("node"));
        extra.insert("args".into(), json!(["server.js"]));
        extra.insert("cwd".into(), json!(self.work.to_string_lossy()));
        ServerRecord {
            id: id.into(),
            name: name.into(),
            enabled: true,
            extra,
            ..Default::default()
        }
    }

    fn host(&self, servers: Vec<ServerRecord>) -> MemoryHost {
        MemoryHost {
            registry: MemoryRegistry::with_servers(servers),
            secrets: MemorySecrets {
                data_dir: Some(self.data.clone()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn alpha(&self) -> MemoryHost {
        self.host(vec![self.server("srv-alpha", "alpha")])
    }
}

trait CanonicalOrSelf {
    fn canonicalize_or_self(self) -> PathBuf;
}

impl CanonicalOrSelf for PathBuf {
    fn canonicalize_or_self(self) -> PathBuf {
        let _ = std::fs::create_dir_all(&self);
        self.canonicalize().unwrap_or(self)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn args(key: &str) -> UpdateArgs {
    UpdateArgs {
        timing: FAST,
        restart_wait: Duration::from_secs(5),
        ..UpdateArgs::new(key)
    }
}

fn record(
    command: Option<&str>,
    cwd: Option<&str>,
    args: &[&str],
    transport: &str,
) -> ServerRecord {
    let mut extra = Map::new();
    extra.insert("transport".into(), json!(transport));
    if let Some(command) = command {
        extra.insert("command".into(), json!(command));
    }
    if let Some(cwd) = cwd {
        extra.insert("cwd".into(), json!(cwd));
    }
    extra.insert("args".into(), json!(args));
    ServerRecord {
        id: "s".into(),
        name: "s".into(),
        extra,
        ..Default::default()
    }
}

#[test]
fn test01_candidates_read_the_cwd_the_command_and_path_arguments() {
    let server = record(
        Some("/work/alpha/.venv/bin/python"),
        Some("/work/alpha"),
        &[
            "./server.py",
            "--config=/etc/alpha.json",
            "--verbose",
            "@scope/pkg",
            "https://x.invalid/a",
        ],
        "stdio",
    );
    let paths: Vec<String> = candidates(&server, None)
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        paths,
        [
            "/work/alpha",
            "/work/alpha/.venv/bin/python",
            "/work/alpha/./server.py",
            "/etc/alpha.json"
        ]
    );
}

#[test]
fn test02_remote_servers_package_runners_and_placeholders_name_no_checkout() {
    assert!(candidates(&record(None, None, &[], "http"), None).is_empty());
    assert!(candidates(&record(Some("node"), None, &["server.js"], "sse"), None).is_empty());
    let runner = candidates(
        &record(
            Some("npx"),
            Some("/work/alpha"),
            &["-y", "/data/projects/foo"],
            "stdio",
        ),
        None,
    );
    assert_eq!(
        runner,
        [PathBuf::from("/work/alpha")],
        "an argument of npx is data, not code"
    );
    let rooted = record(Some("node"), Some("${ROOT}/mcp"), &["server.js"], "stdio");
    assert!(candidates(&rooted, None).is_empty());
    let home = Path::new("/home/me");
    let tilde = record(Some("node"), Some("~/work/alpha"), &[], "stdio");
    assert_eq!(candidates(&tilde, Some(home)), [home.join("work/alpha")]);
}

#[test]
fn test03_package_manager_caches_are_not_checkouts() {
    for path in [
        "/home/me/.npm/_npx/abc/node_modules/x/index.js",
        "/home/me/.cache/uv/archive-v0/x",
        "/home/me/.cargo/registry/src/x",
        "/home/me/.local/share/uv/tools/x/bin/x",
        "/Users/me/Library/Caches/x",
        "/work/alpha/node_modules/dep/cli.js",
    ] {
        assert!(in_package_cache(Path::new(path)), "{path}");
    }
    assert!(!in_package_cache(Path::new("/work/alpha/src/server.js")));
    let server = record(
        Some("node"),
        Some("/work/alpha"),
        &["/work/alpha/node_modules/dep/cli.js"],
        "stdio",
    );
    assert_eq!(candidates(&server, None), [PathBuf::from("/work/alpha")]);
}

#[test]
fn test04_a_server_in_a_work_tree_is_found_with_its_root_and_others_are_not() {
    let world = World::new("discover");
    let sub = world.work.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    let mut inside = world.server("srv-in", "inside");
    inside
        .extra
        .insert("cwd".into(), json!(sub.to_string_lossy()));
    let plain = world.base.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let mut outside = world.server("srv-out", "outside");
    outside
        .extra
        .insert("cwd".into(), json!(plain.to_string_lossy()));
    let mut remote = world.server("srv-http", "remote");
    remote.extra.insert("transport".into(), json!("http"));
    let mut bare = world.server("srv-bare", "bare");
    bare.extra.remove("cwd");
    let found = discover(
        &[inside, outside, remote, bare],
        Some(&world.base.join("home")),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].server_name, "inside");
    assert_eq!(found[0].root, world.work);
}

#[test]
fn test05_a_checkout_that_is_the_home_folder_is_not_a_server_repo() {
    let world = World::new("homerepo");
    let found = discover(&[world.server("srv-alpha", "alpha")], Some(&world.work));
    assert!(
        found.is_empty(),
        "a dotfiles repo in $HOME is not the server's code"
    );
}

#[test]
fn test06_servers_sharing_a_checkout_are_one_repo_named_by_the_first() {
    let world = World::new("shared");
    let host = world.host(vec![
        world.server("srv-b", "beta"),
        world.server("srv-a", "alpha"),
    ]);
    let found = repos(&host).unwrap();
    assert_eq!(found.len(), 1);
    let names: Vec<&str> = found[0].servers.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, ["alpha", "beta"]);
    assert_eq!(found[0].source(), "server:alpha");
    let rows = status_value(&found);
    assert_eq!(rows["repos"].as_array().unwrap().len(), 2);
}

#[test]
fn test10_a_fetch_shows_new_commits_and_moves_neither_head_nor_files() {
    let world = World::new("fetch");
    let before = world.head();
    world.upstream_commit("two.txt", "two");
    assert_eq!(
        state::read(&world.work).behind,
        0,
        "not seen before the fetch"
    );
    world.fetch();
    let seen = state::read(&world.work);
    assert_eq!((seen.behind, seen.ahead, seen.dirty), (1, 0, 0));
    assert_eq!(seen.verdict(), Verdict::Behind);
    assert_eq!(seen.upstream.as_deref(), Some("origin/main"));
    assert_eq!(world.head(), before);
    assert!(!world.work.join("two.txt").exists());
}

#[test]
fn test11_only_the_tracked_remote_is_fetched() {
    let world = World::new("tracked");
    let spare = world.base.join("spare.git");
    git(
        &world.base,
        &["clone", "-q", "--bare", "remote.git", "spare.git"],
    );
    git(
        &world.work,
        &["remote", "add", "spare", &spare.to_string_lossy()],
    );
    git(&world.work, &["fetch", "-q", "spare"]);
    let spare_before = git(&world.work, &["rev-parse", "refs/remotes/spare/main"]);
    world.upstream_commit("two.txt", "two");
    git(
        &world.base.join("other"),
        &["push", "-q", &spare.to_string_lossy(), "HEAD:main"],
    );
    world.fetch();
    assert_eq!(
        git(&world.work, &["rev-parse", "refs/remotes/spare/main"]),
        spare_before
    );
    assert_eq!(state::read(&world.work).behind, 1);
}

#[test]
fn test12_the_verdict_names_dirty_diverged_detached_untracked_and_open_operations() {
    let world = World::new("verdicts");
    world.upstream_commit("two.txt", "two");
    world.fetch();
    std::fs::write(world.work.join("scratch.txt"), "untracked").unwrap();
    assert_eq!(
        state::read(&world.work).verdict(),
        Verdict::Behind,
        "untracked files do not block"
    );
    std::fs::write(world.work.join("README.md"), "edited").unwrap();
    assert_eq!(state::read(&world.work).verdict(), Verdict::Dirty);
    git(&world.work, &["checkout", "-q", "--", "README.md"]);
    std::fs::write(world.work.join(".git").join("MERGE_HEAD"), "x").unwrap();
    assert_eq!(state::read(&world.work).verdict(), Verdict::Busy);
    std::fs::remove_file(world.work.join(".git").join("MERGE_HEAD")).unwrap();
    world.local_commit("mine.txt", "mine");
    let diverged = state::read(&world.work);
    assert_eq!(diverged.verdict(), Verdict::Diverged);
    assert_eq!((diverged.ahead, diverged.behind), (1, 1));
    git(&world.work, &["checkout", "-q", "--detach"]);
    assert_eq!(state::read(&world.work).verdict(), Verdict::Detached);
    git(&world.work, &["checkout", "-q", "-b", "topic"]);
    assert_eq!(state::read(&world.work).verdict(), Verdict::NoUpstream);
}

#[test]
fn test13_the_egress_list_stops_a_fetch_before_git_runs() {
    let world = World::new("egress");
    git(
        &world.work,
        &[
            "remote",
            "set-url",
            "origin",
            "https://updates.toolport.app/x.git",
        ],
    );
    let error = fetch_tracked(&world.work).unwrap_err();
    assert!(error.contains("egress to toolport.app"), "{error}");
}

#[test]
fn test14_head_records_say_whether_the_head_is_on_the_remote() {
    let world = World::new("heads");
    let host = world.alpha();
    let pushed = head_records(&repos(&host).unwrap());
    assert_eq!(pushed.len(), 1);
    assert!(pushed[0].pushed && pushed[0].branch == "main" && pushed[0].server == "alpha");
    assert!(pushed[0].remote.ends_with("remote.git"));
    world.local_commit("mine.txt", "mine");
    let ahead = head_records(&repos(&host).unwrap());
    assert!(!ahead[0].pushed);
    assert_eq!(ahead[0].head, world.head());
}

#[test]
fn test15_check_records_the_fetch_and_a_failure_names_the_checkout() {
    let world = World::new("check");
    world.upstream_commit("two.txt", "two");
    let host = world.alpha();
    let report = check(&host, 1_000, &fetch_tracked).unwrap();
    assert_eq!(report.repos.len(), 1);
    assert!(report.repos[0].1.is_ok());
    let found = repos(&host).unwrap();
    assert_eq!(found[0].state.behind, 1);
    assert_eq!(found[0].checked.as_ref().unwrap().at, 1_000);
    let failed = check(&host, 2_000, &|_| Err("network unreachable".into())).unwrap();
    assert_eq!(
        failed.repos[0].1.as_ref().unwrap_err(),
        "network unreachable"
    );
    assert!(failed.to_value()["repos"][0]["ok"] == false);
    let again = repos(&host).unwrap();
    assert_eq!(
        again[0].checked.as_ref().unwrap().error.as_deref(),
        Some("network unreachable")
    );
}

#[test]
fn test20_the_plan_lists_the_commits_the_files_and_the_build_and_changes_nothing() {
    let world = World::new("plan");
    world.upstream_commit("two.txt", "Add the second file");
    world.upstream_commit("three.txt", "Add the third file");
    world.fetch();
    let before = world.head();
    let plan = update::plan(&world.alpha(), &args("alpha"), false).unwrap();
    assert!(plan.refusal.is_none());
    assert_eq!(plan.commits_total, 2);
    assert_eq!(plan.commits[0].1, "Add the third file");
    let paths: Vec<&str> = plan.files.iter().map(|(_, p)| p.as_str()).collect();
    assert_eq!(paths, ["three.txt", "two.txt"]);
    assert!(plan.files.iter().all(|(status, _)| status == "A"));
    assert_eq!(plan.build.command, None);
    let value = plan.to_value();
    assert_eq!(value["from"], json!(before));
    assert_eq!(value["planHash"], json!(plan.hash));
    assert_eq!(world.head(), before);
    let same = update::plan(&world.alpha(), &args("alpha"), false).unwrap();
    assert_eq!(same.hash, plan.hash);
    let with_build = update::plan(
        &world.alpha(),
        &UpdateArgs {
            build: Some("make".into()),
            ..args("alpha")
        },
        false,
    )
    .unwrap();
    assert_ne!(
        with_build.hash, plan.hash,
        "the build is part of what was reviewed"
    );
}

#[test]
fn test21_an_update_fast_forwards_only_the_tracked_branch_and_undo_puts_it_back() {
    let world = World::new("apply");
    git(&world.work, &["branch", "side"]);
    let side = git(&world.work, &["rev-parse", "side"]);
    let before = world.head();
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let target = git(&world.work, &["rev-parse", "origin/main"]);
    let host = world.alpha();
    let done = update::apply(
        &host,
        &UpdateArgs {
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    assert!(done.moved && done.from == before && done.to == target);
    assert_eq!(world.head(), target);
    assert!(world.work.join("two.txt").exists());
    assert_eq!(
        git(&world.work, &["rev-parse", "side"]),
        side,
        "other branches stay"
    );
    assert_eq!(state::read(&world.work).verdict(), Verdict::UpToDate);

    let back = update::undo(
        &host,
        &UpdateArgs {
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    assert!(back.moved);
    assert_eq!(world.head(), before);
    assert!(!world.work.join("two.txt").exists());
    assert_eq!(state::read(&world.work).verdict(), Verdict::Behind);
    let again = update::undo(&host, &args("alpha")).unwrap_err();
    assert!(again.message.contains("no update of alpha to take back"));
}

#[test]
fn test22_dirty_diverged_and_detached_checkouts_are_refused_and_left_as_they_are() {
    let world = World::new("refuse");
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let host = world.alpha();
    let before = world.head();
    std::fs::write(world.work.join("README.md"), "edited").unwrap();
    let dirty = update::apply(&host, &args("alpha")).unwrap_err();
    assert_eq!(dirty.code(), "update_refused");
    assert!(dirty.message.contains("local changes in 1 tracked file"));
    assert_eq!(dirty.details.unwrap()["reason"], "dirty");
    assert_eq!(
        std::fs::read_to_string(world.work.join("README.md")).unwrap(),
        "edited"
    );
    git(&world.work, &["checkout", "-q", "--", "README.md"]);

    world.local_commit("mine.txt", "mine");
    let diverged = update::apply(&host, &args("alpha")).unwrap_err();
    assert!(diverged
        .message
        .starts_with("your branch diverged: 1 local commit(s) and 1 new"));
    git(&world.work, &["reset", "-q", "--hard", &before]);

    git(&world.work, &["checkout", "-q", "--detach"]);
    let detached = update::apply(&host, &args("alpha")).unwrap_err();
    assert!(detached.message.contains("HEAD is detached"));
    assert_eq!(world.head(), before);
    let plan = update::plan(&host, &args("alpha"), false).unwrap();
    assert_eq!(plan.refusal.unwrap().code, "detached");
}

#[test]
fn test23_a_plan_that_changed_after_it_was_reviewed_is_not_applied() {
    let world = World::new("hash");
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let host = world.alpha();
    let reviewed = update::plan(&host, &args("alpha"), false).unwrap().hash;
    world.upstream_commit("three.txt", "three");
    world.fetch();
    let before = world.head();
    let stale = update::apply(
        &host,
        &UpdateArgs {
            plan_hash: Some(reviewed),
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap_err();
    assert!(stale.message.contains("the plan has changed"));
    assert_eq!(world.head(), before);
    let fresh = update::plan(&host, &args("alpha"), false).unwrap().hash;
    update::apply(
        &host,
        &UpdateArgs {
            plan_hash: Some(fresh),
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
}

#[test]
fn test24_an_up_to_date_checkout_is_a_calm_no_op() {
    let world = World::new("noop");
    let done = update::apply(&world.alpha(), &args("alpha")).unwrap();
    assert!(!done.moved && done.build.is_none() && done.restart.is_none());
    assert!(done.human("Updated").contains("already up to date"));
    let missing = update::apply(&world.alpha(), &args("ghost")).unwrap_err();
    assert_eq!(missing.message, "no server 'ghost'");
    let mut remote = world.server("srv-http", "remote");
    remote.extra.insert("transport".into(), json!("http"));
    let none = update::apply(&world.host(vec![remote]), &args("remote")).unwrap_err();
    assert!(none.message.contains("does not run from a git checkout"));
}

#[test]
fn test30_the_build_runs_in_the_checkout_and_is_remembered_for_the_next_update() {
    let world = World::new("build");
    let host = world.alpha();
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let first = update::apply(
        &host,
        &UpdateArgs {
            build: Some("echo built > built.txt; echo hello".into()),
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    let run = first.build.unwrap();
    assert!(run.ok && run.output.contains("hello"));
    assert_eq!(
        std::fs::read_to_string(world.work.join("built.txt"))
            .unwrap()
            .trim(),
        "built"
    );
    let store = store::RepoStore::read(&world.data).unwrap();
    assert_eq!(
        store.remembered("srv-alpha"),
        Some("echo built > built.txt; echo hello")
    );

    world.upstream_commit("three.txt", "three");
    world.fetch();
    let plan = update::plan(&host, &args("alpha"), false).unwrap();
    assert!(plan.build.remembered);
    assert_eq!(
        plan.build.command.as_deref(),
        Some("echo built > built.txt; echo hello")
    );
    std::fs::remove_file(world.work.join("built.txt")).unwrap();
    let second = update::apply(
        &host,
        &UpdateArgs {
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    assert!(second.build.unwrap().ok, "no flag runs what was agreed to");
    assert!(world.work.join("built.txt").exists());
}

#[test]
fn test31_a_different_command_is_a_new_yes_and_no_build_changes_only_the_files() {
    let world = World::new("newcmd");
    let host = world.alpha();
    world.upstream_commit("two.txt", "two");
    world.fetch();
    update::apply(
        &host,
        &UpdateArgs {
            build: Some("true".into()),
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    world.upstream_commit("three.txt", "three");
    world.fetch();
    let changed = update::plan(
        &host,
        &UpdateArgs {
            build: Some("echo other".into()),
            ..args("alpha")
        },
        false,
    )
    .unwrap();
    assert!(
        !changed.build.remembered,
        "the person has to agree to the new text"
    );
    let skipped = update::apply(
        &host,
        &UpdateArgs {
            no_build: true,
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    assert!(skipped.build.is_none());
    assert_eq!(
        store::RepoStore::read(&world.data)
            .unwrap()
            .remembered("srv-alpha"),
        Some("true"),
        "skipping once does not forget"
    );
}

#[test]
fn test32_a_failed_build_keeps_the_files_skips_the_restart_and_can_be_undone() {
    let world = World::new("badbuild");
    let host = world.alpha();
    let before = world.head();
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let done = update::apply(
        &host,
        &UpdateArgs {
            build: Some("echo compiling >&2; exit 3".into()),
            ..args("alpha")
        },
    )
    .unwrap();
    let run = done.build.as_ref().unwrap();
    assert!(!run.ok && run.code == 3 && run.output.contains("compiling"));
    assert!(done.failed());
    assert_eq!(done.restart.as_ref().unwrap().0, "skipped");
    assert_ne!(world.head(), before);
    assert_eq!(
        store::RepoStore::read(&world.data)
            .unwrap()
            .remembered("srv-alpha"),
        None
    );
    update::undo(
        &host,
        &UpdateArgs {
            no_build: true,
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    assert_eq!(world.head(), before);
}

#[test]
fn test33_a_build_that_overruns_is_stopped_and_secrets_stay_out_of_its_output() {
    let world = World::new("slow");
    let host = world.alpha();
    let timed = update::run_build(&host, &world.work, "sleep 5", Duration::from_millis(200));
    assert!(!timed.ok && timed.output.contains("timed out"));
    std::fs::write(
        world.work.join("log.txt"),
        "https://user:hunter2@git.example.invalid/x.git\ntoken=fake-secret-value-123\n",
    )
    .unwrap();
    let leaked = update::run_build(&host, &world.work, "cat log.txt", Duration::from_secs(10));
    assert!(
        leaked.ok && !leaked.output.contains("hunter2") && !leaked.output.contains("fake-secret"),
        "{}",
        leaked.output
    );
}

#[test]
fn test40_undo_stops_when_the_branch_moved_or_has_local_changes() {
    let world = World::new("undoguard");
    let host = world.alpha();
    world.upstream_commit("two.txt", "two");
    world.fetch();
    update::apply(
        &host,
        &UpdateArgs {
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    let done_at = world.head();
    std::fs::write(world.work.join("README.md"), "edited").unwrap();
    let dirty = update::undo(&host, &args("alpha")).unwrap_err();
    assert_eq!(dirty.details.unwrap()["reason"], "dirty");
    git(&world.work, &["checkout", "-q", "--", "README.md"]);
    world.local_commit("mine.txt", "mine");
    let moved = update::undo(&host, &args("alpha")).unwrap_err();
    assert!(moved.message.contains("has moved on since the update"));
    assert!(update::undo_plan(&host, "alpha").unwrap().refusal.is_some());
    assert_ne!(world.head(), done_at);
    assert!(world.work.join("mine.txt").exists());
}

struct Gateway {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Gateway {
    fn start(base: &Path, gateway: Arc<MemoryGateway>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (base, flag) = (base.to_path_buf(), Arc::clone(&stop));
        let thread = std::thread::spawn(move || {
            let claims = restart::Claims::default();
            while !flag.load(Ordering::SeqCst) {
                let router: Arc<dyn GatewayRouter> = gateway.clone();
                for work in restart::serve_with(&base, router, 4242, &claims) {
                    work.join().unwrap();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn test50_the_restart_goes_through_the_gateway_and_waits_while_a_call_is_running() {
    let world = World::new("restart");
    let host = world.alpha();
    let gateway = Arc::new(MemoryGateway::holding(&[("srv-alpha", 1)]));
    let _running = Gateway::start(&world.data, Arc::clone(&gateway));
    world.upstream_commit("two.txt", "two");
    world.fetch();

    let plan = update::plan(&host, &args("alpha"), true).unwrap();
    assert_eq!(plan.in_flight, Some(1));
    assert!(plan.human().contains("1 call(s) are running"));

    let busy = update::apply(&host, &args("alpha")).unwrap();
    assert!(busy.moved);
    let (status, detail) = busy.restart.unwrap();
    assert_eq!(status, "busy");
    assert!(detail.contains("toolportctl server restart alpha"));
    assert!(
        gateway.restarted().is_empty(),
        "a running call is never cut by default"
    );

    world.upstream_commit("three.txt", "three");
    world.fetch();
    let forced = update::apply(
        &host,
        &UpdateArgs {
            restart: RestartChoice::Now,
            ..args("alpha")
        },
    )
    .unwrap();
    assert_eq!(forced.restart.unwrap().0, "restarted");
    assert_eq!(gateway.restarted(), ["srv-alpha"]);

    gateway.held.lock().unwrap().insert("srv-alpha".into(), 0);
    world.upstream_commit("four.txt", "four");
    world.fetch();
    let idle = update::apply(&host, &args("alpha")).unwrap();
    assert_eq!(idle.restart.unwrap().0, "restarted");
    assert_eq!(gateway.restarted().len(), 2);
}

#[test]
fn test51_with_no_gateway_running_the_update_says_so_and_the_plan_asks_nobody_to_wait() {
    let world = World::new("nogateway");
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let host = world.alpha();
    let plan = update::plan(&host, &args("alpha"), true).unwrap();
    assert_eq!(plan.in_flight, None);
    let done = update::apply(&host, &args("alpha")).unwrap();
    assert_eq!(done.restart.as_ref().unwrap().0, "not_running");
    assert!(!done.failed());
    let value: Value = done.to_value();
    assert_eq!(value["undo"], "toolportctl server repo undo alpha");
}

fn rows_of(host: &MemoryHost) -> Vec<crate::attention::Item> {
    crate::attention::snapshot(host)
        .items
        .into_iter()
        .filter(|item| item.id.starts_with("updates:server"))
        .collect()
}

#[test]
fn test60_attention_check_fetches_the_checkout_and_a_skippable_row_appears() {
    let world = World::new("row");
    let host = world.alpha();
    world.upstream_commit("two.txt", "two");
    assert!(
        rows_of(&host).is_empty(),
        "nothing is known before the fetch"
    );
    let before = world.head();
    crate::attention::ops::check(&host).unwrap();
    assert_eq!(world.head(), before);
    let rows = rows_of(&host);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.title, "alpha: 1 new commit on main");
    assert_eq!(row.urgency, crate::attention::Urgency::WorthALook);
    assert!(row.id.starts_with("updates:server:alpha:") && row.id.contains('>'));
    assert!(row.skip.as_deref().unwrap().starts_with("Skip "));
    let command = row
        .action
        .as_ref()
        .unwrap()
        .command
        .as_ref()
        .unwrap()
        .join(" ");
    assert_eq!(command, "toolportctl server repo update alpha --dry-run");
    let state = crate::attention::store::Store::read(&world.data).unwrap();
    assert!(state.updates.sources.contains_key("server:alpha"));
}

#[test]
fn test61_local_changes_and_a_diverged_branch_are_calm_rows_that_cannot_be_skipped() {
    let world = World::new("calm");
    let host = world.alpha();
    world.upstream_commit("two.txt", "two");
    crate::attention::ops::check(&host).unwrap();
    std::fs::write(world.work.join("README.md"), "edited").unwrap();
    let dirty = rows_of(&host);
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].title, "alpha: local changes block the update");
    assert!(dirty[0].skip.is_none() && dirty[0].id.starts_with("updates:server-blocked:alpha:"));
    git(&world.work, &["checkout", "-q", "--", "README.md"]);
    world.local_commit("mine.txt", "mine");
    let diverged = rows_of(&host);
    assert_eq!(diverged[0].title, "alpha: your branch diverged");
    assert!(diverged[0]
        .detail
        .contains("1 local commit(s) and 1 new commit(s) on origin/main"));
    assert!(diverged[0].skip.is_none());
}

#[test]
fn test62_a_checkout_that_is_up_to_date_has_no_row_and_servers_sharing_it_are_named() {
    let world = World::new("quiet");
    let host = world.host(vec![
        world.server("srv-a", "alpha"),
        world.server("srv-b", "beta"),
    ]);
    crate::attention::ops::check(&host).unwrap();
    assert!(rows_of(&host).is_empty());
    world.upstream_commit("two.txt", "two");
    crate::attention::ops::check(&host).unwrap();
    let rows = rows_of(&host);
    assert_eq!(rows.len(), 1, "one checkout is one row");
    assert!(rows[0].detail.contains("beta runs from the same checkout"));
}

#[test]
fn test63_an_unreachable_remote_is_reported_by_the_calm_offline_row_not_a_server_row() {
    let world = World::new("offline");
    let host = world.alpha();
    git(
        &world.work,
        &[
            "remote",
            "set-url",
            "origin",
            "/nonexistent/path/remote.git",
        ],
    );
    crate::attention::ops::check(&host).unwrap();
    assert!(rows_of(&host).is_empty());
    let all = crate::attention::snapshot(&host).items;
    assert!(all.iter().any(|item| item.id == "updates:unreachable"));
}

const FAKE: &str = "fake-secret-value-123";

fn tree_text(dir: &Path) -> String {
    let mut text = String::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return text;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if path.is_dir() {
            text.push_str(&tree_text(&path));
        } else if let Ok(body) = std::fs::read_to_string(&path) {
            text.push_str(&body);
        }
    }
    text
}

fn secret_commands() -> Vec<String> {
    vec![
        format!("make --token={FAKE}"),
        format!("make --password {FAKE}"),
        format!("make --api-key='{FAKE}'"),
        format!("MY_API_KEY={FAKE} make"),
        format!("make DB_PASSWORD=\"{FAKE}\""),
        format!("curl -H 'Authorization: Bearer {FAKE}' https://example.invalid/x"),
        format!("curl -H \"X-Api-Key: {FAKE}\" https://example.invalid/x"),
        format!("git clone https://builder:{FAKE}@example.invalid/x.git"),
        "make --id Zk3Qp9Xw2Lm8Vb5Nc7Rt1Yh4Gd6Fs0Jq".to_string(),
        "make --id=\"aB3dE5fG7hJ9kL2mN4pQ6rS8tV0wX1yZ\"".to_string(),
    ]
}

#[test]
fn test34_a_build_command_with_a_secret_is_refused_and_leaves_no_trace() {
    let world = World::new("screen");
    let host = world.alpha();
    host.tasks
        .known_secrets
        .borrow_mut()
        .push("vault-held-value-77".into());
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let before = world.head();
    let mut commands = secret_commands();
    commands.push("make --flavor vault-held-value-77".into());
    let mut seen = String::new();
    for command in &commands {
        if !command.contains("builder:") && !command.contains("vault-held") {
            assert!(screen::pattern_reason(command).is_some(), "{command}");
        }
        let build = Some(command.clone());
        let refused = update::plan(
            &host,
            &UpdateArgs {
                build: build.clone(),
                ..args("alpha")
            },
            false,
        )
        .unwrap_err();
        assert_eq!(refused.code(), "build_refused", "{command}");
        assert!(refused
            .message
            .contains("put the secret in the server's env"));
        let apply = update::apply(
            &host,
            &UpdateArgs {
                build,
                restart: RestartChoice::Skip,
                ..args("alpha")
            },
        )
        .unwrap_err();
        assert_eq!(apply.code(), "build_refused", "{command}");
        seen.push_str(&format!("{refused:?}{apply:?}"));
    }
    assert_eq!(world.head(), before, "a refused command changes nothing");
    assert!(!world.data.join("plus").join("server-repos.json").exists());
    let all = format!("{seen}{}", tree_text(&world.base));
    for needle in [FAKE, "vault-held-value-77", "Zk3Qp9Xw2Lm8", "aB3dE5fG7hJ9"] {
        assert!(!all.contains(needle), "{needle} leaked");
    }
}

#[test]
fn test35_plain_commands_and_variable_references_pass_the_pattern_screen() {
    let world = World::new("refs");
    let host = world.alpha();
    for command in ["npm ci && npm run build", "cargo build --release --locked"] {
        assert_eq!(screen::refusal(&host, command), None, "{command}");
    }
    for command in [
        "make --token=$BUILD_TOKEN",
        "make --password \"$DB_PASSWORD\"",
        "API_TOKEN=${API_TOKEN} make",
        "curl -H \"Authorization: Bearer $TOKEN\" https://example.invalid/x",
    ] {
        assert_eq!(screen::pattern_reason(command), None, "{command}");
    }
}

#[test]
fn test36_a_remembered_command_that_now_carries_a_secret_is_not_run_until_replaced() {
    let world = World::new("stale");
    let host = world.alpha();
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let mut stored = store::RepoStore::default();
    stored.builds.insert(
        "srv-alpha".into(),
        store::Build {
            command: format!("make --token={FAKE}"),
            at: 1,
        },
    );
    stored.write(&world.data).unwrap();
    let plan = update::plan(&host, &args("alpha"), false).unwrap();
    assert!(plan.build.command.is_none() && plan.build.blocked.is_some());
    let shown = format!("{}{}", plan.human(), plan.to_value());
    assert!(!shown.contains(FAKE), "{shown}");
    let refused = update::apply(
        &host,
        &UpdateArgs {
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap_err();
    assert_eq!(refused.code(), "build_refused");
    assert!(!refused.message.contains(FAKE));
    let skipped = update::apply(
        &host,
        &UpdateArgs {
            no_build: true,
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    assert!(skipped.build.is_none() && skipped.moved);
}

#[test]
fn test37_an_undo_that_would_rebuild_with_a_secret_asks_for_no_build_and_prints_nothing() {
    let world = World::new("undo-secret");
    let host = world.alpha();
    let before = world.head();
    world.upstream_commit("two.txt", "two");
    world.fetch();
    update::apply(
        &host,
        &UpdateArgs {
            build: Some("true".into()),
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    let mut stored = store::RepoStore::read(&world.data).unwrap();
    stored.applied.get_mut("srv-alpha").unwrap().build = Some(format!("make --token={FAKE}"));
    stored.write(&world.data).unwrap();
    let plan = update::undo_plan(&host, "alpha").unwrap();
    assert!(plan.blocked.is_some());
    assert!(!format!("{}{}", plan.human(), plan.to_value()).contains(FAKE));
    let refused = update::undo(
        &host,
        &UpdateArgs {
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap_err();
    assert_eq!(refused.code(), "build_refused");
    assert_ne!(world.head(), before, "nothing moved");
    update::undo(
        &host,
        &UpdateArgs {
            no_build: true,
            restart: RestartChoice::Skip,
            ..args("alpha")
        },
    )
    .unwrap();
    assert_eq!(world.head(), before);
    assert!(!tree_text(&world.base).contains(FAKE));
}

#[test]
fn test38_what_a_build_prints_is_masked_for_vault_values_and_inline_secrets() {
    let world = World::new("mask");
    let host = world.alpha();
    host.tasks
        .known_secrets
        .borrow_mut()
        .push("vault-held-value-77".into());
    let text = format!(
        "ok vault-held-value-77\nexport DB_PASSWORD={FAKE}\nAuthorization: Bearer {FAKE}\nBuilt 9be07d2a4c1f0e5d8b7a6c3f2e1d0c9b8a7f6e5d\nplain line"
    );
    let out = screen::shown(&host, &text);
    for needle in ["vault-held-value-77", FAKE] {
        assert!(!out.contains(needle), "{out}");
    }
    assert!(
        out.contains("plain line") && out.contains("9be07d2a4c1f"),
        "{out}"
    );
}

#[test]
fn test39_the_command_line_refuses_a_secret_in_every_form_and_prints_none_of_it() {
    let world = World::new("cli");
    let host = world.alpha();
    world.upstream_commit("two.txt", "two");
    world.fetch();
    let mut printed = String::new();
    for command in secret_commands() {
        for extra in [&["--dry-run"][..], &[][..]] {
            let mut argv: Vec<String> = ["--json", "server", "repo", "update", "alpha"]
                .iter()
                .map(|a| a.to_string())
                .collect();
            argv.extend(extra.iter().map(|a| a.to_string()));
            argv.extend(["--no-restart".into(), "--build".into(), command.clone()]);
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = crate::ctl::run_with(&host, &argv, &mut out, &mut err);
            assert_eq!(code, 1, "{command}");
            printed.push_str(&String::from_utf8_lossy(&out));
            printed.push_str(&String::from_utf8_lossy(&err));
        }
    }
    assert!(printed.contains("build_refused"), "{printed}");
    let all = format!("{printed}{}", tree_text(&world.base));
    assert!(
        !all.contains(FAKE) && !all.contains("Zk3Qp9Xw2Lm8"),
        "{all}"
    );
}
