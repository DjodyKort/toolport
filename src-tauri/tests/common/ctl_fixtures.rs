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

/// The client-folder home of `common/loads_world.rs` (`context loads`).
pub fn loads_home(world: &CtlWorld) {
    crate::loads_world::build_in(
        &world.base,
        conduit_lib::plus::context::layers::MANAGED_LOCAL_HEADER,
    );
}

/// The same home with a `claude` that is the stream-json stub (`fixtures/loads/claude-stub.sh`)
/// and a user skill Claude Code does not list (`context measure`).
pub fn measure_home(world: &CtlWorld) {
    let loaded = crate::loads_world::build_in(
        &world.base,
        conduit_lib::plus::context::layers::MANAGED_LOCAL_HEADER,
    );
    crate::claude_stub::ClaudeStub::install(&world.claude, &world.base);
    let unlisted = loaded.claude.join("skills/handoff/SKILL.md");
    std::fs::create_dir_all(unlisted.parent().unwrap()).unwrap();
    std::fs::write(
        unlisted,
        "---\nname: handoff\ndescription: \"Write a handoff\nfor the next session\"\n---\nBody\n",
    )
    .unwrap();
}

pub const BUNDLE_ACME_DEV: &str = include_str!("../fixtures/bundles/acme-dev.yaml");
pub const BUNDLE_DEFAULT: &str = include_str!("../fixtures/bundles/default.yaml");
pub const BUNDLE_BROKEN: &str = "format: 1\nname: broken\nskills: 3\n";

/// A foreign `settings.local.json`: Claude Code wrote the permissions, the user an
/// `enabledPlugins` entry of their own.
pub const FOREIGN_SETTINGS: &str = "{\n  \"permissions\": {\n    \"allow\": [\"Bash(git status)\", \"Read(./docs/**)\"],\n    \"deny\": [\"Bash(rm -rf *)\"]\n  },\n  \"enabledPlugins\": {\n    \"user-notes@notes-market\": true\n  },\n  \"model\": \"sonnet\"\n}\n";

/// The client-folder home of `loads_world` plus bundles: the library holds `acme-dev`, the legacy
/// `default` list and a broken definition, the workspace repository above the client repository
/// has files of its own (nothing may be written there), the client repository holds a foreign
/// `settings.local.json` and a user-written `CLAUDE.local.md`, a second client repository starts
/// clean, and the registry has a server profile named like the bundle.
pub fn bundle_home(world: &CtlWorld) {
    let loaded = crate::loads_world::build_in(
        &world.base,
        conduit_lib::plus::context::layers::MANAGED_LOCAL_HEADER,
    );
    let put = |path: std::path::PathBuf, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let profiles = loaded.library.join("profiles");
    put(profiles.join("acme-dev.yaml"), BUNDLE_ACME_DEV);
    put(profiles.join("default.yaml"), BUNDLE_DEFAULT);
    put(profiles.join("broken.yaml"), BUNDLE_BROKEN);
    put(
        loaded.library.join("rules/acme-knowledge/SKILL.md"),
        "---\nname: acme-knowledge\ndescription: ERP knowledge\nactivation: always\n---\nPost invoices before closing the period.\n",
    );
    for name in ["notes-helper", "scratch-one", "scratch-two", "long-guide", "erp-core", "erp-reports"] {
        put(
            loaded.library.join(format!("skills/{name}/SKILL.md")),
            &format!("---\nname: {name}\ndescription: Synthetic {name}\n---\nBody of {name}.\n"),
        );
    }
    put(loaded.client.join(".claude/settings.local.json"), FOREIGN_SETTINGS);
    put(
        loaded.client.join("CLAUDE.local.md"),
        "# My notes\nKeep the invoices in order.\n",
    );
    let second = loaded.workspace.join("clients/acme-two");
    std::fs::create_dir_all(second.join(".git")).unwrap();
    put(second.join(".claude/settings.local.json"), FOREIGN_SETTINGS);

    let path = world.data.join("registry.json");
    let mut registry = read_json(&path);
    registry["profiles"].as_array_mut().unwrap().push(json!({
        "id": "acme-dev", "name": "acme-dev", "enabledServerIds": ["srv-alpha", "srv-beta"]
    }));
    write_json(&path, &registry);
}

