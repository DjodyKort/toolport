#![allow(dead_code)]

//! Fixtures that the contract tests add to a `CtlWorld`: local git repositories (no network),
//! an mcpm config root and Claude Code transcripts. Nothing in them is a real credential.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::ctl_world::{read_json, write_json, CtlWorld};

pub const PASSPHRASE: &str = "FAKE-sync-passphrase-31d8";

pub fn git(dir: &Path, home: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

/// `{base}/tap-src` is a local skills repository with one commit (a tap, a git-sync source),
/// `{base}/remote.git` an empty bare repository (the sync remote) and `{home}/a.txt` the file a
/// sync project tracks: no network is involved.
pub fn git_world(world: &CtlWorld) {
    let tap = world.base.join("tap-src");
    let skill = tap.join("skills/tapskill/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        skill,
        "---\nname: tapskill\ndescription: A synthetic tap skill\n---\nTap body\n",
    )
    .unwrap();
    git(&tap, &world.home, &["init", "-q"]);
    git(
        &tap,
        &world.home,
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    git(&tap, &world.home, &["add", "-A"]);
    git(&tap, &world.home, &["commit", "-q", "-m", "fixture"]);
    let remote = world.base.join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &world.home, &["init", "-q", "--bare"]);
    git(
        &remote,
        &world.home,
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    std::fs::write(world.home.join("a.txt"), "tracked by the sync project\n").unwrap();
}

/// An mcpm config root with two servers, a tool list and a file that refers to a renamed tool.
pub fn import_world(world: &CtlWorld) {
    let mock = &world.mock;
    write_json(
        &world.base.join("mcpm/servers.json"),
        &json!({
            "alpha-mock": {"name": "alpha-mock", "profile_tags": ["smoke"],
                           "command": mock, "args": []},
            "beta-mock": {"name": "beta-mock", "profile_tags": ["smoke"],
                          "command": mock, "args": []}
        }),
    );
    write_json(
        &world.base.join("tools.json"),
        &json!({"alpha-mock": ["echo", "add"], "beta-mock": ["echo"]}),
    );
    let refs = world.base.join("refs");
    std::fs::create_dir_all(&refs).unwrap();
    std::fs::write(
        refs.join("note.md"),
        "Use mcp__mcpm_alpha-mock__echo to echo.\n",
    )
    .unwrap();
}

/// Claude Code transcripts with billed usage, for `usage` and `compression verify`.
pub fn transcripts_world(world: &CtlWorld) {
    let dir = world.home.join(".claude/projects/demo");
    std::fs::create_dir_all(&dir).unwrap();
    let lines: Vec<String> = (1..=6)
        .map(|n| {
            let message = |tools: Value| -> Value {
                json!({
                    "type": "assistant",
                    "uuid": format!("u-m{n}"),
                    "sessionId": "s1",
                    "cwd": "/work/demo",
                    "timestamp": format!("2026-10-01T10:00:0{n}Z"),
                    "message": {
                        "id": format!("m{n}"),
                        "role": "assistant",
                        "model": "claude-a",
                        "content": tools,
                        "usage": {
                            "input_tokens": 10,
                            "output_tokens": 40,
                            "cache_creation_input_tokens": 500,
                            "cache_read_input_tokens": 7000,
                            "service_tier": "standard"
                        }
                    }
                })
            };
            let content = if n % 2 == 0 {
                json!([{"type": "tool_use", "id": format!("t{n}"), "name": "mcp__github__list", "input": {}}])
            } else {
                json!([{"type": "text", "text": "synthetic"}])
            };
            message(content).to_string()
        })
        .collect();
    std::fs::write(dir.join("s1.jsonl"), lines.join("\n") + "\n").unwrap();
}

/// The skills repository of the world is a git clone of `{base}/skills-remote.git` with one pushed
/// commit and one file that is not committed yet, for the tools that commit and push it.
pub fn skills_repo_remote_world(world: &CtlWorld) {
    let remote = world.base.join("skills-remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &world.home, &["init", "-q", "--bare"]);
    git(
        &remote,
        &world.home,
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    let repo = &world.repo;
    git(repo, &world.home, &["init", "-q"]);
    git(
        repo,
        &world.home,
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    git_identity(repo, &world.home);
    git(
        repo,
        &world.home,
        &["remote", "add", "origin", &world.path(&remote)],
    );
    git(repo, &world.home, &["add", "-A"]);
    git(repo, &world.home, &["commit", "-q", "-m", "fixture"]);
    git(repo, &world.home, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(repo.join("skills/demo/extra.md"), "not committed yet\n").unwrap();
}

/// The server `forked` is a git checkout (`{base}/fork-work`, one local commit ahead) of a bare
/// repository (`{base}/upstream.git`, also its `upstream` remote) that has gained a commit.
pub fn fork_world(world: &CtlWorld) {
    let upstream = world.base.join("upstream.git");
    std::fs::create_dir_all(&upstream).unwrap();
    git(
        &upstream,
        &world.home,
        &["init", "-q", "--bare", "-b", "main"],
    );
    let work = world.base.join("fork-work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &world.home, &["init", "-q", "-b", "main"]);
    git_identity(&work, &world.home);
    std::fs::write(work.join("a.txt"), "a\n").unwrap();
    git(&work, &world.home, &["add", "-A"]);
    git(&work, &world.home, &["commit", "-q", "-m", "base"]);
    for remote in ["origin", "upstream"] {
        git(
            &work,
            &world.home,
            &["remote", "add", remote, &world.path(&upstream)],
        );
    }
    git(&work, &world.home, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(work.join("local.txt"), "mine\n").unwrap();
    git(&work, &world.home, &["add", "-A"]);
    git(&work, &world.home, &["commit", "-q", "-m", "local change"]);

    let other = world.base.join("fork-other");
    git(
        &world.base,
        &world.home,
        &["clone", "-q", &world.path(&upstream), &world.path(&other)],
    );
    git_identity(&other, &world.home);
    std::fs::write(other.join("b.txt"), "b\n").unwrap();
    git(&other, &world.home, &["add", "-A"]);
    git(
        &other,
        &world.home,
        &["commit", "-q", "-m", "upstream change"],
    );
    git(&other, &world.home, &["push", "-q", "origin", "main"]);

    let path = world.data.join("registry.json");
    let mut registry = read_json(&path);
    registry["servers"].as_array_mut().unwrap().push(json!({
        "id": "srv-fork", "name": "forked", "transport": "stdio", "command": world.mock,
        "args": [],
        "mcpmSource": {"type": "git", "path": world.path(&work), "branch": "main"}
    }));
    write_json(&path, &registry);
}

/// Back to `main` in the fork checkout, as a user would between two fork syncs.
pub fn fork_back_to_main(world: &CtlWorld) {
    let work = world.base.join("fork-work");
    git(&work, &world.home, &["checkout", "-q", "main"]);
}

fn git_identity(dir: &Path, home: &Path) {
    for (key, value) in [
        ("user.name", "fixture"),
        ("user.email", "fixture@example.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        git(dir, home, &["config", key, value]);
    }
}

/// Runs the real `toolportctl` against the world, for the setup that a test needs before the
/// program under test (the self-MCP server has no `sync init`).
pub fn ctl(world: &CtlWorld, argv: &[&str], stdin: Option<&str>) {
    use std::io::Write;
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .arg("--json")
        .args(argv)
        .env_clear()
        .current_dir(&world.home)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in world.env() {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawn toolportctl");
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().expect("wait for toolportctl");
    assert!(
        output.status.success(),
        "toolportctl {argv:?}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// The sync of the world points at the local remote and tracks one project file.
pub fn sync_world(world: &CtlWorld) {
    git_world(world);
    sync_setup(world);
}

pub fn sync_setup(world: &CtlWorld) {
    ctl(
        world,
        &[
            "sync",
            "init",
            "--repo",
            &world.path(&world.base.join("remote.git")),
            "--machine-id",
            "m-test",
            "--passphrase-stdin",
        ],
        Some(PASSPHRASE),
    );
    ctl(
        world,
        &[
            "sync",
            "add-project",
            &world.path(&world.home),
            "--name",
            "proj1",
            "--files",
            "a.txt",
        ],
        None,
    );
}

const HEALTH: &str = r#"{"ready":true,"config":{"max_items_after_crush":50,"protect_recent":null,"accuracy_guard":true}}"#;

/// Answers `GET /health` on a local port for the rest of the test process, like the compression
/// proxy does, so that the commands that read the live posture have one to read.
pub fn health_proxy(port: u16) {
    static STARTED: Mutex<Vec<u16>> = Mutex::new(Vec::new());
    let mut started = STARTED.lock().unwrap();
    if started.contains(&port) {
        return;
    }
    let listener = TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("port {port} is needed for the fake proxy: {e}"));
    started.push(port);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{HEALTH}",
                HEALTH.len()
            );
        }
    });
}
