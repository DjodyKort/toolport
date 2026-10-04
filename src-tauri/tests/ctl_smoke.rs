//! Smoke test for every registered `toolportctl` command (MIG-HRD-1).
//!
//! The real binary runs against a synthetic data directory, home and skills
//! repository. Every row of `COMMANDS` is accounted for: `--help` for all of
//! them, `--json` with a checked exit code and envelope for the read-only
//! ones, and a reason for the rest. A new command fails the coverage test
//! until it is classified here.

#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use conduit_lib::plus::ctl::{COMMANDS, SCHEMA_VERSION};
use serde_json::{json, Value};

#[path = "common/exec.rs"]
mod exec_fixture;

const FAKE_SECRET: &str = "FAKE-SECRET-VALUE-do-not-print-7f3a";
const VAULTED_VALUE: &str = "FAKE-vaulted-value-1c9e";
const RUN_TIMEOUT: Duration = Duration::from_secs(120);

static NEXT: AtomicUsize = AtomicUsize::new(0);

const NOT_READ_ONLY: &[(&str, &str)] = &[
    ("server install", "adds a catalog server; round trip test"),
    ("server new", "adds a server; round trip test"),
    ("server edit", "changes a server; round trip test"),
    ("secret set", "writes the vault; round trip test"),
    ("secret rm", "writes the vault; round trip test"),
    ("auth probe", "writes the auth status cache; ctl::auth_tests"),
    ("auth login", "starts a sign-in and writes the vault; ctl::auth_tests"),
    ("compression proxy", "starts or stops a local proxy process"),
    ("compression enable", "writes the policy, shims and registry entry; compression round trip test"),
    ("compression disable", "writes the policy and removes artifacts; compression round trip test"),
    ("compression set-provider", "writes the policy, artifacts and registry; compression round trip test"),
    ("compression use", "writes the policy and artifacts; compression round trip test"),
    ("compression sync", "reconciles artifacts and registry; compression round trip test"),
    ("compression seal", "reads a live proxy /health; stub engine tests in ctl::compression_cfg_tests"),
    ("skills unbundle", "extracts into the target; bundle round trip test"),
    ("usage", "indexes transcripts into the data dir; ctl::usage unit tests"),
    ("obs otel enable", "writes the Claude settings and the receiver config; obs::otel_e2e_tests round trip"),
    ("obs otel disable", "removes what enable wrote; obs::otel_e2e_tests round trip"),
];

struct World {
    base: PathBuf,
    data: PathBuf,
    home: PathBuf,
    repo: PathBuf,
    mcpm: PathBuf,
    tools: PathBuf,
    refs: PathBuf,
    claude: PathBuf,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn envelope(&self) -> Value {
        assert_eq!(
            self.stdout.trim_end().lines().count(),
            1,
            "--json prints exactly one line: {:?}",
            self.stdout
        );
        serde_json::from_str(self.stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "stdout is not JSON ({e}): {:?} / {:?}",
                self.stdout, self.stderr
            )
        })
    }
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "ctl-smoke-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let world = Self {
            data: base.join("data"),
            home: base.join("home"),
            repo: base.join("skills-repo"),
            mcpm: base.join("mcpm"),
            tools: base.join("tools.json"),
            refs: base.join("refs"),
            claude: base.join("fake-claude"),
            base,
        };
        let mock = env!("CARGO_BIN_EXE_mock-mcp-server");
        for dir in [&world.data, &world.home, &world.mcpm, &world.refs] {
            std::fs::create_dir_all(dir).unwrap();
        }
        write_json(
            &world.data.join("registry.json"),
            &json!({
                "version": 1,
                "servers": [
                    {"id": "srv-alpha", "name": "alpha", "transport": "stdio",
                     "command": mock, "args": [],
                     "mcpmSource": {"type": "unknown", "reason": "synthetic fixture"},
                     "env": [{"key": "API_KEY", "value": FAKE_SECRET, "secret": true}]},
                    {"id": "srv-beta", "name": "beta", "transport": "http",
                     "url": "https://example.invalid/mcp"}
                ],
                "profiles": [{"id": "default", "name": "Default",
                              "enabledServerIds": ["srv-alpha"]}],
                "activeProfileId": "default"
            }),
        );
        write_json(
            &world.home.join(".cursor/mcp.json"),
            &json!({"mcpServers": {"direct-one": {"command": "echo", "args": ["x"]}}}),
        );
        std::fs::create_dir_all(world.home.join(".claude")).unwrap();
        for (rel, text) in [
            (
                "skills/demo/SKILL.md",
                "---\nname: demo\ndescription: A synthetic demo skill\n---\nBody text\n",
            ),
            (
                "agents/helper/AGENT.md",
                "---\nname: helper\ndescription: A synthetic helper agent\nmodel: inherit\n---\nAgent prompt\n",
            ),
            (
                "styles/plain/STYLE.md",
                "---\nname: plain\ndescription: A synthetic plain style\nkeep-coding-instructions: true\n---\nStyle text\n",
            ),
        ] {
            let path = world.repo.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        write_json(
            &world.mcpm.join("servers.json"),
            &json!({
                "alpha-mock": {"name": "alpha-mock", "profile_tags": ["smoke"],
                               "command": mock, "args": []},
                "beta-mock": {"name": "beta-mock", "profile_tags": ["smoke"],
                              "command": mock, "args": []}
            }),
        );
        write_json(
            &world.tools,
            &json!({"alpha-mock": ["echo", "add"], "beta-mock": ["echo"]}),
        );
        std::fs::write(
            world.refs.join("note.md"),
            "Use mcp__mcpm_alpha-mock__echo to echo.\n",
        )
        .unwrap();
        exec_fixture::write_executable(
            &world.claude,
            "#!/bin/sh\necho '[{\"name\":\"demo-plugin\",\"marketplace\":\"fake-market\",\"version\":\"1.0.0\"}]'\n",
        );
        world
    }

    fn path(&self, p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Run {
        let mut child = Command::new(env!("CARGO_BIN_EXE_toolportctl"))
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("CLAUDE_CONFIG_DIR", self.home.join(".claude"))
            .env("TOOLPORT_DATA_DIR", &self.data)
            .env("TOOLPORT_SECRET_KEY", "ab".repeat(32))
            .env("TOOLPORT_CLAUDE_BIN", &self.claude)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn toolportctl");
        if let Some(text) = stdin {
            let mut pipe = child.stdin.take().unwrap();
            pipe.write_all(text.as_bytes()).unwrap();
        }
        let pid = child.id();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(child.wait_with_output());
        });
        let output = match receiver.recv_timeout(RUN_TIMEOUT) {
            Ok(output) => output.expect("wait for toolportctl"),
            Err(_) => {
                let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
                panic!("toolportctl {args:?} did not finish in {RUN_TIMEOUT:?}");
            }
        };
        let run = Run {
            code: output.status.code().expect("exit code, not a signal"),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
        for secret in [FAKE_SECRET, VAULTED_VALUE] {
            if args.contains(&"--reveal") && secret == VAULTED_VALUE {
                continue;
            }
            assert!(
                !run.stdout.contains(secret) && !run.stderr.contains(secret),
                "{args:?} leaked a secret value"
            );
        }
        run
    }

    fn json(&self, args: &[&str]) -> (Run, Value) {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let run = self.run(&full, None);
        let value = run.envelope();
        (run, value)
    }

    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        collect(&self.base, &mut files);
        files
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn write_json(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn same_path(actual: &Value, expected: &Path) {
    let actual = Path::new(actual.as_str().expect("a path string"));
    assert_eq!(
        std::fs::canonicalize(actual).unwrap(),
        std::fs::canonicalize(expected).unwrap()
    );
}

fn collect(dir: &Path, into: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, into);
        } else if path.file_name().and_then(|n| n.to_str()) != Some("registry.json.lock") {
            // flock sentinel that a read path may create next to the registry
            into.insert(path.clone(), std::fs::read(&path).unwrap_or_default());
        }
    }
}