/// `bundle_home` with `acme-dev` applied in both client repositories and the second one changed
/// since: a value Toolport wrote was edited and a foreign key was added.
pub fn bundle_drift_home(world: &CtlWorld) {
    bundle_home(world);
    let workspace = world.home.join("work/erp");
    for client in ["clients/acme-erp", "clients/acme-two"] {
        ctl(
            world,
            &[
                "context",
                "bundle",
                "apply",
                "acme-dev",
                "--cwd",
                &world.path(&workspace.join(client)),
            ],
            None,
        );
    }
    let file = workspace.join("clients/acme-two/.claude/settings.local.json");
    let mut settings = read_json(&file);
    settings["skillOverrides"]["scratch-one"] = json!("on");
    settings["effortLevel"] = json!("high");
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&settings).unwrap() + "\n",
    )
    .unwrap();
}

/// The home of the layer goldens (MIG-CTX-11): a user memory file the corporate clone provides, a
/// workspace memory, two client repositories and one repository outside the client tree, a
/// knowledge folder with an import chain of four hops and a cycle, and a layer of every kind: one
/// named after its client folder, one per scope, one that imports by `@path`, one with a cycle,
/// and a scaffold in the config that fills in `tree-knowledge`. Nothing is deployed yet.
pub fn layers_home(world: &CtlWorld) {
    let home = &world.home;
    let put = |rel: &str, text: &str| {
        let path = home.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let layer = |dir: &str, front: &str, body: &str| {
        put(
            &format!(".config/mcpm/skills_repo/rules/{dir}/SKILL.md"),
            &format!("---\nname: {dir}\ndescription: \"Synthetic {dir}\"\nactivation: always\n{front}---\n\n{body}\n"),
        );
    };
    put(".claude/CLAUDE.md", "# Corp rules\nBe brief.\n");
    put(".local/share/corp-tools/claude/CLAUDE.md", "# Corp rules\nBe brief.\n");
    put(
        ".config/mcpm/context.json",
        "{\n  \"clients_root\": \"~/work/erp/clients\",\n  \"corp_tools_dir\": \"~/.local/share/corp-tools\",\n  \"layerScaffolds\": {\n    \"tree-knowledge\": {\n      \"scope\": \"folder\",\n      \"folders\": [\"~/work/erp/clients/acme-two\", \"~/work/other\"],\n      \"imports\": [\"~/kb/CLAUDE.md\"]\n    }\n  }\n}\n",
    );
    put("work/erp/CLAUDE.md", "# Workspace\nUse the shared test database.\n");
    for repo in ["work/erp/clients/acme-erp", "work/erp/clients/acme-two", "work/other"] {
        std::fs::create_dir_all(home.join(repo).join(".git/info")).unwrap();
    }
    put("kb/CLAUDE.md", "# Tree knowledge\nPost invoices before closing the period.\n");
    put("kb/chain/hop-1.md", "Hop one: the chart of accounts.\n@hop-2.md\n");
    put("kb/chain/hop-2.md", "Hop two: the tax codes.\n@hop-3.md\n");
    put("kb/chain/hop-3.md", "Hop three: the journals.\n@hop-4.md\n");
    put("kb/chain/hop-4.md", "Hop four: the period locks.\n");
    put("kb/cycle-a.md", "Cycle a.\n@cycle-b.md\n");
    put("kb/cycle-b.md", "Cycle b.\n@cycle-a.md\n");
    layer("personal", "", "## Personal preferences\n\nKeep answers short.");
    layer("team-conventions", "scope: global\n", "Commit messages say why.");
    layer("client-acme-erp", "globs: \"**/clients/acme-erp/**\"\n", "## acme-erp\n\nThe fiscal year starts in April.");
    layer(
        "client-erp-knowledge",
        "scope: folder\nfolders: [\"~/work/erp/clients/acme-erp\", \"~/work/other\"]\nimports: [\"~/kb/CLAUDE.md\"]\n",
        "## ERP knowledge",
    );
    layer(
        "client-chain",
        "scope: folder\nfolders: [\"~/work/erp/clients/acme-two\"]\nimports: [\"~/kb/chain/hop-1.md\"]\n",
        "## Accounting chain",
    );
    layer(
        "client-linked",
        "scope: folder\nfolders: [\"~/work/erp/clients/acme-two\"]\nimports: [\"~/kb/CLAUDE.md\"]\ndelivery: import\n",
        "## Linked knowledge",
    );
    layer(
        "client-loop",
        "globs: \"**/loop/**\"\nimports: [\"~/kb/cycle-a.md\"]\n",
        "## Loop",
    );
}

/// `layers_home` after one `context sync`: the managed `CLAUDE.local.md` files are in place.
pub fn layers_deployed_home(world: &CtlWorld) {
    layers_home(world);
    ctl(world, &["context", "sync"], None);
}

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
        "args": [], "cwd": world.path(&work),
        "mcpmSource": {"type": "git", "path": world.path(&work), "branch": "main"}
    }));
    write_json(&path, &registry);
}

