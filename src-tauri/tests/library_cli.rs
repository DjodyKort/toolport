//! The skills library as a source (MIG-SRC-3), through the real `toolportctl`: the network is
//! touched only with `--fetch` (a `git` wrapper on PATH fails every network transport and logs
//! each call), and the credential in a remote URL is shown in no envelope, human line or error,
//! whether the transport works (`insteadOf` rewrites it to a local bare repository) or fails and
//! git's own message carries the URL.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

#[path = "common/claude_stub.rs"]
mod claude_stub;
#[path = "common/ctl_fixtures.rs"]
mod ctl_fixtures;
#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/library_world.rs"]
mod library_world;
#[path = "common/loads_world.rs"]
mod loads_world;

use ctl_world::CtlWorld;

const CREDENTIAL: &str = "FAKE-library-credential-7c2d";
const NETWORK: [&str; 5] = ["fetch", "ls-remote", "pull", "push", "clone"];

fn world(tag: &str) -> CtlWorld {
    let world = CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"));
    library_world::library_home(&world);
    world
}

fn credentialed_url() -> String {
    format!("https://library-user:{CREDENTIAL}@library.example.invalid/org/skills.git")
}

/// The remote URL carries a credential; `insteadOf` sends the transport to the local bare repository.
fn credentialed_remote(world: &CtlWorld) {
    let lib = library_world::library(world);
    let url = credentialed_url();
    let remote = world.path(&library_world::remote(world));
    ctl_fixtures::git(&lib, &world.home, &["remote", "set-url", "origin", &url]);
    ctl_fixtures::git(
        &lib,
        &world.home,
        &["config", "--local", &format!("url.{remote}.insteadOf"), &url],
    );
}

struct Wrapped {
    dir: PathBuf,
    log: PathBuf,
}