fn row_key(path: &[&str]) -> String {
    path.join(" ")
}

fn group_keys() -> BTreeSet<String> {
    COMMANDS
        .iter()
        .filter(|c| {
            COMMANDS
                .iter()
                .any(|o| o.path.len() > c.path.len() && o.path.starts_with(c.path))
        })
        .map(|c| row_key(c.path))
        .collect()
}

fn assert_envelope(label: &str, run: &Run, value: &Value, command: &str, exit: i32) {
    assert_eq!(
        run.code, exit,
        "{label}: exit code; stdout {:?}",
        run.stdout
    );
    assert!(
        run.stderr.is_empty(),
        "{label}: --json keeps stderr empty: {:?}",
        run.stderr
    );
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{label}: not an object"));
    let allowed: BTreeSet<&str> = ["ok", "command", "schemaVersion", "data", "error"].into();
    for key in object.keys() {
        assert!(
            allowed.contains(key.as_str()),
            "{label}: unexpected key {key}"
        );
    }
    assert_eq!(value["schemaVersion"], SCHEMA_VERSION, "{label}");
    assert_eq!(value["command"], command, "{label}");
    assert_eq!(value["ok"], exit == 0, "{label}");
    match exit {
        0 => {
            assert!(value["data"].is_object(), "{label}: success carries data");
            assert!(value.get("error").is_none(), "{label}");
        }
        _ => {
            assert!(
                value["error"]["code"]
                    .as_str()
                    .is_some_and(|c| !c.is_empty()),
                "{label}"
            );
            assert!(
                value["error"]["message"]
                    .as_str()
                    .is_some_and(|c| !c.is_empty()),
                "{label}"
            );
        }
    }
}

#[test]
fn every_registered_command_prints_help_and_exits_zero() {
    let world = World::new("help");
    let before = world.snapshot();
    for command in COMMANDS {
        for flag in ["--help", "-h"] {
            let mut argv: Vec<&str> = command.path.to_vec();
            argv.push(flag);
            let run = world.run(&argv, None);
            assert_eq!(run.code, 0, "{argv:?}: {:?}", run.stderr);
            assert!(run.stderr.is_empty(), "{argv:?}");
            assert!(run.stdout.contains("Usage: toolportctl"), "{argv:?}");
            assert!(
                run.stdout.contains(command.summary),
                "{argv:?}: the help lists {:?}",
                command.summary
            );
            assert!(run.stdout.contains(&row_key(command.path)), "{argv:?}");
        }
    }
    let run = world.run(&["--help"], None);
    assert_eq!(run.code, 0);
    let listed = run.stdout.lines().filter(|l| l.starts_with("  ")).count();
    assert!(
        listed >= COMMANDS.len(),
        "{listed} help rows for {} commands",
        COMMANDS.len()
    );
    assert_eq!(world.snapshot(), before, "help must not write");
}

