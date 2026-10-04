//! Contract per command (MIG-GUI-0 step 6, D-058): one golden `--json` envelope per case under
//! `tests/fixtures/ctl-envelopes/`, produced by the real `toolportctl` against the synthetic world
//! of `common/ctl_world.rs`. The GUI parses these envelopes, so a change in their shape must be a
//! reviewed change: `CTL_ENVELOPE_BLESS=1 cargo test --test ctl_contract` rewrites the goldens
//! (rejected when `CI` is set) and `src/plus/bridge/data.ts` carries the matching TS shape.
//!
//! Adding a command to the table (`CASES` below, or `ctl_contract/cases.rs` for the bulk):
//!   1. `case("<command id>", &[read(...), apply(...), usage(...)])`; a writer gets a `--dry-run`
//!      step that must leave the world untouched, then the apply step, and a command with a
//!      required operand a `usage` step. `setup` steps prepare the world and write no golden.
//!   2. `CTL_ENVELOPE_BLESS=1 cargo test --no-default-features --test ctl_contract`, read the new
//!      files, `npx prettier --write src-tauri/tests/fixtures/ctl-envelopes`, and describe each
//!      `data` in `src/plus/types/<group>.ts`.
//! `contract_coverage` prints what is still uncovered; `CTL_CONTRACT_STRICT=1` turns that into a
//! failure. `no_output_carries_a_canary_secret` runs every case again with a canary in the vault.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use conduit_lib::plus::ctl::{registry, SCHEMA_VERSION};
use serde_json::{json, Value};

#[path = "ctl_contract/cases.rs"]
mod cases;
#[path = "common/claude_stub.rs"]
mod claude_stub;
#[path = "common/ctl_fixtures.rs"]
mod ctl_fixtures;
#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/golden.rs"]
mod golden;
#[path = "common/loads_world.rs"]
mod loads_world;
#[path = "common/normalize.rs"]
mod normalize;
#[path = "common/sources_world.rs"]
mod sources_world;

use ctl_fixtures::PASSPHRASE;
use ctl_world::{CtlWorld, FAKE_SECRET};

const VAULTED: &str = "FAKE-vaulted-value-1c9e";
const CANARY: &str = "FAKE-canary-in-the-vault-5b07";
const COUNCIL_KEY: &str = "FAKE-council-key-77c2";
const RUN_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy)]
struct Step {
    label: &'static str,
    argv: &'static [&'static str],
    stdin: Option<&'static str>,
    exit: i32,
    writes: bool,
    reveals: bool,
    golden: bool,
    env: &'static [(&'static str, &'static str)],
}

/// A step that must leave the world as it was: a read or a `--dry-run`.
const fn read(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        label,
        argv,
        stdin: None,
        exit: 0,
        writes: false,
        reveals: false,
        golden: true,
        env: &[],
    }
}

/// A step whose argv fails the parser (exit 2): nothing runs, so the world must stay as it was.
const fn usage(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        exit: 2,
        ..read(label, argv)
    }
}

/// Prepares the world for the steps after it; checked for exit code and leaks, no golden.
const fn setup(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        writes: true,
        golden: false,
        ..read(label, argv)
    }
}

const fn apply(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        writes: true,
        ..read(label, argv)
    }
}

impl Step {
    const fn stdin(self, text: &'static str) -> Self {
        Self {
            stdin: Some(text),
            ..self
        }
    }

    const fn exit(self, code: i32) -> Self {
        Self { exit: code, ..self }
    }

    const fn reveals(self) -> Self {
        Self {
            reveals: true,
            ..self
        }
    }

    const fn env(self, env: &'static [(&'static str, &'static str)]) -> Self {
        Self { env, ..self }
    }
}

struct Case {
    id: &'static str,
    prepare: Option<fn(&CtlWorld)>,
    steps: &'static [Step],
}

const fn case(id: &'static str, steps: &'static [Step]) -> Case {
    Case {
        id,
        prepare: None,
        steps,
    }
}

fn sources_home(world: &CtlWorld) {
    sources_world::build_in(&world.base);
}

fn loads_home(world: &CtlWorld) {
    loads_world::build_in(
        &world.base,
        conduit_lib::plus::context::layers::MANAGED_LOCAL_HEADER,
    );
}

