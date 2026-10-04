//! The bulk of the contract table (MIG-GUI-13): every command that `ctl_contract.rs` does not list
//! itself. Same rules: a writer previews with `--dry-run` first, then applies on the scratch world;
//! a command with a required operand gets a `usage` step; a command that cannot run headless
//! (`direct run`, `auth login`, `compression proxy`, `compression run`) records what it prints
//! without a terminal, a browser or an engine, and says so next to its case.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use super::ctl_world::write_json;
use super::{apply, case, prepared, read, setup, usage, Case, CtlWorld, COUNCIL_KEY};
use super::{PASSPHRASE, VAULTED};

const NEW_PASSPHRASE: &str = "FAKE-sync-passphrase-31d8-rotated";
const STATUSLINE: &str = r#"{"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":1000,"cache_read_input_tokens":500,"cache_creation_input_tokens":0}},"model":{"id":"claude-sonnet-5"}}"#;

fn git(dir: &Path, home: &Path, args: &[&str]) {
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
fn git_world(world: &CtlWorld) {
    let tap = world.base.join("tap-src");
    let skill = tap.join("skills/tapskill/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        skill,
        "---\nname: tapskill\ndescription: A synthetic tap skill\n---\nTap body\n",
    )
    .unwrap();
    git(&tap, &world.home, &["init", "-q"]);
    git(&tap, &world.home, &["symbolic-ref", "HEAD", "refs/heads/main"]);
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
fn import_world(world: &CtlWorld) {
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
fn transcripts_world(world: &CtlWorld) {
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

fn git_and_transcripts_world(world: &CtlWorld) {
    git_world(world);
    transcripts_world(world);
}

const ADD_PROJECT: &[&str] = &[
    "sync",
    "add-project",
    "{home}",
    "--name",
    "proj1",
    "--files",
    "a.txt",
];

const OFF_PORT_PROXY: &[&str] = &[
    "compression",
    "enable",
    "--provider",
    "headroom",
    "--port",
    "49213",
];

const PUSH_ALL: &[&str] = &["sync", "push", "--include-projects"];

const SYNC_INIT: &[&str] = &[
    "sync",
    "init",
    "--repo",
    "{base}/remote.git",
    "--machine-id",
    "m-test",
    "--passphrase-stdin",
];

pub const MORE: &[Case] = &[
    // server
    case(
        "server install",
        &[
            apply("apply", &["server", "install", "Stripe", "--offline"]),
            apply("again", &["server", "install", "Stripe", "--offline"]).exit(1),
            apply(
                "unknown",
                &["server", "install", "no-such-server", "--offline"],
            )
            .exit(1),
            usage("usage", &["server", "install"]),
        ],
    ),
    case(
        "server new",
        &[
            apply(
                "apply",
                &[
                    "server",
                    "new",
                    "gamma",
                    "--command",
                    "/bin/true",
                    "--arg",
                    "x",
                ],
            ),
            apply(
                "remote",
                &[
                    "server",
                    "new",
                    "remote1",
                    "--url",
                    "https://example.invalid/mcp2",
                    "--transport",
                    "http",
                ],
            ),
            read("after", &["server", "ls"]),
            usage("usage", &["server", "new"]),
        ],
    ),
    case(
        "server edit",
        &[
            setup(
                "setup",
                &["server", "new", "gamma", "--command", "/bin/true"],
            ),
            apply(
                "apply",
                &[
                    "server",
                    "edit",
                    "gamma",
                    "--arg",
                    "y",
                    "--forward-instructions",
                    "off",
                ],
            ),
            read("after", &["server", "info", "gamma"]),
            apply("unknown", &["server", "edit", "no-such-server", "--arg", "q"]).exit(1),
            usage("usage", &["server", "edit"]),
        ],
    ),
    // client
    case(
        "client edit",
        &[
            read(
                "preview",
                &[
                    "client",
                    "edit",
                    "cursor",
                    "--set-profiles",
                    "default",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["client", "edit", "cursor", "--set-profiles", "default"],
            ),
            usage("usage", &["client", "edit"]),
        ],
    ),
    case(
        "client import",
        &[
            read(
                "preview",
                &["client", "import", "cursor", "--all", "--dry-run"],
            ),
            apply(
                "apply",
                &[
                    "client",
                    "import",
                    "cursor",
                    "--all",
                    "--profile",
                    "cursor-import",
                ],
            ),
            usage("usage", &["client", "import"]),
        ],
    ),
    case(
        "client direct add",
        &[
            read(
                "preview",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["client", "direct", "add", "alpha", "--client", "claude-code"],
            ),
            read("after", &["client", "direct", "ls"]),
            usage("usage", &["client", "direct", "add"]),
        ],
    ),
    case(
        "client direct rm",
        &[
            setup(
                "setup",
                &["client", "direct", "add", "alpha", "--client", "claude-code"],
            ),
            read(
                "preview",
                &[
                    "client",
                    "direct",
                    "rm",
                    "alpha",
                    "--client",
                    "claude-code",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["client", "direct", "rm", "alpha", "--client", "claude-code"],
            ),
            usage("usage", &["client", "direct", "rm"]),
        ],
    ),
    // terminal surface: the command replaces itself with the server process, so only its refusals
    // print an envelope
    case(
        "direct run",
        &[
            apply("unknown", &["direct", "run", "no-such-server"]).exit(1),
            usage("usage", &["direct", "run"]),
        ],
    ),
    // auth and secrets
    case(
        "auth probe",
        &[
            apply("apply", &["auth", "probe"]),
            apply("unknown", &["auth", "probe", "--server", "alpha"]).exit(1),
        ],
    ),
    // a stdio server signs in without a browser; the browser flow of an http server needs a
    // resolvable authorization server and is not part of a headless contract run
    case(
        "auth login",
        &[
            apply("signed-in", &["auth", "login", "alpha", "--no-open"]),
            usage("usage", &["auth", "login"]),
        ],
    ),
    case(
        "secret get",
        &[
            setup("setup", &["secret", "set", "alpha", "API_TOKEN"]).stdin(VAULTED),
            read("set", &["secret", "get", "alpha", "API_TOKEN"]),
            read("reveal", &["secret", "get", "alpha", "API_TOKEN", "--reveal"]).reveals(),
            read("unset", &["secret", "get", "alpha", "NO_SUCH_KEY"]).exit(1),
            usage("usage", &["secret", "get"]),
        ],
    ),
    case(
        "secret rm",
        &[
            setup("setup", &["secret", "set", "alpha", "API_TOKEN"]).stdin(VAULTED),
            apply("apply", &["secret", "rm", "alpha", "API_TOKEN"]),
            read("after", &["secret", "get", "alpha", "API_TOKEN"]).exit(1),
            usage("usage", &["secret", "rm"]),
        ],
    ),
    // context
    case(
        "context loads",
        &[
            // the walk up from the folder reads project settings of every ancestor, so the
            // golden starts at the filesystem root instead of the machine's temp directory
            read("root", &["context", "loads", "--cwd", "/"]),
            read("profile", &["context", "loads", "--profile", "no-such-profile"]).exit(1),
        ],
    ),
    case(
        "context checkpoint-status",
        &[
            read(
                "status",
                &["context", "checkpoint-status", "--checkpoint-at", "50000"],
            )
            .stdin(STATUSLINE),
            read("empty", &["context", "checkpoint-status"]).exit(1),
        ],
    ),
    case("context plan", &[read("", &["context", "plan"])]),
    case(
        "context apply",
        &[
            read("preview", &["context", "apply", "--dry-run"]),
            apply("apply", &["context", "apply"]),
        ],
    ),
    case(
        "context sync",
        &[
            read("preview", &["context", "sync", "--dry-run"]),
            apply("apply", &["context", "sync"]),
        ],
    ),
    case(
        "context init",
        &[
            read("preview", &["context", "init", "--dry-run"]),
            apply("apply", &["context", "init"]),
        ],
    ),
    case(
        "context client add",
        &[
            read("preview", &["context", "client", "add", "acme", "--dry-run"]),
            apply("apply", &["context", "client", "add", "acme"]),
            usage("usage", &["context", "client", "add"]),
        ],
    ),
    case(
        "context profile add",
        &[
            read(
                "preview",
                &[
                    "context", "profile", "add", "work", "--rules", "none", "--servers", "none",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context", "profile", "add", "work", "--rules", "none", "--servers", "none",
                ],
            ),
            usage("usage", &["context", "profile", "add"]),
        ],
    ),
    case(
        "context profile remove",
        &[
            setup(
                "setup",
                &[
                    "context", "profile", "add", "work", "--rules", "none", "--servers", "none",
                ],
            ),
            read(
                "preview",
                &["context", "profile", "remove", "work", "--purge", "--dry-run"],
            ),
            apply("apply", &["context", "profile", "remove", "work", "--purge"]),
            usage("usage", &["context", "profile", "remove"]),
        ],
    ),
    case(
        "context disable",
        &[
            setup(
                "setup",
                &[
                    "context", "profile", "add", "work", "--rules", "none", "--servers", "none",
                ],
            ),
            read(
                "preview",
                &["context", "disable", "--purge-profiles", "--dry-run"],
            ),
            apply("apply", &["context", "disable", "--purge-profiles"]),
        ],
    ),
    // compression
    case(
        "compression enable",
        &[
            read(
                "preview",
                &["compression", "enable", "--provider", "rtk-only", "--dry-run"],
            ),
            apply("apply", &["compression", "enable", "--provider", "rtk-only"]),
        ],
    ),
    case(
        "compression use",
        &[
            read("preview", &["compression", "use", "agent", "--dry-run"]),
            apply("apply", &["compression", "use", "agent"]),
            usage("usage", &["compression", "use"]),
        ],
    ),
    case(
        "compression set-provider",
        &[
            read(
                "preview",
                &["compression", "set-provider", "headroom", "--dry-run"],
            ),
            apply("apply", &["compression", "set-provider", "headroom"]),
            usage("usage", &["compression", "set-provider"]),
        ],
    ),
    case(
        "compression sync",
        &[
            read("preview", &["compression", "sync", "--dry-run"]),
            apply("apply", &["compression", "sync"]),
        ],
    ),
    // reads the proxy's /health; the preset moves to a port nothing listens on, so a proxy of the
    // machine that runs the test is never touched
    case(
        "compression seal",
        &[
            setup("setup", OFF_PORT_PROXY),
            read("no-proxy", &["compression", "seal"]).exit(1),
        ],
    ),
    case(
        "compression disable",
        &[
            read(
                "preview",
                &["compression", "disable", "--teardown", "--dry-run"],
            ),
            apply("apply", &["compression", "disable", "--teardown"]),
        ],
    ),
    // terminal surface: `--plan` prints what would run instead of replacing the process
    case(
        "compression run",
        &[read("plan", &["compression", "run", "--plan", "claude"])],
    ),
    prepared(
        "compression verify",
        transcripts_world,
        &[
            read(
                "no-transcripts",
                &["compression", "verify", "--transcripts", "{base}/none"],
            )
            .exit(1),
            read(
                "measured",
                &[
                    "compression",
                    "verify",
                    "--transcripts",
                    "{home}/.claude/projects",
                    "--min-turns",
                    "1",
                ],
            ),
        ],
    ),
    case(
        "compression ledger record",
        &[
            apply(
                "apply",
                &[
                    "compression",
                    "ledger",
                    "record",
                    "--provider",
                    "rtk-only",
                    "--before",
                    "1000",
                    "--after",
                    "400",
                    "--source",
                    "contract",
                    "--session",
                    "s1",
                ],
            ),
            usage("usage", &["compression", "ledger", "record"]),
        ],
    ),
    // the proxy needs the engine on PATH, which the contract world does not have
    case(
        "compression proxy up",
        &[
            setup("setup", OFF_PORT_PROXY),
            apply("no-engine", &["compression", "proxy", "up"]).exit(1),
        ],
    ),
    case(
        "compression proxy down",
        &[
            setup("setup", OFF_PORT_PROXY),
            apply("no-proxy", &["compression", "proxy", "down"]).exit(1),
        ],
    ),
    case(
        "compression proxy restart",
        &[
            setup("setup", OFF_PORT_PROXY),
            apply("no-engine", &["compression", "proxy", "restart"]).exit(1),
        ],
    ),
    // `--accept` would install the engine with uv and the latest version comes from the network,
    // so only the preview of an explicit target is recorded
    case(
        "compression update",
        &[read(
            "preview",
            &["compression", "update", "--to", "0.30.0"],
        )],
    ),
    // import
    prepared(
        "import mcpm",
        import_world,
        &[
            read("preview", &["import", "mcpm", "{base}/mcpm", "--dry-run"]),
            read(
                "name-map",
                &[
                    "import",
                    "mcpm",
                    "{base}/mcpm",
                    "--dry-run",
                    "--tools",
                    "{base}/tools.json",
                    "--name-map",
                ],
            ),
            apply("apply", &["import", "mcpm", "{base}/mcpm"]),
            usage("usage", &["import", "mcpm"]),
        ],
    ),
    prepared(
        "import rename-refs",
        import_world,
        &[
            read(
                "preview",
                &[
                    "import",
                    "rename-refs",
                    "{base}/mcpm",
                    "--tools",
                    "{base}/tools.json",
                    "--paths",
                    "{base}/refs",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "import",
                    "rename-refs",
                    "{base}/mcpm",
                    "--tools",
                    "{base}/tools.json",
                    "--paths",
                    "{base}/refs",
                ],
            ),
            usage("usage", &["import", "rename-refs"]),
        ],
    ),
    // council and the self-management server
    case(
        "council install",
        &[
            apply("apply", &["council", "install"]),
            apply(
                "key",
                &["council", "install", "--api-key-env", "COUNCIL_TEST_KEY"],
            )
            .env(&[("COUNCIL_TEST_KEY", COUNCIL_KEY)]),
        ],
    ),
    case(
        "council uninstall",
        &[
            setup("setup", &["council", "install"]),
            apply("apply", &["council", "uninstall"]),
            apply("again", &["council", "uninstall", "--purge-key"]),
        ],
    ),
    case(
        "mcp install",
        &[
            apply("apply", &["mcp", "install"]),
            apply("profile", &["mcp", "install", "--profile", "default"]),
        ],
    ),
    case(
        "mcp uninstall",
        &[
            setup("setup", &["mcp", "install"]),
            apply("apply", &["mcp", "uninstall"]),
        ],
    ),
    // skills
    case(
        "skills init",
        &[
            read(
                "preview",
                &["skills", "init", "--path", "{base}/fresh", "--name", "contract", "--dry-run"],
            ),
            apply(
                "apply",
                &["skills", "init", "--path", "{base}/fresh", "--name", "contract"],
            ),
        ],
    ),
    case(
        "skills add",
        &[
            setup(
                "setup",
                &["skills", "init", "--path", "{base}/fresh", "--name", "contract"],
            ),
            read(
                "preview",
                &["skills", "add", "fresh-skill", "--path", "{base}/fresh", "--dry-run"],
            ),
            apply(
                "apply",
                &["skills", "add", "fresh-skill", "--path", "{base}/fresh"],
            ),
            usage("usage", &["skills", "add"]),
        ],
    ),
    case(
        "skills bundle",
        &[
            read("preview", &["skills", "bundle", "--repo", "{repo}", "--dry-run"]),
            apply(
                "apply",
                &["skills", "bundle", "--repo", "{repo}", "--output", "{base}/demo.zip"],
            ),
        ],
    ),
    case(
        "skills unbundle",
        &[
            setup(
                "bundle",
                &["skills", "bundle", "--repo", "{repo}", "--output", "{base}/demo.zip"],
            ),
            setup(
                "init",
                &["skills", "init", "--path", "{base}/fresh", "--name", "contract"],
            ),
            read(
                "preview",
                &["skills", "unbundle", "{base}/demo.zip", "--path", "{base}/fresh", "--dry-run"],
            ),
            apply(
                "apply",
                &["skills", "unbundle", "{base}/demo.zip", "--path", "{base}/fresh"],
            ),
            usage("usage", &["skills", "unbundle"]),
        ],
    ),
    case(
        "skills clean",
        &[
            setup("setup", &["skills", "sync", "--repo", "{repo}"]),
            read("preview", &["skills", "clean", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["skills", "clean", "--repo", "{repo}"]),
        ],
    ),
    case(
        "skills uninstall",
        &[
            setup("setup", &["skills", "sync", "--repo", "{repo}"]),
            read(
                "preview",
                &["skills", "uninstall", "demo", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["skills", "uninstall", "demo", "--repo", "{repo}"]),
            apply("unknown", &["skills", "uninstall", "demo", "--repo", "{repo}"]).exit(1),
            usage("usage", &["skills", "uninstall"]),
        ],
    ),
    case(
        "skills resolve",
        &[
            setup("setup", &["skills", "sync", "--repo", "{repo}"]),
            read("preview", &["skills", "resolve", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["skills", "resolve", "--repo", "{repo}"]),
        ],
    ),
    prepared(
        "skills tap add",
        git_world,
        &[
            read(
                "preview",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local", "--dry-run"],
            ),
            apply(
                "apply",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            read("after", &["skills", "tap", "ls"]),
            usage("usage", &["skills", "tap", "add"]),
        ],
    ),
    prepared(
        "skills tap remove",
        git_world,
        &[
            setup(
                "setup",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            read("preview", &["skills", "tap", "remove", "local", "--dry-run"]),
            apply("apply", &["skills", "tap", "remove", "local"]),
            apply("unknown", &["skills", "tap", "remove", "local"]).exit(1),
            usage("usage", &["skills", "tap", "remove"]),
        ],
    ),
    prepared(
        "skills tap update",
        git_world,
        &[
            read("none", &["skills", "tap", "update", "--dry-run"]),
            setup(
                "setup",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            read("preview", &["skills", "tap", "update", "--dry-run"]),
            apply("apply", &["skills", "tap", "update", "local"]),
        ],
    ),
    prepared(
        "skills search",
        git_world,
        &[
            read("empty", &["skills", "search", "tapskill"]),
            setup(
                "setup",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            read("hit", &["skills", "search", "tapskill"]),
            usage("usage", &["skills", "search"]),
        ],
    ),
    // the clone url of an `@user/repo` spec is github, so only the dry-run plan is recorded
    case(
        "skills install",
        &[
            read(
                "preview",
                &["skills", "install", "@acme/skills", "--dry-run"],
            ),
            usage("usage", &["skills", "install"]),
        ],
    ),
    // agents
    case(
        "agents add",
        &[
            read("preview", &["agents", "add", "scout", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["agents", "add", "scout", "--repo", "{repo}"]),
            usage("usage", &["agents", "add"]),
        ],
    ),
    case(
        "agents audit",
        &[read("", &["agents", "audit", "--repo", "{repo}"])],
    ),
    case(
        "agents sync",
        &[
            read("preview", &["agents", "sync", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["agents", "sync", "--repo", "{repo}"]),
        ],
    ),
    case(
        "agents clean",
        &[
            setup("setup", &["agents", "sync", "--repo", "{repo}"]),
            read("preview", &["agents", "clean", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["agents", "clean", "--repo", "{repo}"]),
        ],
    ),
    case(
        "agents uninstall",
        &[
            setup("setup", &["agents", "sync", "--repo", "{repo}"]),
            read(
                "preview",
                &["agents", "uninstall", "helper", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["agents", "uninstall", "helper", "--repo", "{repo}"]),
            usage("usage", &["agents", "uninstall"]),
        ],
    ),
    // styles
    case(
        "styles add",
        &[
            read("preview", &["styles", "add", "terse", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["styles", "add", "terse", "--repo", "{repo}"]),
            usage("usage", &["styles", "add"]),
        ],
    ),
    case(
        "styles sync",
        &[
            read("preview", &["styles", "sync", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["styles", "sync", "--repo", "{repo}"]),
        ],
    ),
    case(
        "styles apply",
        &[
            read(
                "preview",
                &["styles", "apply", "plain", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["styles", "apply", "plain", "--repo", "{repo}"]),
            usage("usage", &["styles", "apply"]),
        ],
    ),
    case(
        "styles remove",
        &[
            setup("setup", &["styles", "apply", "plain", "--repo", "{repo}"]),
            read("preview", &["styles", "remove", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["styles", "remove", "--repo", "{repo}"]),
        ],
    ),
    case(
        "styles clean",
        &[
            setup("setup", &["styles", "sync", "--repo", "{repo}"]),
            read("preview", &["styles", "clean", "--repo", "{repo}", "--dry-run"]),
            apply("apply", &["styles", "clean", "--repo", "{repo}"]),
        ],
    ),
    // sync: the remote is a local bare repository
    prepared(
        "sync init",
        git_world,
        &[
            apply("apply", SYNC_INIT).stdin(PASSPHRASE),
            read("after", &["sync", "status"]),
            usage("usage", &["sync", "init"]),
            usage(
                "passphrase",
                &["sync", "init", "--repo", "{base}/remote.git"],
            ),
        ],
    ),
    prepared(
        "sync push",
        git_world,
        &[
            apply("unconfigured", PUSH_ALL).exit(1),
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            read(
                "preview",
                &["sync", "push", "--include-projects", "--dry-run"],
            ),
            apply("apply", PUSH_ALL),
        ],
    ),
    prepared(
        "sync pull",
        git_world,
        &[
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            setup("push", PUSH_ALL),
            read(
                "preview",
                &["sync", "pull", "--include-projects", "--dry-run"],
            ),
            apply("apply", &["sync", "pull", "--include-projects"]),
        ],
    ),
    prepared(
        "sync diff",
        git_world,
        &[
            read("unconfigured", &["sync", "diff"]).exit(1),
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            setup("push", PUSH_ALL),
            read("configured", &["sync", "diff"]),
        ],
    ),
    prepared(
        "sync reset",
        git_world,
        &[
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            apply("apply", &["sync", "reset"]),
            read("after", &["sync", "status"]),
        ],
    ),
    prepared(
        "sync rotate-passphrase",
        git_world,
        &[
            apply(
                "unconfigured",
                &["sync", "rotate-passphrase", "--passphrase-stdin"],
            )
            .stdin(NEW_PASSPHRASE)
            .exit(1),
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            setup("push", PUSH_ALL),
            apply(
                "apply",
                &["sync", "rotate-passphrase", "--passphrase-stdin"],
            )
            .stdin(NEW_PASSPHRASE),
            usage("usage", &["sync", "rotate-passphrase"]),
        ],
    ),
    prepared(
        "sync add-project",
        git_world,
        &[
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            apply("apply", ADD_PROJECT),
            read("after", &["sync", "status"]),
            usage("usage", &["sync", "add-project"]),
        ],
    ),
    prepared(
        "sync remove-project",
        git_world,
        &[
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            apply("apply", &["sync", "remove-project", "proj1"]),
            read("after", &["sync", "status"]),
            usage("usage", &["sync", "remove-project"]),
        ],
    ),
    prepared(
        "sync git-sync",
        git_world,
        &[
            apply("status", &["sync", "git-sync", "--status"]),
            apply(
                "configure",
                &[
                    "sync",
                    "git-sync",
                    "--repo",
                    "{base}/tap-src",
                    "--branch",
                    "main",
                ],
            ),
            apply("clear", &["sync", "git-sync", "--clear"]),
        ],
    ),
    // a real migration needs a bundle in the legacy mcpm format; the refusals are recorded
    prepared(
        "sync migrate",
        git_world,
        &[
            apply(
                "no-bundle",
                &["sync", "migrate", "{base}", "--passphrase-stdin"],
            )
            .stdin(PASSPHRASE)
            .exit(1),
            usage("usage", &["sync", "migrate"]),
        ],
    ),
    // plugins, usage and telemetry
    case(
        "cc update",
        &[
            read("preview", &["cc", "update", "--dry-run"]),
            read("one", &["cc", "update", "demo-plugin", "--dry-run"]),
            apply("apply", &["cc", "update", "demo-plugin"]),
        ],
    ),
    prepared(
        "usage",
        transcripts_world,
        &[
            apply("apply", &["usage"]),
            apply("cached", &["usage", "--no-refresh"]),
        ],
    ),
    case(
        "obs otel enable",
        &[
            read("preview", &["obs", "otel", "enable", "--port", "4999", "--dry-run"]),
            apply("apply", &["obs", "otel", "enable", "--port", "4999"]),
        ],
    ),
    case(
        "obs otel disable",
        &[
            setup("setup", &["obs", "otel", "enable", "--port", "4999"]),
            read("preview", &["obs", "otel", "disable", "--dry-run"]),
            apply("apply", &["obs", "otel", "disable"]),
        ],
    ),
];