#[test]
fn version_and_usage_errors_use_the_documented_exit_codes() {
    let world = World::new("usage");
    for flag in ["--version", "-V"] {
        let (run, value) = world.json(&[flag]);
        assert_envelope(flag, &run, &value, "version", 0);
        assert_eq!(value["data"]["name"], "toolportctl");
        assert_eq!(value["data"]["version"], env!("CARGO_PKG_VERSION"));
    }
    for (argv, command) in [
        (vec!["frobnicate"], "frobnicate"),
        (vec!["status", "extra"], "status"),
        (vec!["server", "info"], "server info"),
        (vec!["secret", "get", "only-one"], "secret get"),
        (vec!["skills", "ls", "--bogus"], "skills ls"),
        (vec!["skills", "add"], "skills add"),
        (vec!["skills", "unbundle"], "skills unbundle"),
    ] {
        let (run, value) = world.json(&argv);
        assert_envelope(&argv.join(" "), &run, &value, command, 2);
        assert_eq!(value["error"]["code"], "usage", "{argv:?}");
    }
    for argv in [vec!["--bogus", "status"], vec!["--data-dir"], vec![]] {
        let run = world.run(&argv, None);
        assert_eq!(run.code, 2, "{argv:?}");
        assert!(
            run.stdout.is_empty() || run.stdout.contains("Usage"),
            "{argv:?}"
        );
    }
    let run = world.run(&["status"], None);
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("Data dir:"), "{}", run.stdout);
    assert!(run.stderr.is_empty());
    let run = world.run(&["server", "info", "nope"], None);
    assert_eq!(run.code, 1);
    assert!(
        run.stdout.is_empty(),
        "failures go to stderr without --json"
    );
    assert!(run.stderr.contains("nope"), "{}", run.stderr);
}

struct Case {
    key: &'static str,
    argv: Vec<String>,
    stdin: Option<&'static str>,
    exit: i32,
    check: fn(&World, &Value),
}

fn case(key: &'static str, argv: &[&str], exit: i32, check: fn(&World, &Value)) -> Case {
    Case {
        key,
        argv: argv.iter().map(|s| s.to_string()).collect(),
        stdin: None,
        exit,
        check,
    }
}