/// A `git` first on PATH that logs its arguments and fails network subcommands the way a refused
/// connection does: with the credentialed URL in its message.
fn wrapper(world: &CtlWorld) -> Wrapped {
    let dir = world.base.join("wrapper-bin");
    let log = world.base.join("git-calls.log");
    std::fs::create_dir_all(&dir).unwrap();
    let message = format!(
        "fatal: unable to access '{}': Could not resolve host: library.example.invalid",
        credentialed_url()
    );
    exec::write_executable(
        &dir.join("git"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nfor a in \"$@\"; do\n  case \"$a\" in\n    fetch|ls-remote|pull|push|clone) echo \"{message}\" >&2; exit 128 ;;\n  esac\ndone\nexec /usr/bin/git \"$@\"\n",
            log.display()
        ),
    );
    Wrapped { dir, log }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(world: &CtlWorld, wrapped: Option<&Wrapped>, args: &[&str]) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .args(args)
        .env_clear()
        .current_dir(&world.home)
        .stdin(Stdio::null());
    for (key, value) in world.env() {
        let value = match (key, wrapped) {
            ("PATH", Some(w)) => format!("{}:{value}", w.dir.display()),
            _ => value,
        };
        command.env(key, value);
    }
    let out = command.output().expect("run toolportctl");
    Run {
        code: out.status.code().expect("exit code"),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn envelope(run: &Run) -> Value {
    serde_json::from_str(run.stdout.lines().next().expect("an envelope line")).unwrap()
}

fn logged(wrapped: &Wrapped) -> Vec<String> {
    std::fs::read_to_string(&wrapped.log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn network_calls(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.split_whitespace().any(|word| NETWORK.contains(&word)))
        .cloned()
        .collect()
}

#[test]
fn status_and_push_dry_run_run_no_network_transport_without_fetch() {
    let world = world("library-offline");
    library_world::ahead_one(&world);
    let wrapped = wrapper(&world);

    let status = run(&world, Some(&wrapped), &["--json", "library", "status"]);
    assert_eq!(status.code, 0, "{}", status.stdout);
    let data = envelope(&status)["data"].clone();
    assert_eq!(data["ahead"], 1);
    assert_eq!(data["fetch"]["requested"], false);
    let push = run(
        &world,
        Some(&wrapped),
        &["--json", "library", "push", "--dry-run"],
    );
    assert_eq!(push.code, 0, "{}", push.stdout);
    let calls = logged(&wrapped);
    assert!(
        !calls.is_empty(),
        "the wrapper must have seen the git calls, or the check proves nothing"
    );
    assert_eq!(
        network_calls(&calls),
        Vec::<String>::new(),
        "no network transport without --fetch"
    );

    let fetched = run(
        &world,
        Some(&wrapped),
        &["--json", "library", "status", "--fetch"],
    );
    assert_eq!(fetched.code, 0, "{}", fetched.stdout);
    let data = envelope(&fetched)["data"].clone();
    assert_eq!(data["fetch"]["requested"], true);
    assert_eq!(data["fetch"]["ok"], false);
    assert_eq!(data["fetch"]["error"], "network unreachable");
    assert!(
        logged(&wrapped)
            .iter()
            .any(|line| line.split_whitespace().any(|word| word == "ls-remote")),
        "--fetch probes the remote with ls-remote"
    );
}

#[test]
fn a_pull_preview_fetches_refs_and_says_so() {
    let world = world("library-preview");
    library_world::behind_two(&world);
    let wrapped = wrapper(&world);
    let preview = run(
        &world,
        Some(&wrapped),
        &["--json", "library", "pull", "--dry-run"],
    );
    assert_eq!(preview.code, 0, "{}", preview.stdout);
    let data = envelope(&preview)["data"].clone();
    assert_eq!(data["dryRun"], true);
    assert!(
        !data["plan"]["warnings"].as_array().unwrap().is_empty(),
        "a failed fetch is a plan warning: {data}"
    );
    assert!(
        network_calls(&logged(&wrapped))
            .iter()
            .any(|line| line.split_whitespace().any(|word| word == "fetch")),
        "the preview fetches refs"
    );
}

#[test]
fn no_output_of_any_library_command_carries_the_remote_credential() {
    for failing in [false, true] {
        let world = world(if failing {
            "library-credential-failing"
        } else {
            "library-credential-working"
        });
        library_world::ahead_one(&world);
        credentialed_remote(&world);
        let wrapped = failing.then(|| wrapper(&world));
        let mut seen = 0;
        for args in [
            &["library", "status"][..],
            &["library", "status", "--fetch"],
            &["library", "pull", "--dry-run"],
            &["library", "pull"],
            &["library", "push", "--dry-run"],
            &["library", "push"],
            &["library"],
        ] {
            for json in [true, false] {
                let mut argv: Vec<&str> = if json { vec!["--json"] } else { vec![] };
                argv.extend(args);
                let out = run(&world, wrapped.as_ref(), &argv);
                seen += 1;
                for text in [&out.stdout, &out.stderr] {
                    for secret in [CREDENTIAL, "library-user"] {
                        assert!(
                            !text.contains(secret),
                            "{argv:?} (failing transport: {failing}) printed the credential: {text}"
                        );
                    }
                }
                if json && args.get(1) == Some(&"status") {
                    let data = envelope(&out)["data"].clone();
                    assert_eq!(
                        data["remote"].as_str().map(|r| r.contains("library.example.invalid")),
                        Some(true),
                        "the remote is still shown, without its userinfo: {data}"
                    );
                }
            }
        }
        assert_eq!(seen, 14);
        let fetched = run(&world, wrapped.as_ref(), &["--json", "library", "status", "--fetch"]);
        assert_eq!(
            envelope(&fetched)["data"]["fetch"]["ok"],
            !failing,
            "the transport works exactly when it is not the failing wrapper"
        );
        assert_tree_has_no_credential_outside_git(&world.base);
    }
}

fn assert_tree_has_no_credential_outside_git(root: &Path) {
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == ".git" || name == "wrapper-bin") {
                continue;
            }
            assert_tree_has_no_credential_outside_git(&path);
        } else if path.file_name().is_some_and(|name| name != "git-calls.log") {
            let bytes = std::fs::read(&path).unwrap_or_default();
            assert!(
                !bytes
                    .windows(CREDENTIAL.len())
                    .any(|w| w == CREDENTIAL.as_bytes()),
                "{} stores the credential",
                path.display()
            );
        }
    }
}

#[test]
fn pull_refuses_a_dirty_clone_and_leaves_the_tree_as_it_was() {
    let world = world("library-dirty");
    library_world::behind_two(&world);
    library_world::dirty(&world);
    let lib = library_world::library(&world);
    let before = std::fs::read_to_string(lib.join("skills/review/SKILL.md")).unwrap();
    for args in [
        &["--json", "library", "pull"][..],
        &["--json", "library", "pull", "--dry-run"],
    ] {
        let out = run(&world, None, args);
        assert_eq!(out.code, 1, "{}", out.stdout);
        assert_eq!(envelope(&out)["error"]["code"], "refused");
        assert!(out.stderr.is_empty(), "{}", out.stderr);
    }
    assert_eq!(
        std::fs::read_to_string(lib.join("skills/review/SKILL.md")).unwrap(),
        before
    );
    assert!(lib.join("skills/draft/SKILL.md").is_file());
    assert!(!lib.join("skills/notes-one/SKILL.md").exists());
}

#[test]
fn push_refuses_a_secret_in_a_commit_and_pushes_nothing() {
    let world = world("library-secret");
    library_world::secret_commit(&world);
    let out = run(&world, None, &["--json", "library", "push"]);
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert_eq!(envelope(&out)["error"]["code"], "refused");
    for text in [&out.stdout, &out.stderr] {
        assert!(!text.contains(library_world::SECRET), "{text}");
    }
    let remote = world.path(&library_world::remote(&world));
    let heads = Command::new("/usr/bin/git")
        .args(["-C", &remote, "rev-list", "--count", "main"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&heads.stdout).trim(), "2");
}