/// `fork_world`, with `upstream` already fetched once, so `refs/remotes/upstream/*` is
/// populated and a source switch to that remote's `main` validates against the repo.
pub fn fork_world_upstream_fetched(world: &CtlWorld) {
    fork_world(world);
    let work = world.base.join("fork-work");
    git(&work, &world.home, &["fetch", "-q", "upstream"]);
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

/// Like `ctl`, and returns the `data` of the envelope.
pub fn ctl_data(world: &CtlWorld, argv: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_toolportctl"))
        .arg("--json")
        .args(argv)
        .env_clear()
        .envs(world.env())
        .current_dir(&world.home)
        .stdin(Stdio::null())
        .output()
        .expect("run toolportctl");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "toolportctl {argv:?}: {stdout}");
    let envelope: Value = serde_json::from_str(stdout.trim()).expect("one envelope line");
    envelope["data"].clone()
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
    let mut started = STARTED.lock().unwrap_or_else(|e| e.into_inner());
    if started.contains(&port) {
        return;
    }
    // Callers pick ports below the ephemeral range, where a child process's outbound socket could hold one.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let listener = loop {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => break listener,
            Err(e) if std::time::Instant::now() < deadline => {
                eprintln!("port {port} is busy for the fake proxy ({e}), retrying");
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(e) => panic!("port {port} is needed for the fake proxy: {e}"),
        }
    };
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

const SIGN_IN_TASK: &str = "---\ndescription: Refresh the portal token\n---\n# Refresh the portal token\n- Sign in to the portal in the browser\nCall mcp__toolport__srv-alpha__get_token and keep the value.\n";