fn read_only_cases(w: &World) -> Vec<Case> {
    let (repo, home, cwd) = (w.path(&w.repo), w.path(&w.home), w.path(&w.repo));
    let fresh = w.path(&w.base.join("fresh-skills"));
    let (mcpm, tools, refs) = (w.path(&w.mcpm), w.path(&w.tools), w.path(&w.refs));
    let own = |parts: &[&str]| -> Vec<String> { parts.iter().map(|s| s.to_string()).collect() };
    let mut cases = vec![
        case("status", &["status"], 0, |w, d| {
            same_path(&d["dataDir"], &w.data);
            assert_eq!(d["serverCount"], 2);
            assert_eq!(d["activeProfile"], "default");
            assert_eq!(d["secretsBackend"], "encrypted-file");
        }),
        case("doctor", &["doctor"], 0, |_, d| {
            assert_eq!(d["healthy"], true);
            assert!(d["checks"].as_array().unwrap().len() >= 4);
        }),
        case("server ls", &["server", "ls"], 0, |_, d| {
            let names: Vec<&str> = d["servers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["name"].as_str().unwrap())
                .collect();
            assert_eq!(names, ["alpha", "beta"]);
        }),
        case(
            "server search",
            &["server", "search", "--offline", "--limit", "3"],
            0,
            |_, d| {
                assert_eq!(d["results"].as_array().unwrap().len(), 3);
                assert!(d["total"].as_u64().unwrap() >= 3);
            },
        ),
        case("server info", &["server", "info", "alpha"], 0, |_, d| {
            assert_eq!(d["id"], "srv-alpha");
            assert_eq!(d["env"], json!([{"key": "API_KEY", "secret": true}]));
        }),
        case(
            "server uninstall",
            &["server", "uninstall", "alpha", "--dry-run"],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["id"], "srv-alpha");
            },
        ),
        case("inspect", &["inspect", "alpha"], 0, |_, d| {
            let tools = d["servers"][0]["tools"].as_array().unwrap();
            assert!(tools.iter().any(|t| t["name"] == "echo"), "{tools:?}");
        }),
        case("profile inspect", &["profile", "inspect"], 0, |_, d| {
            assert_eq!(d["profile"], "default");
            assert_eq!(d["servers"][0]["id"], "srv-alpha");
        }),
        case("client ls", &["client", "ls"], 0, |_, d| {
            let cursor = d["clients"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == "cursor")
                .expect("the synthetic cursor config is detected");
            assert_eq!(cursor["entries"], json!(["direct-one"]));
            assert_eq!(cursor["gateway"], "absent");
        }),
        case(
            "client sync",
            &["client", "sync", "--dry-run"],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
            },
        ),
        case("auth statusline", &["auth", "statusline"], 0, |_, d| {
            assert_eq!(d["auth"]["text"], "auth ok (0)");
        }),
        case("auth hook", &["auth", "hook"], 0, |_, d| {
            assert!(d["auth"]["worst"].is_array());
        }),
        case(
            "secret get",
            &["secret", "get", "srv-alpha", "API_KEY"],
            1,
            |_, _| {},
        ),
        case(
            "context loads",
            &["context", "loads", "--cwd", &cwd],
            0,
            |w, d| {
                same_path(&d["cwd"], &w.repo);
                assert!(d["items"].is_array());
            },
        ),
        case(
            "context folders",
            &["context", "folders", "--cwd", &cwd],
            0,
            |_, d| {
                assert_eq!(d["enabled"], false);
            },
        ),
        case(
            "context plan",
            &["context", "plan", "--home", &home],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
            },
        ),
        case(
            "context apply",
            &["context", "apply", "--dry-run", "--home", &home],
            0,
            |_, d| {
                assert!(d["actions"].is_array());
            },
        ),
        case(
            "context sync",
            &["context", "sync", "--dry-run", "--home", &home],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert!(d["plan"].is_object());
            },
        ),
        case(
            "compression status",
            &["compression", "status"],
            0,
            |_, d| {
                assert_eq!(d["configExists"], false);
                assert_eq!(d["pin"]["package"], "headroom-ai");
            },
        ),
        case(
            "compression presets",
            &["compression", "presets"],
            0,
            |_, d| {
                assert_eq!(d["active"], "interactive");
                assert!(d["presets"].as_array().unwrap().len() >= 3);
            },
        ),
        case(
            "compression run",
            &["compression", "run", "--plan"],
            0,
            |_, d| {
                assert!(d["argv"].is_array());
                assert!(d["env"].is_object());
            },
        ),
        case(
            "compression verify",
            &["compression", "verify"],
            1,
            |_, d| {
                assert_eq!(d["provider"], "none");
                assert_eq!(d["transcripts"]["count"], 0);
                assert!(d["buckets"].is_null());
            },
        ),
        case(
            "compression env",
            &["compression", "env", "--cwd", &home],
            0,
            |_, d| {
                assert_eq!(d["launch"], "plain");
                assert_eq!(d["lines"], json!(["HRCOMPRESS_LAUNCH=plain"]));
            },
        ),
        case("compression pin", &["compression", "pin"], 0, |_, d| {
            assert_eq!(d["pin"], "0.29.0");
            assert_eq!(d["set"], false);
            assert!(d["install"].is_null());
        }),
        case(
            "compression doctor",
            &["compression", "doctor"],
            0,
            |_, d| {
                assert!(!d["checks"].as_array().unwrap().is_empty());
            },
        ),
        case(
            "compression ledger",
            &["compression", "ledger", "summary"],
            0,
            |_, d| {
                assert_eq!(d["tokensSaved"], 0);
            },
        ),
        case(
            "compression update",
            &["compression", "update", "--to", "0.30.0"],
            0,
            |_, d| {
                assert_eq!(d["accepted"], false);
                assert_eq!(d["target"], "0.30.0");
            },
        ),
        case(
            "import mcpm",
            &[
                "import",
                "mcpm",
                &mcpm,
                "--dry-run",
                "--home",
                &home,
                "--skip-clients",
            ],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["servers"].as_array().unwrap().len(), 2);
                assert!(d["rejects"].as_array().unwrap().is_empty());
            },
        ),
        case(
            "import mcpm",
            &["import", "mcpm", &mcpm, "--name-map", "--tools", &tools],
            0,
            |_, d| {
                assert_eq!(d["count"], 3);
                assert_eq!(
                    d["map"]["mcp__mcpm_alpha-mock__echo"],
                    "mcp__toolport__alpha_mock__echo"
                );
            },
        ),
        case(
            "import rename-refs",
            &[
                "import",
                "rename-refs",
                &mcpm,
                "--tools",
                &tools,
                "--paths",
                &refs,
                "--dry-run",
            ],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["replaced"], 1);
            },
        ),
        case("council", &["council", "doctor"], 1, |_, d| {
            assert!(!d["checks"].as_array().unwrap().is_empty());
        }),
        case("council", &["council", "tools"], 0, |_, d| {
            assert!(!d["tools"].as_array().unwrap().is_empty());
            assert_eq!(d["resources"].as_array().unwrap().len(), 2);
        }),
        case("mcp", &["mcp", "doctor"], 1, |_, d| {
            assert!(!d["checks"].as_array().unwrap().is_empty());
        }),
        case("mcp", &["mcp", "tools"], 0, |_, d| {
            assert_eq!(d["tools"].as_array().unwrap().len(), 49);
            assert_eq!(d["resources"].as_array().unwrap().len(), 11);
        }),
        case(
            "skills sync",
            &[
                "skills",
                "sync",
                "--repo",
                &repo,
                "--home",
                &home,
                "--dry-run",
            ],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["skillCount"], 1);
            },
        ),
        case(
            "skills ls",
            &["skills", "ls", "--repo", &repo, "--home", &home],
            0,
            |_, d| {
                assert_eq!(d["skills"][0]["name"], "demo");
            },
        ),
        case(
            "skills lint",
            &["skills", "lint", "--repo", &repo, "--home", &home],
            0,
            |_, d| {
                assert_eq!(d["errors"], 0);
            },
        ),
        case(
            "skills diff",
            &["skills", "diff", "--repo", &repo, "--home", &home],
            1,
            |_, d| {
                assert_eq!(d["noLockfile"], true);
                assert_eq!(d["new"], json!(["demo"]));
            },
        ),
        case(
            "skills audit",
            &["skills", "audit", "--path", &repo],
            0,
            |_, d| {
                assert_eq!(d["skillCount"], 1);
                assert_eq!(d["clean"], true);
            },
        ),
        case(
            "skills init",
            &["skills", "init", "--path", &fresh, "--dry-run"],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["created"].as_array().unwrap().len(), 6);
            },
        ),
        case(
            "skills add",
            &["skills", "add", "fresh-skill", "--repo", &repo, "--dry-run"],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["files"].as_array().unwrap().len(), 1);
            },
        ),
        case(
            "skills bundle",
            &["skills", "bundle", "--repo", &repo, "--dry-run"],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["fileCount"], 1);
            },
        ),
        case(
            "skills status",
            &["skills", "status", "--repo", &repo, "--home", &home],
            0,
            |_, d| {
                assert_eq!(d["lockfilePresent"], false);
                assert_eq!(d["drift"], false);
            },
        ),
        case(
            "skills clean",
            &["skills", "clean", "--repo", &repo, "--home", &home, "--dry-run"],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["lockfilePresent"], false);
                assert_eq!(d["removed"], json!([]));
            },
        ),
        case(
            "skills uninstall",
            &[
                "skills",
                "uninstall",
                "demo",
                "--repo",
                &repo,
                "--home",
                &home,
                "--dry-run",
            ],
            0,
            |w, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["name"], "demo");
                assert_eq!(d["lockUpdated"], false);
                same_path(&d["sourcePath"], &w.repo.join("skills/demo"));
            },
        ),
        case(
            "skills resolve",
            &["skills", "resolve", "--repo", &repo, "--home", &home, "--dry-run"],
            0,
            |_, d| {
                assert_eq!(d["dryRun"], true);
                assert_eq!(d["skillCount"], 1);
                assert_eq!(d["collisions"], json!([]));
            },
        ),
        case("sync", &["sync", "status"], 0, |_, d| {
            assert_eq!(d["configured"], false);
        }),
        case("cc", &["cc", "list"], 0, |_, d| {
            assert_eq!(d["mode"], "list");
            assert_eq!(d["plugins"][0]["id"], "demo-plugin@fake-market");
        }),
        case("update", &["update", "--check"], 0, |_, d| {
            assert_eq!(d["mode"], "check");
            assert_eq!(d["counts"]["skipped"], 2);
        }),
        case("obs otel status", &["obs", "otel", "status"], 0, |_, d| {
            assert_eq!(d["enabled"], false);
            assert_eq!(d["receiver"]["state"], "disabled");
            assert_eq!(d["settings"]["state"], "missing");
            assert!(d["settings"]["path"]
                .as_str()
                .unwrap()
                .ends_with(".claude/settings.json"));
            assert_eq!(d["events"]["count"], 0);
        }),
    ];
    cases.push(Case {
        key: "context checkpoint-status",
        argv: own(&["context", "checkpoint-status", "--checkpoint-at", "50000"]),
        stdin: Some(
            r#"{"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":1500,"cache_creation_input_tokens":500,"cache_read_input_tokens":1000}}}"#,
        ),
        exit: 0,
        check: |_, d| {
            assert_eq!(d["used_tokens"], 3000);
            assert_eq!(d["checkpoint_point"], 150_000);
            assert_eq!(d["at_checkpoint"], false);
        },
    });
    cases
}

