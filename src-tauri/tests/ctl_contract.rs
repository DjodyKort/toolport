//! Contract per command (MIG-GUI-0 step 6, D-058): one golden `--json` envelope per case under
//! `tests/fixtures/ctl-envelopes/`, produced by the real `toolportctl` against the synthetic world
//! of `common/ctl_world.rs`. The GUI parses these envelopes, so a change in their shape must be a
//! reviewed change: `CTL_ENVELOPE_BLESS=1 cargo test --test ctl_contract` rewrites the goldens
//! (rejected when `CI` is set) and `src/plus/bridge/data.ts` carries the matching TS shape.
//!
//! Adding a command to the table:
//!   1. `case("<command id>", &[step(...)])` in `cases()`; a writer gets a `--dry-run` step that
//!      must leave the world untouched, then the apply step.
//!   2. `CTL_ENVELOPE_BLESS=1 cargo test --no-default-features --test ctl_contract`, read the new
//!      file, and describe its `data` in `src/plus/bridge/data.ts`.
//! `contract_coverage` prints what is still uncovered; `CTL_CONTRACT_STRICT=1` turns that into a
//! failure.

#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use conduit_lib::plus::ctl::{registry, SCHEMA_VERSION};
use regex::Regex;
use serde_json::{json, Value};

#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/golden.rs"]
mod golden;

use ctl_world::{CtlWorld, FAKE_SECRET};

const VAULTED: &str = "FAKE-vaulted-value-1c9e";
const RUN_TIMEOUT: Duration = Duration::from_secs(120);

/// Keys whose value changes between machines or releases; the golden keeps only that they exist.
const MASKED_KEYS: &[&str] = &["version", "pid", "elapsedMs", "durationMs", "tookMs"];

#[derive(Clone, Copy)]
struct Step {
    label: &'static str,
    argv: &'static [&'static str],
    stdin: Option<&'static str>,
    exit: i32,
    writes: bool,
    reveals: bool,
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
}

struct Case {
    id: &'static str,
    steps: &'static [Step],
}

const fn case(id: &'static str, steps: &'static [Step]) -> Case {
    Case { id, steps }
}

/// `{repo}`, `{home}`, `{data}` and `{mock}` in an argv word stand for the synthetic world.
const CASES: &[Case] = &[
    case("commands", &[read("", &["commands"])]),
    case("status", &[read("", &["status"])]),
    case("doctor", &[read("", &["doctor"])]),
    case("server ls", &[read("", &["server", "ls"])]),
    case("server info", &[read("", &["server", "info", "alpha"])]),
    case("profile ls", &[read("", &["profile", "ls"])]),
    case("client ls", &[read("", &["client", "ls"])]),
    case("client direct ls", &[read("", &["client", "direct", "ls"])]),
    case(
        "skills ls",
        &[read("", &["skills", "ls", "--repo", "{repo}"])],
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
        .replace("{mock}", &world.mock)
}

fn run_ctl(world: &CtlWorld, argv: &[String], stdin: Option<&str>) -> Run {
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

fn snapshot(world: &CtlWorld) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(dir: &std::path::Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "lock") {
                continue;
            } else if let Ok(bytes) = std::fs::read(&path) {
                files.insert(path, bytes);
            }
        }
    }
    let mut files = BTreeMap::new();
    for dir in [&world.data, &world.home, &world.repo] {
        collect(dir, &mut files);
    }
    files
}

fn mask(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if MASKED_KEYS.contains(&key.as_str()) && !inner.is_null() {
                    *inner = json!("<masked>");
                } else {
                    mask(inner);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mask),
        _ => {}
    }
}

fn world_roots(world: &CtlWorld) -> Vec<String> {
    let mut roots = Vec::new();
    if let Ok(real) = std::fs::canonicalize(&world.base) {
        roots.push(real.to_string_lossy().into_owned());
    }
    roots.push(world.path(&world.base));
    roots.dedup();
    roots
}

fn bin_dirs() -> Vec<String> {
    let dir = std::path::Path::new(env!("CARGO_BIN_EXE_toolportctl"))
        .parent()
        .unwrap();
    let mut dirs = vec![dir.to_string_lossy().into_owned()];
    if let Ok(real) = std::fs::canonicalize(dir) {
        dirs.push(real.to_string_lossy().into_owned());
    }
    dirs
}

/// Paths and times that differ per run become placeholders, so the golden holds the shape and
/// the stable values only.
fn normalize(world: &CtlWorld, envelope: &Value) -> Value {
    let time =
        Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?").unwrap();
    let mut text = serde_json::to_string(envelope).unwrap();
    for root in world_roots(world) {
        text = text.replace(&root, "<WORLD>");
    }
    text = text.replace(&world.mock, "<MOCK>");
    for dir in bin_dirs() {
        text = text.replace(&dir, "<BIN>");
    }
    let text = time.replace_all(&text, "<TIME>").into_owned();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    mask(&mut value);
    value
}

fn stem(case: &Case, step: &Step) -> String {
    if step.label.is_empty() {
        golden::file_stem(case.id)
    } else {
        format!("{}.{}", golden::file_stem(case.id), step.label)
    }
}

fn run_case(case: &Case) {
    let world = world(&golden::file_stem(case.id));
    for step in case.steps {
        let argv: Vec<String> = step.argv.iter().map(|w| expand(&world, w)).collect();
        let before = snapshot(&world);
        let run = run_ctl(&world, &argv, step.stdin);
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

        let mut canaries = vec![FAKE_SECRET];
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
            let after = snapshot(&world);
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
                    world_roots(&world)
                        .iter()
                        .fold(p, |acc, root| acc.replace(root, "<WORLD>"))
                })
                .collect();
            assert!(
                changed.is_empty(),
                "{name}: a read or dry-run step changed the world: {changed:?}"
            );
        }

        golden::assert_golden(
            &stem(case, step),
            &json!({
                "argv": step.argv,
                "exitCode": step.exit,
                "envelope": normalize(&world, &envelope),
            }),
        );
    }
}

#[test]
fn every_case_matches_its_golden_envelope() {
    let mut failures = Vec::new();
    for case in CASES {
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
    CASES.iter().map(|c| c.id).collect()
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
    for case in CASES {
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
    for case in CASES {
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
            if step.writes {
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
    for case in CASES {
        for step in case.steps {
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
    let writers_without_preview: Vec<&str> = CASES
        .iter()
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