/// Tasks of contract section 8: a task with a `needs-you` step that writes one secret, a scheduled
/// task that runs on its own, a disabled draft and a broken file; run records with fixed ids (one
/// waiting for the user, one finished, one whose runner process is gone) and the definition files
/// the `task add|edit` cases read.
pub fn tasks_home(world: &CtlWorld) {
    let tasks = world.data.join("plus/tasks");
    let runs = world.data.join("plus/task-runs");
    let defs = world.home.join("defs");
    let every_trigger = json!({"manual": true, "cli": true, "selfMcp": {"enabled": true, "approval": "every-run"}, "schedule": null, "onAuthFailure": ["srv-alpha"]});
    let portal = json!({
        "id": "portal-token", "title": "Refresh the portal token", "description": "Sign in and store a fresh token", "enabled": true,
        "requires": {"servers": ["srv-alpha"], "commands": []},
        "writesSecrets": [{"server": "srv-alpha", "key": "API_KEY"}],
        "steps": [
            {"id": "sign-in", "title": "Sign in", "type": "needs-you", "instructions": "Sign in to the portal in the browser window", "waitFor": {"kind": "manual", "timeoutSec": 900}},
            {"id": "read-token", "title": "Read the token", "type": "mcp", "server": "srv-alpha", "tool": "get_token", "args": {}, "capture": ["token"]},
            {"id": "store-token", "title": "Store the token", "type": "secret-set", "server": "srv-alpha", "key": "API_KEY", "from": "token"},
            {"id": "restart", "title": "Restart the server", "type": "restart-server", "server": "srv-alpha"}
        ],
        "triggers": every_trigger,
        "createdFrom": {"kind": "manual"}
    });
    let nightly = json!({
        "id": "nightly-report", "title": "Nightly report", "description": "", "enabled": true,
        "requires": {"servers": [], "commands": ["echo"]}, "writesSecrets": [],
        "steps": [{"id": "say", "title": "Say hello", "type": "exec", "program": "echo", "args": ["report"]}],
        "triggers": {"manual": true, "cli": false, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": {"cron": "0 3 * * *", "autoRun": true}, "onAuthFailure": []},
        "createdFrom": null
    });
    let draft = json!({
        "id": "draft-cleanup", "title": "Clean up", "description": "", "enabled": false,
        "requires": {"servers": [], "commands": []}, "writesSecrets": [],
        "steps": [{"id": "ask", "title": "Ask Claude", "type": "prompt", "prompt": "Tidy the notes", "allowedTools": ["Read"]}],
        "triggers": {"manual": true, "cli": false, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": null, "onAuthFailure": []},
        "createdFrom": {"kind": "command", "path": "/example/commands/cleanup.md"}
    });
    write_json(&tasks.join("portal-token.json"), &portal);
    write_json(&tasks.join("nightly-report.json"), &nightly);
    write_json(&tasks.join("draft-cleanup.json"), &draft);
    std::fs::write(tasks.join("broken.json"), "{ not json").unwrap();
    let step = |id: &str, title: &str, kind: &str, status: &str, output: &str| json!({"id": id, "title": title, "type": kind, "status": status, "startedAt": null, "endedAt": null, "output": output});
    write_json(
        &runs.join("run-fixture-waiting.json"),
        &json!({
            "id": "run-fixture-waiting", "task": "portal-token", "trigger": "manual", "status": "waiting",
            "startedAt": "2026-10-05T08:00:00Z", "endedAt": null, "durationMs": null,
            "steps": [
                {"id": "sign-in", "title": "Sign in", "type": "needs-you", "status": "waiting", "startedAt": "2026-10-05T08:00:01Z", "endedAt": null, "output": "", "instructions": "Sign in to the portal in the browser window"},
                step("read-token", "Read the token", "mcp", "pending", ""),
                step("store-token", "Store the token", "secret-set", "pending", ""),
                step("restart", "Restart the server", "restart-server", "pending", "")
            ]
        }),
    );
    write_json(
        &runs.join("run-fixture-ok.json"),
        &json!({
            "id": "run-fixture-ok", "task": "nightly-report", "trigger": "schedule", "status": "ok",
            "startedAt": "2026-10-04T03:00:00Z", "endedAt": "2026-10-04T03:00:01Z", "durationMs": 1200,
            "steps": [{"id": "say", "title": "Say hello", "type": "exec", "status": "ok", "startedAt": "2026-10-04T03:00:00Z", "endedAt": "2026-10-04T03:00:01Z", "output": "report"}]
        }),
    );
    write_json(
        &runs.join("run-fixture-stale.json"),
        &json!({
            "id": "run-fixture-stale", "task": "draft-cleanup", "trigger": "manual", "status": "running",
            "startedAt": "2026-10-03T09:00:00Z", "endedAt": null, "durationMs": null, "runnerPid": 2_147_000_000u32,
            "steps": [step("ask", "Ask Claude", "prompt", "running", "")]
        }),
    );
    write_json(&defs.join("weekly-cleanup.json"), &json!({
        "id": "weekly-cleanup", "title": "Weekly cleanup", "description": "", "enabled": true,
        "requires": {"servers": [], "commands": ["echo"]}, "writesSecrets": [],
        "steps": [{"id": "say", "title": "Say hello", "type": "exec", "program": "echo", "args": ["clean"]}],
        "triggers": {"manual": true, "cli": true, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": null, "onAuthFailure": []},
        "createdFrom": {"kind": "manual"}
    }));
    let mut changed = nightly.clone();
    changed["title"] = json!("Nightly report (changed)");
    write_json(&defs.join("nightly-report.json"), &changed);
    let mut undeclared = portal.clone();
    undeclared["steps"][2]["key"] = json!("OTHER_KEY");
    write_json(&defs.join("undeclared.json"), &undeclared);
    std::fs::write(defs.join("refresh-login.md"), SIGN_IN_TASK).unwrap();
}