#[test]
fn every_command_is_either_read_only_tested_or_given_a_reason() {
    let world = World::new("coverage");
    let table: BTreeSet<String> = COMMANDS.iter().map(|c| row_key(c.path)).collect();
    assert_eq!(table.len(), COMMANDS.len(), "paths are unique");
    let read_only: BTreeSet<String> = read_only_cases(&world)
        .iter()
        .map(|c| c.key.to_string())
        .collect();
    let skipped: BTreeSet<String> = NOT_READ_ONLY.iter().map(|(k, _)| k.to_string()).collect();
    let groups = group_keys();
    let mut classified = BTreeMap::<String, u32>::new();
    for key in read_only.iter().chain(&skipped).chain(&groups) {
        *classified.entry(key.clone()).or_default() += 1;
    }
    for (key, count) in &classified {
        assert!(table.contains(key), "{key} is not a registered command");
        assert_eq!(*count, 1, "{key} is classified twice");
    }
    let missing: Vec<&String> = table
        .iter()
        .filter(|k| !classified.contains_key(*k))
        .collect();
    assert!(missing.is_empty(), "unclassified commands: {missing:?}");
    for (key, reason) in NOT_READ_ONLY {
        assert!(!reason.is_empty(), "{key}");
    }
}

#[test]
fn read_only_commands_print_valid_envelopes_and_change_nothing() {
    let world = World::new("readonly");
    let before = world.snapshot();
    let mut ran = BTreeSet::new();
    for case in read_only_cases(&world) {
        let label = case.argv.join(" ");
        let mut argv: Vec<&str> = vec!["--json"];
        argv.extend(case.argv.iter().map(String::as_str));
        let run = world.run(&argv, case.stdin);
        let value = run.envelope();
        assert_envelope(&label, &run, &value, case.key, case.exit);
        if case.exit == 0 {
            (case.check)(&world, &value["data"]);
        } else if value.get("data").is_some() {
            (case.check)(&world, &value["data"]);
        }
        ran.insert(case.key);
    }
    assert!(ran.len() >= 30, "{} commands exercised", ran.len());
    assert_eq!(
        world.snapshot(),
        before,
        "read-only commands must leave the data dir, home, repository and import root as they were"
    );
}