/// The client-folder home with a `claude` that is the stream-json stub
/// (`fixtures/loads/claude-stub.sh`) and a user skill Claude Code does not list.
fn measure_home(world: &CtlWorld) {
    let loaded = loads_world::build_in(
        &world.base,
        conduit_lib::plus::context::layers::MANAGED_LOCAL_HEADER,
    );
    claude_stub::ClaudeStub::install(&world.claude, &world.base);
    let unlisted = loaded.claude.join("skills/handoff/SKILL.md");
    std::fs::create_dir_all(unlisted.parent().unwrap()).unwrap();
    std::fs::write(
        unlisted,
        "---\nname: handoff\ndescription: \"Write a handoff\nfor the next session\"\n---\nBody\n",
    )
    .unwrap();
}

/// A case that runs over the sources fixture home of the GUI-wave contract, section 13.
const fn sources_case(id: &'static str, steps: &'static [Step]) -> Case {
    Case {
        prepare: Some(sources_home),
        ..case(id, steps)
    }
}

/// A case whose world needs files the shared fixture does not have (a git repository, transcripts).
const fn prepared(id: &'static str, prepare: fn(&CtlWorld), steps: &'static [Step]) -> Case {
    Case {
        id,
        prepare: Some(prepare),
        steps,
    }
}