#[test]
fn failing_read_only_commands_still_describe_the_failure() {
    let world = World::new("failures");
    let (run, value) = world.json(&["secret", "get", "srv-alpha", "API_KEY"]);
    assert_envelope("secret get", &run, &value, "secret get", 1);
    assert_eq!(value["error"]["code"], "not_found");

    let (run, value) = world.json(&["server", "info", "no-such-server"]);
    assert_envelope("server info", &run, &value, "server info", 1);
    assert_eq!(value["error"]["code"], "not_found");

    let (run, value) = world.json(&["skills", "diff", "--repo", &world.path(&world.repo)]);
    assert_envelope("skills diff", &run, &value, "skills diff", 1);
    assert_eq!(value["error"]["code"], "unhealthy");
    assert!(
        value["data"].is_object(),
        "an unhealthy result keeps its data"
    );

    let (run, value) = world.json(&["sync", "diff"]);
    assert_envelope("sync diff", &run, &value, "sync", 1);
    assert_eq!(value["error"]["code"], "sync");
}

#[test]
fn bare_groups_report_usage_or_not_implemented() {
    let world = World::new("groups");
    let groups = group_keys();
    assert!(groups.len() >= 8, "{groups:?}");
    for command in COMMANDS {
        let key = row_key(command.path);
        if !groups.contains(&key) {
            continue;
        }
        let (run, value) = world.json(command.path);
        let (exit, code) = if command.planned() {
            (1, "not_implemented")
        } else {
            (2, "usage")
        };
        assert_envelope(&key, &run, &value, &key, exit);
        assert_eq!(value["error"]["code"], code, "{key}");
    }
}

#[test]
fn compression_config_commands_round_trip_on_a_synthetic_data_dir() {
    let world = World::new("compression-cfg");
    let before = world.snapshot();
    for argv in [
        vec!["compression", "enable", "--provider", "rtk-only", "--dry-run"],
        vec!["compression", "use", "agent", "--dry-run"],
        vec!["compression", "set-provider", "headroom", "--dry-run"],
        vec!["compression", "disable", "--teardown", "--dry-run"],
        vec!["compression", "sync", "--dry-run"],
    ] {
        let (run, value) = world.json(&argv);
        assert_envelope(&argv.join(" "), &run, &value, &argv[..2].join(" "), 0);
        assert_eq!(value["data"]["dryRun"], true, "{argv:?}");
        assert_eq!(world.snapshot(), before, "{argv:?} must write nothing");
    }

    let (run, value) = world.json(&["compression", "enable", "--provider", "rtk-only"]);
    assert_envelope("compression enable", &run, &value, "compression enable", 0);
    assert_eq!(value["data"]["provider"], "rtk-only");
    let policy = world.data.join("compression.json");
    assert!(policy.is_file());
    assert!(!world.data.join("compression-env.sh").exists());
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&policy), 0o600);

    let (run, value) = world.json(&["compression", "use", "agent"]);
    assert_envelope("compression use", &run, &value, "compression use", 0);
    assert_eq!(value["data"]["preset"]["name"], "agent");
    let (_, status) = world.json(&["compression", "status"]);
    assert_eq!(status["data"]["preset"]["name"], "agent");

    let (run, value) = world.json(&["compression", "set-provider", "headroom"]);
    assert_envelope("compression set-provider", &run, &value, "compression set-provider", 0);
    let shims = world.data.join("compression-shims.zsh");
    let env = world.data.join("compression-env.sh");
    assert_eq!(mode(&shims), 0o644);
    assert_eq!(mode(&env), 0o600);
    let warnings = value["data"]["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w.as_str().unwrap().contains("not on PATH")),
        "no engine is installed here: {warnings:?}"
    );
    let registry: Value =
        serde_json::from_str(&std::fs::read_to_string(world.data.join("registry.json")).unwrap())
            .unwrap();
    let headroom = registry["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "headroom")
        .expect("the engine's MCP server is registered");
    assert_eq!(headroom["command"], "headroom");
    assert_eq!(headroom["source"], "plus:compression");
    assert!(registry["profiles"][0]["enabledServerIds"]
        .as_array()
        .unwrap()
        .contains(&headroom["id"]));

    let (run, first) = world.json(&["compression", "sync"]);
    assert_envelope("compression sync", &run, &first, "compression sync", 0);
    let after_first = world.snapshot();
    let (_, second) = world.json(&["compression", "sync"]);
    assert_eq!(first["data"]["actions"], second["data"]["actions"]);
    assert_eq!(world.snapshot(), after_first, "a second sync changes nothing");

    let (_, value) = world.json(&["compression", "set-provider", "none"]);
    assert_eq!(value["data"]["provider"], "none");
    assert!(!env.exists());
    assert!(shims.is_file(), "the shims stay, as in mcpm");
    let registry: Value =
        serde_json::from_str(&std::fs::read_to_string(world.data.join("registry.json")).unwrap())
            .unwrap();
    assert!(registry["servers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["name"] != "headroom"));

    let (run, value) = world.json(&["compression", "disable"]);
    assert_envelope("compression disable", &run, &value, "compression disable", 0);
    let (_, status) = world.json(&["compression", "status"]);
    assert_eq!(status["data"]["provider"], "none");
    assert_eq!(status["data"]["preset"]["name"], "agent", "disable keeps the policy");
}

#[test]
fn compression_sync_adopts_an_mcpm_policy_from_the_given_root_once() {
    let world = World::new("compression-legacy");
    let legacy = world.mcpm.join("compression.json");
    std::fs::write(
        &legacy,
        r#"{"provider": "rtk-only", "runtime": "hook", "active_preset": "agent"}"#,
    )
    .unwrap();
    let root = world.path(&world.mcpm);
    let (run, value) = world.json(&["compression", "sync", "--mcpm-root", &root]);
    assert_envelope("compression sync", &run, &value, "compression sync", 0);
    assert_eq!(value["data"]["provider"], "rtk-only");
    same_path(&value["data"]["adopted"]["from"], &legacy);
    assert!(
        world.data.join("compression.json").is_file(),
        "the adopted policy is saved in the data dir"
    );
    let (_, again) = world.json(&["compression", "sync", "--mcpm-root", &root]);
    assert!(again["data"]["adopted"].is_null());
    assert!(
        std::fs::read_to_string(&legacy).unwrap().contains("rtk-only"),
        "the mcpm file is only read"
    );
}

#[test]
fn mutating_commands_round_trip_on_a_synthetic_data_dir() {
    let world = World::new("mutating");

    let (run, value) = world.json(&[
        "server",
        "new",
        "gamma",
        "--command",
        "/bin/true",
        "--arg",
        "x",
    ]);
    assert_envelope("server new", &run, &value, "server new", 0);
    let (run, value) = world.json(&["server", "edit", "gamma", "--arg", "y"]);
    assert_envelope("server edit", &run, &value, "server edit", 0);
    assert_eq!(value["data"]["changed"], json!(["args"]));
    let (_, value) = world.json(&["server", "info", "gamma"]);
    assert_eq!(value["data"]["args"], json!(["y"]));
    let (_, found) = world.json(&["server", "search", "--offline", "--limit", "1"]);
    let catalog_name = found["data"]["results"][0]["name"]
        .as_str()
        .expect("the offline catalog has an entry")
        .to_string();
    let (run, value) = world.json(&["server", "install", &catalog_name, "--offline"]);
    assert_envelope("server install", &run, &value, "server install", 0);
    let (_, value) = world.json(&["server", "ls"]);
    assert_eq!(value["data"]["servers"].as_array().unwrap().len(), 4);
    let (run, value) = world.json(&["server", "install", &catalog_name, "--offline"]);
    assert_envelope("server install again", &run, &value, "server install", 1);
    assert_eq!(value["error"]["code"], "conflict");

    let run = world.run(
        &["--json", "secret", "set", "srv-alpha", "API_KEY"],
        Some(VAULTED_VALUE),
    );
    let value = run.envelope();
    assert_envelope("secret set", &run, &value, "secret set", 0);
    assert_eq!(value["data"]["stored"], true);
    let (run, value) = world.json(&["secret", "get", "srv-alpha", "API_KEY"]);
    assert_envelope("secret get", &run, &value, "secret get", 0);
    assert_eq!(value["data"]["set"], true);
    assert!(value["data"].get("value").is_none());
    let (_, value) = world.json(&["secret", "get", "srv-alpha", "API_KEY", "--reveal"]);
    assert_eq!(value["data"]["value"], VAULTED_VALUE);
    let on_disk: String = world
        .snapshot()
        .values()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect();
    assert!(
        !on_disk.contains(VAULTED_VALUE),
        "the vault never stores plaintext"
    );
    let (run, value) = world.json(&["secret", "rm", "srv-alpha", "API_KEY"]);
    assert_envelope("secret rm", &run, &value, "secret rm", 0);
    assert_eq!(value["data"]["removed"], true);
    let (run, value) = world.json(&["secret", "get", "srv-alpha", "API_KEY"]);
    assert_envelope("secret get after rm", &run, &value, "secret get", 1);

    let (repo, home) = (world.path(&world.repo), world.path(&world.home));
    let (run, value) = world.json(&[
        "skills",
        "sync",
        "--repo",
        &repo,
        "--home",
        &home,
        "--client",
        "claude-code",
    ]);
    assert_envelope("skills sync", &run, &value, "skills sync", 0);
    assert!(world.home.join(".claude/skills/demo/SKILL.md").is_file());
    let (run, value) = world.json(&["skills", "diff", "--repo", &repo, "--home", &home]);
    assert_envelope("skills diff after sync", &run, &value, "skills diff", 0);
    assert_eq!(value["data"]["clean"], true);

    let (run, value) = world.json(&["skills", "status", "--repo", &repo, "--home", &home]);
    assert_envelope("skills status", &run, &value, "skills status", 0);
    assert_eq!(value["data"]["lockfilePresent"], true);
    assert_eq!(value["data"]["lockedCount"], 1);
    assert_eq!(value["data"]["drift"], false);
    let output = world.home.join(".claude/skills/demo/SKILL.md");
    std::fs::remove_file(&output).unwrap();
    let (run, value) = world.json(&[
        "skills", "status", "--repo", &repo, "--home", &home, "--strict",
    ]);
    assert_envelope("skills status drift", &run, &value, "skills status", 1);
    assert_eq!(value["data"]["drift"], true);
    let (run, value) = world.json(&["skills", "sync", "--repo", &repo, "--home", &home]);
    assert_envelope("skills resync", &run, &value, "skills sync", 0);
    assert!(output.is_file());

    let before = world.snapshot();
    let (run, value) = world.json(&[
        "skills",
        "clean",
        "--repo",
        &repo,
        "--home",
        &home,
        "--dry-run",
    ]);
    assert_envelope("skills clean dry", &run, &value, "skills clean", 0);
    assert_eq!(value["data"]["dryRun"], true);
    assert!(!value["data"]["removed"].as_array().unwrap().is_empty());
    assert_eq!(world.snapshot(), before, "a clean dry run writes nothing");
    let (run, value) = world.json(&["skills", "clean", "--repo", &repo, "--home", &home]);
    assert_envelope("skills clean", &run, &value, "skills clean", 0);
    assert_eq!(value["data"]["lockfileRemoved"], true);
    assert!(!output.exists());
    let (run, value) = world.json(&["skills", "status", "--repo", &repo, "--home", &home]);
    assert_envelope("skills status after clean", &run, &value, "skills status", 0);
    assert_eq!(value["data"]["lockfilePresent"], false);

    let fresh = world.path(&world.base.join("fresh-skills"));
    let (run, value) = world.json(&["skills", "init", "--path", &fresh, "--name", "smoke"]);
    assert_envelope("skills init", &run, &value, "skills init", 0);
    let (run, value) = world.json(&["skills", "add", "smoke-skill", "--path", &fresh]);
    assert_envelope("skills add", &run, &value, "skills add", 0);
    let (run, value) = world.json(&["skills", "audit", "--path", &fresh]);
    assert_envelope("skills audit", &run, &value, "skills audit", 0);
    assert_eq!(value["data"]["skillCount"], 1);
    let zip = world.path(&world.base.join("smoke.zip"));
    let (run, value) = world.json(&["skills", "bundle", "--path", &fresh, "--output", &zip]);
    assert_envelope("skills bundle", &run, &value, "skills bundle", 0);
    let unpacked = world.path(&world.base.join("unpacked"));
    let (run, value) = world.json(&["skills", "unbundle", &zip, "--path", &unpacked]);
    assert_envelope("skills unbundle", &run, &value, "skills unbundle", 0);
    assert_eq!(value["data"]["names"], json!(["smoke-skill"]));
    assert_eq!(
        std::fs::read(world.base.join("fresh-skills/skills/smoke-skill/SKILL.md")).unwrap(),
        std::fs::read(world.base.join("unpacked/skills/smoke-skill/SKILL.md")).unwrap()
    );

    let (run, value) = world.json(&[
        "skills", "sync", "--repo", &fresh, "--home", &home, "--client", "claude-code",
    ]);
    assert_envelope("skills sync fresh", &run, &value, "skills sync", 0);
    let installed = world.home.join(".claude/skills/smoke-skill/SKILL.md");
    assert!(installed.is_file());
    let before = world.snapshot();
    let (run, value) = world.json(&[
        "skills",
        "uninstall",
        "smoke-skill",
        "--repo",
        &fresh,
        "--home",
        &home,
        "--dry-run",
    ]);
    assert_envelope("skills uninstall dry", &run, &value, "skills uninstall", 0);
    assert_eq!(value["data"]["dryRun"], true);
    assert_eq!(world.snapshot(), before, "an uninstall dry run writes nothing");
    let (run, value) = world.json(&[
        "skills",
        "uninstall",
        "smoke-skill",
        "--repo",
        &fresh,
        "--home",
        &home,
    ]);
    assert_envelope("skills uninstall", &run, &value, "skills uninstall", 0);
    assert_eq!(value["data"]["lockUpdated"], true);
    assert!(!installed.exists());
    assert!(!world.base.join("fresh-skills/skills/smoke-skill").exists());
    let (run, value) = world.json(&[
        "skills",
        "uninstall",
        "../escape",
        "--repo",
        &fresh,
        "--home",
        &home,
    ]);
    assert_envelope("skills uninstall refused", &run, &value, "skills uninstall", 1);

    let (run, value) = world.json(&["client", "sync", "--client", "cursor"]);
    assert_envelope("client sync", &run, &value, "client sync", 0);
    assert_eq!(value["data"]["dryRun"], false);
    let (_, value) = world.json(&["client", "ls"]);
    let cursor = value["data"]["clients"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "cursor")
        .unwrap()
        .clone();
    assert_eq!(cursor["managed"], true);
    assert_eq!(cursor["gateway"], "managed");

    for name in ["gamma", catalog_name.as_str()] {
        let (run, value) = world.json(&["server", "uninstall", name]);
        assert_envelope("server uninstall", &run, &value, "server uninstall", 0);
    }
    let (_, value) = world.json(&["server", "ls"]);
    assert_eq!(value["data"]["servers"].as_array().unwrap().len(), 2);
}