/// `{repo}`, `{home}`, `{data}`, `{base}` and `{mock}` in an argv word stand for the synthetic world.
const CASES: &[Case] = &[
    case("commands", &[read("", &["commands"])]),
    case("status", &[read("", &["status"])]),
    case("doctor", &[read("", &["doctor"])]),
    case("server ls", &[read("", &["server", "ls"])]),
    case("server info", &[read("", &["server", "info", "alpha"])]),
    case("profile ls", &[read("", &["profile", "ls"])]),
    case("client ls", &[read("", &["client", "ls"])]),
    case("client direct ls", &[read("", &["client", "direct", "ls"])]),
    sources_case(
        "skills ls",
        &[
            read("repo", &["skills", "ls", "--repo", "{repo}"]),
            read("library", &["skills", "ls", "--source", "library"]),
            read("source", &["skills", "ls", "--source", "repo:odh"]),
        ],
    ),
    sources_case(
        "sources ls",
        &[
            read("summary", &["sources", "ls"]),
            read("items", &["sources", "ls", "--items"]),
            read("partial", &["sources", "ls", "--budget", "repo=0"]),
            read("org", &["sources", "ls", "--source", "org", "--items"]),
        ],
    ),
    sources_case(
        "sources root ls",
        &[read("", &["sources", "root", "ls"])],
    ),
    sources_case(
        "sources root add",
        &[
            read("preview", &["sources", "root", "add", "{home}/work", "--dry-run"]),
            apply("apply", &["sources", "root", "add", "{home}/work"]),
        ],
    ),
    sources_case(
        "sources root rm",
        &[
            read("preview", &["sources", "root", "rm", "{home}/dups", "--dry-run"]),
            apply("apply", &["sources", "root", "rm", "{home}/dups"]),
        ],
    ),
    case(
        "skills lint",
        &[read("", &["skills", "lint", "--repo", "{repo}"])],
    ),
    case(
        "agents ls",
        &[read("", &["agents", "ls", "--repo", "{repo}"])],
    ),
    case(
        "styles ls",
        &[read("", &["styles", "ls", "--repo", "{repo}"])],
    ),
    case("auth statusline", &[read("", &["auth", "statusline"])]),
    case(
        "compression status",
        &[read("", &["compression", "status"])],
    ),
    case("sync status", &[read("", &["sync", "status"])]),
    case("cc list", &[read("", &["cc", "list"])]),
    case("inspect", &[read("", &["inspect", "alpha"])]),
    case("profile inspect", &[read("", &["profile", "inspect"])]),
    case(
        "server search",
        &[read("", &["server", "search", "--offline", "--limit", "3"])],
    ),
    case(
        "agents lint",
        &[read("", &["agents", "lint", "--repo", "{repo}"])],
    ),
    case(
        "agents diff",
        &[read("", &["agents", "diff", "--repo", "{repo}"])],
    ),
    case(
        "agents status",
        &[read("", &["agents", "status", "--repo", "{repo}"])],
    ),
    case(
        "styles lint",
        &[read("", &["styles", "lint", "--repo", "{repo}"])],
    ),
    case(
        "styles diff",
        &[read("", &["styles", "diff", "--repo", "{repo}"])],
    ),
    case(
        "styles status",
        &[read("", &["styles", "status", "--repo", "{repo}"])],
    ),
    case(
        "skills diff",
        &[read("", &["skills", "diff", "--repo", "{repo}"]).exit(1)],
    ),
    case(
        "skills status",
        &[read("", &["skills", "status", "--repo", "{repo}"])],
    ),
    case(
        "skills audit",
        &[read("", &["skills", "audit", "--repo", "{repo}"])],
    ),
    case("skills tap ls", &[read("", &["skills", "tap", "ls"])]),
    case("context folders", &[read("", &["context", "folders"])]),
    case("context status", &[read("", &["context", "status"])]),
    case(
        "context client list",
        &[read("", &["context", "client", "list"])],
    ),
    case(
        "context profile list",
        &[read("", &["context", "profile", "list"])],
    ),
    case(
        "compression presets",
        &[read("", &["compression", "presets"])],
    ),
    case("compression pin", &[read("", &["compression", "pin"])]),
    case("compression env", &[read("", &["compression", "env"])]),
    case(
        "compression doctor",
        &[read("", &["compression", "doctor"])],
    ),
    case(
        "compression ledger summary",
        &[read("", &["compression", "ledger", "summary"])],
    ),
    case("auth hook", &[read("", &["auth", "hook"])]),
    case(
        "council doctor",
        &[read("", &["council", "doctor"]).exit(1)],
    ),
    case("council tools", &[read("", &["council", "tools"])]),
    case("mcp doctor", &[read("", &["mcp", "doctor"]).exit(1)]),
    case(
        "update",
        &[
            read("check", &["update", "--check"]),
            read("preview", &["update", "--apply", "--dry-run"]),
        ],
    ),
    case("mcp tools", &[read("", &["mcp", "tools"])]),
    case("obs otel status", &[read("", &["obs", "otel", "status"])]),
    case(
        "profile create",
        &[
            read("preview", &["profile", "create", "demo", "--dry-run"]),
            apply("apply", &["profile", "create", "demo"]),
            read("after", &["profile", "ls"]),
        ],
    ),
    case(
        "profile edit",
        &[
            read(
                "preview",
                &[
                    "profile",
                    "edit",
                    "default",
                    "--add-server",
                    "beta",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["profile", "edit", "default", "--add-server", "beta"],
            ),
        ],
    ),
    case(
        "profile rm",
        &[
            apply("setup", &["profile", "create", "scratch"]),
            read("preview", &["profile", "rm", "scratch", "--dry-run"]),
        ],
    ),
    case(
        "server uninstall",
        &[read(
            "preview",
            &["server", "uninstall", "alpha", "--dry-run"],
        )],
    ),
    case(
        "client sync",
        &[
            read("preview", &["client", "sync", "--dry-run"]),
            apply("apply", &["client", "sync"]),
        ],
    ),
    case(
        "skills sync",
        &[
            read(
                "preview",
                &["skills", "sync", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["skills", "sync", "--repo", "{repo}"]),
        ],
    ),
    case(
        "secret set",
        &[
            apply("apply", &["secret", "set", "alpha", "API_TOKEN"]).stdin(VAULTED),
            read("get", &["secret", "get", "alpha", "API_TOKEN"]),
        ],
    ),
];

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn world(tag: &str) -> CtlWorld {
    CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"))
}

fn expand(world: &CtlWorld, word: &str) -> String {
    word.replace("{repo}", &world.path(&world.repo))
        .replace("{home}", &world.path(&world.home))
        .replace("{data}", &world.path(&world.data))
        .replace("{base}", &world.path(&world.base))
        .replace("{mock}", &world.mock)
}

fn run_ctl(
    world: &CtlWorld,
    argv: &[String],
    stdin: Option<&str>,
    extra_env: &[(&str, &str)],
) -> Run {
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
    for (key, value) in extra_env {
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
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    let output = match receiver.recv_timeout(RUN_TIMEOUT) {
        Ok(output) => output.expect("wait for toolportctl"),
        Err(_) => {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            panic!("toolportctl {argv:?} did not finish in {RUN_TIMEOUT:?}");
        }
    };
    Run {
        code: output.status.code().expect("exit code, not a signal"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn scrub(envelope: &Value) -> Value {
    let text = serde_json::to_string(envelope).unwrap();
    serde_json::from_str(&text.replace(VAULTED, "<VAULTED>")).unwrap()
}

fn stem(case: &Case, step: &Step) -> String {
    if step.label.is_empty() {
        golden::file_stem(case.id)
    } else {
        format!("{}.{}", golden::file_stem(case.id), step.label)
    }
}

fn all_cases() -> impl Iterator<Item = &'static Case> {
    CASES.iter().chain(cases::MORE)
}

fn world_for(case: &Case) -> CtlWorld {
    let world = world(&golden::file_stem(case.id));
    if let Some(prepare) = case.prepare {
        prepare(&world);
    }
    world
}

fn run_case(case: &Case) {
    let world = world_for(case);
    for step in case.steps {
        let argv: Vec<String> = step.argv.iter().map(|w| expand(&world, w)).collect();
        let before = world.snapshot();
        let run = run_ctl(&world, &argv, step.stdin, step.env);
        let name = format!("{} {}", case.id, step.label);

        assert_eq!(
            run.code, step.exit,
            "{name}: exit code\nstdout: {}\nstderr: {}",
            run.stdout, run.stderr
        );
        let lines: Vec<&str> = run.stdout.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "{name}: stdout is exactly one envelope line, got {:?}",
            run.stdout
        );
        let envelope: Value = serde_json::from_str(lines[0])
            .unwrap_or_else(|e| panic!("{name}: stdout is not JSON: {e}"));
        assert_eq!(envelope["schemaVersion"], SCHEMA_VERSION, "{name}");
        assert_eq!(
            envelope["ok"],
            step.exit == 0,
            "{name}: ok follows the exit code"
        );
        assert!(envelope["command"].is_string(), "{name}");
        if step.exit == 0 {
            assert!(
                envelope.get("data").is_some(),
                "{name}: ok envelope without data"
            );
        } else {
            assert!(
                envelope["error"]["code"].is_string() && envelope["error"]["message"].is_string(),
                "{name}: failed envelope without error code and message"
            );
        }

        let mut canaries = vec![FAKE_SECRET, CANARY, PASSPHRASE];
        canaries.extend(step.env.iter().map(|(_, value)| *value));
        if !step.reveals {
            canaries.push(VAULTED);
        }
        for canary in canaries {
            assert!(
                !run.stdout.contains(canary) && !run.stderr.contains(canary),
                "{name}: leaked a secret value"
            );
        }
        if !step.writes {
            let after = world.snapshot();
            let changed: Vec<String> = after
                .iter()
                .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
                .map(|(path, _)| path.display().to_string())
                .chain(
                    before
                        .keys()
                        .filter(|p| !after.contains_key(*p))
                        .map(|p| format!("removed {}", p.display())),
                )
                .map(|p| {
                    normalize::world_roots(&world)
                        .iter()
                        .fold(p, |acc, root| acc.replace(root, "<WORLD>"))
                })
                .collect();
            assert!(
                changed.is_empty(),
                "{name}: a read or dry-run step changed the world: {changed:?}"
            );
        }

        if step.golden {
            golden::assert_golden(
                &stem(case, step),
                &json!({
                    "argv": step.argv,
                    "exitCode": step.exit,
                    "envelope": normalize::normalize(&world, &scrub(&envelope)),
                }),
            );
        }
    }
}

#[test]
fn every_case_matches_its_golden_envelope() {
    let mut failures = Vec::new();
    for case in all_cases() {
        let result = std::panic::catch_unwind(|| run_case(case));
        if let Err(payload) = result {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            failures.push(format!("{}: {message}", case.id));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn covered_ids() -> BTreeSet<&'static str> {
    all_cases().map(|c| c.id).collect()
}

fn command_ids() -> Vec<String> {
    registry()["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "command")
        .map(|row| row["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn every_case_names_a_command_and_every_golden_belongs_to_a_case() {
    let ids: BTreeSet<String> = command_ids().into_iter().collect();
    for case in all_cases() {
        assert!(
            ids.contains(case.id),
            "case `{}` is not a registry command",
            case.id
        );
        assert!(!case.steps.is_empty(), "{}", case.id);
        let labels: BTreeSet<&str> = case.steps.iter().map(|s| s.label).collect();
        assert_eq!(
            labels.len(),
            case.steps.len(),
            "{}: step labels repeat",
            case.id
        );
        if case.steps.len() > 1 {
            assert!(
                !labels.contains(""),
                "{}: several steps need labels",
                case.id
            );
        }
    }
    let data = registry();
    let rows: Vec<&Value> = data["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "command")
        .collect();
    for case in all_cases() {
        for step in case.steps {
            let command = rows
                .iter()
                .filter(|row| {
                    let words = row["path"].as_array().unwrap();
                    step.argv.len() >= words.len()
                        && words.iter().zip(step.argv).all(|(w, a)| w == a)
                })
                .max_by_key(|row| row["path"].as_array().unwrap().len())
                .unwrap_or_else(|| {
                    panic!(
                        "{} {}: argv starts with no registry command",
                        case.id, step.label
                    )
                });
            if step.writes || step.exit == 2 {
                continue;
            }
            let flag = command["preview"]["flag"].as_str().unwrap_or("");
            assert!(
                command["baseTier"] == "read" || (!flag.is_empty() && step.argv.contains(&flag)),
                "{} {}: a read step runs `{}`, which writes unless it previews with {flag}",
                case.id,
                step.label,
                command["id"]
            );
        }
    }
    let mut owned: BTreeSet<String> = BTreeSet::new();
    for case in all_cases() {
        for step in case.steps.iter().filter(|step| step.golden) {
            owned.insert(stem(case, step));
        }
    }
    for entry in std::fs::read_dir(golden::root()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = name.strip_suffix(".json").unwrap_or(&name);
        assert!(owned.contains(stem), "golden {name} belongs to no case");
    }
}

#[test]
fn contract_coverage() {
    let covered = covered_ids();
    let ids = command_ids();
    let missing: Vec<&String> = ids
        .iter()
        .filter(|id| !covered.contains(id.as_str()))
        .collect();
    let rows = registry();
    let rows = rows["commands"].as_array().unwrap();
    let writers_without_preview: Vec<&str> = all_cases()
        .filter(|case| {
            rows.iter().any(|row| {
                let flag = row["preview"]["flag"].as_str().unwrap_or("");
                row["id"] == case.id
                    && row["dryRun"] == true
                    && row["baseTier"] != "read"
                    && !case
                        .steps
                        .iter()
                        .any(|s| !s.writes && s.argv.contains(&flag))
            })
        })
        .map(|case| case.id)
        .collect();
    assert!(
        writers_without_preview.is_empty(),
        "commands that can preview need a --dry-run step: {writers_without_preview:?}"
    );
    let mut seen = BTreeSet::new();
    for case in all_cases() {
        assert!(seen.insert(case.id), "case `{}` appears twice", case.id);
    }
    eprintln!(
        "ctl contract: {} of {} commands have a golden envelope; {} still to add",
        ids.len() - missing.len(),
        ids.len(),
        missing.len()
    );
    if std::env::var_os("CTL_CONTRACT_STRICT").is_some_and(|v| !v.is_empty() && v != "0") {
        assert!(missing.is_empty(), "no golden envelope for: {missing:?}");
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// Every case runs again in a world whose vault holds a canary: it must not show up in a
/// command's output, in a file the command wrote, or in a golden.
#[test]
fn no_output_carries_a_canary_secret() {
    let mut runs = 0;
    for case in all_cases() {
        let world = world_for(case);
        let seeded = run_ctl(
            &world,
            &["secret", "set", "alpha", "CANARY_KEY"].map(String::from),
            Some(CANARY),
            &[],
        );
        assert_eq!(seeded.code, 0, "{}: {}", case.id, seeded.stdout);
        let mut secrets = vec![CANARY, PASSPHRASE];
        for step in case.steps {
            let argv: Vec<String> = step.argv.iter().map(|w| expand(&world, w)).collect();
            let run = run_ctl(&world, &argv, step.stdin, step.env);
            runs += 1;
            let mut shown = secrets.clone();
            shown.push(FAKE_SECRET);
            shown.extend(step.env.iter().map(|(_, value)| *value));
            if !step.reveals {
                shown.push(VAULTED);
            }
            for secret in shown {
                assert!(
                    !run.stdout.contains(secret) && !run.stderr.contains(secret),
                    "{} {}: leaked a secret value",
                    case.id,
                    step.label
                );
            }
            secrets.extend(step.env.iter().map(|(_, value)| *value));
        }
        for (path, bytes) in world.snapshot() {
            for secret in secrets.iter().chain(&[VAULTED]) {
                assert!(
                    !contains(&bytes, secret),
                    "{}: {} holds a secret in plain text",
                    case.id,
                    path.display()
                );
            }
        }
    }
    assert!(runs > 100, "{runs} runs");

    for entry in std::fs::read_dir(golden::root()).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for secret in [FAKE_SECRET, CANARY, VAULTED, PASSPHRASE, COUNCIL_KEY] {
            assert!(
                !text.contains(secret),
                "golden {} contains a secret value",
                entry.path().display()
            );
        }
    }
}
