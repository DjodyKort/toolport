//! Contract per `toolportctl` command (D-058): one golden `--json` envelope per case under
//! `plus/fixtures/ctl-envelopes/`, produced by the real binary against the synthetic world of
//! `common/world.rs`. The GUI bridge and scripts parse these envelopes, so a change in their shape
//! must be a reviewed change: `CTL_ENVELOPE_BLESS=1 cargo test -p toolport-plus-bins --test
//! plus_ctl_contract` rewrites the goldens (rejected when `CI` is set).
//!
//! A command is classified in the command table (`Row::smoke`), not here: `every_command_is_...`
//! walks the table, so a new command fails until its row says how it is covered. Adding a case:
//! `case("<command id>", &[read(...), apply(...), usage(...)])`; a writer gets a `--dry-run` step
//! that leaves the world as it was, then the apply step.

use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use toolport_plus::ctl::{registry, rows, Smoke, Tier, COMMANDS, SCHEMA_VERSION};

mod common;
use common::world::{World, FAKE_SECRET, SECRET_KEY};
use common::{golden, normalize};

/// Goldens that still hold the previous line's text; they are re-blessed in the golden-bump commit.
const BUMP_PENDING: &[&str] = &[
    "commands",
    "mcp-call.servers_add_profile_tag",
    "mcp-call.servers_remove_profile_tag",
    "mcp-doctor",
    "mcp-install.doctor",
    "mcp-tools",
    "mcp-uninstall.doctor",
    "server-edit.after",
    "server-edit.usage",
    "server-info",
    "server-ls",
    "server-new.after",
    "server-new.usage",
];

#[derive(Clone, Copy)]
struct Step {
    label: &'static str,
    argv: &'static [&'static str],
    stdin: Option<&'static str>,
    exit: i32,
    writes: bool,
    golden: bool,
    hook: Option<fn(&World)>,
}

/// A step that must leave the world as it was: a read or a `--dry-run`.
const fn read(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        label,
        argv,
        stdin: None,
        exit: 0,
        writes: false,
        golden: true,
        hook: None,
    }
}

/// A step whose argv fails the parser (exit 2): nothing runs, so the world must stay as it was.
const fn usage(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        exit: 2,
        ..read(label, argv)
    }
}

const fn apply(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        writes: true,
        ..read(label, argv)
    }
}

/// Prepares the world for the steps after it with a command: checked for exit code and leaks,
/// no golden.
const fn setup(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        golden: false,
        ..apply(label, argv)
    }
}

/// Reshapes the world for the steps after it; runs no command and records no golden.
const fn hook(label: &'static str, prepare: fn(&World)) -> Step {
    Step {
        golden: false,
        hook: Some(prepare),
        ..read(label, &[])
    }
}

impl Step {
    const fn exit(self, exit: i32) -> Self {
        Self { exit, ..self }
    }

    const fn stdin(self, stdin: &'static str) -> Self {
        Self {
            stdin: Some(stdin),
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

fn gateway_without_the_seam(world: &World) {
    world.stub_gateway(b"an upstream gateway, built without the fork seam");
}

fn explicit_default_access(world: &World) {
    let mut registry = world.registry();
    registry["version"] = json!(3);
    registry["servers"][0]["enabled"] = json!(true);
    registry["servers"][1]["enabled"] = json!(true);
    registry["defaultAccessProfileId"] = json!("default");
    registry["profiles"] = json!([
        {"id": "default", "name": "Default", "enabledServerIds": ["srv-alpha"]},
        {"id": "focus", "name": "Focus", "enabledServerIds": []},
    ]);
    registry["clientScopes"] = json!({"claude-code": "focus"});
    world.write_registry(&registry);
}

fn unreadable_registry(world: &World) {
    std::fs::write(world.data.join("registry.json"), "{ not json").unwrap();
}

const CASES: &[Case] = &[
    case("commands", &[read("", &["commands"])]),
    case(
        "status",
        &[
            read("", &["status"]),
            hook("explicit-access-sets", explicit_default_access),
            read("access-sets", &["status"]),
            hook("unreadable", unreadable_registry),
            read("unreadable-registry", &["status"]),
        ],
    ),
    case(
        "doctor",
        &[
            read("", &["doctor"]),
            hook("no-seam", gateway_without_the_seam),
            read("gateway-without-seam", &["doctor"]),
            hook("unreadable", unreadable_registry),
            read("unreadable-registry", &["doctor"]).exit(1),
        ],
    ),
    case("server ls", &[read("", &["server", "ls"])]),
    case("server info", &[read("", &["server", "info", "alpha"])]),
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
            apply(
                "env",
                &[
                    "server",
                    "edit",
                    "gamma",
                    "--env",
                    "JAVA_HOME=/opt/jdk",
                    "--env",
                    "MODE=fast",
                    "--unset-env",
                    "NEVER_SET",
                ],
            ),
            apply(
                "env-refused",
                &["server", "edit", "gamma", "--env", "API_KEY=plain"],
            )
            .exit(1),
            apply(
                "unknown",
                &["server", "edit", "no-such-server", "--arg", "q"],
            )
            .exit(1),
            usage("usage", &["server", "edit"]),
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
        "server restart",
        &[
            read("preview", &["server", "restart", "alpha", "--dry-run"]),
            apply(
                "not-running",
                &["server", "restart", "alpha", "--wait", "1"],
            ),
        ],
    ),
    case("mcp doctor", &[read("", &["mcp", "doctor"]).exit(1)]),
    case("mcp tools", &[read("", &["mcp", "tools"])]),
    case(
        "mcp install",
        &[
            apply("apply", &["mcp", "install"]),
            apply("profile", &["mcp", "install", "--profile", "default"]),
            hook("explicit-access-sets", explicit_default_access),
            apply("access-set", &["mcp", "install", "--access-set", "focus"]),
            read("doctor", &["mcp", "doctor"]),
            usage("bogus", &["mcp", "install", "--bogus"]),
            read(
                "unknown-access-set",
                &["mcp", "install", "--access-set", "nope"],
            )
            .exit(1),
        ],
    ),
    case(
        "mcp uninstall",
        &[
            apply("setup", &["mcp", "install"]),
            apply("apply", &["mcp", "uninstall"]),
            apply("again", &["mcp", "uninstall"]),
            read("doctor", &["mcp", "doctor"]),
        ],
    ),
    case(
        "mcp call",
        &[
            read("where_am_i", &["mcp", "call", "where_am_i"]),
            read(
                "flow_diagram",
                &["mcp", "call", "flow_diagram", "--args", "{}"],
            ),
            read("doctor", &["mcp", "call", "doctor"]),
            read(
                "invalid",
                &["mcp", "call", "where_am_i", "--args", r#"{"depth":2}"#],
            )
            .exit(1),
            usage("unknown", &["mcp", "call", "where_am"]),
            usage("operand", &["mcp", "call"]),
            usage(
                "secret",
                &[
                    "mcp",
                    "call",
                    "where_am_i",
                    "--args",
                    r#"{"config":{"env":{"API_KEY":"FAKE-inline-secret-9d41"}}}"#,
                ],
            ),
            read("stdin", &["mcp", "call", "where_am_i", "--args-stdin"]).stdin("{}"),
            apply(
                "servers_add_profile_tag",
                &[
                    "mcp",
                    "call",
                    "servers_add_profile_tag",
                    "--args",
                    r#"{"name":"beta","profile_tag":"Default"}"#,
                ],
            ),
            apply(
                "servers_remove_profile_tag",
                &[
                    "mcp",
                    "call",
                    "servers_remove_profile_tag",
                    "--args",
                    r#"{"name":"beta","profile_tag":"Default"}"#,
                ],
            ),
            read(
                "servers_set_mode.refused",
                &[
                    "mcp",
                    "call",
                    "servers_set_mode",
                    "--args",
                    r#"{"name":"alpha","mode":"direct"}"#,
                ],
            )
            .exit(1),
            apply(
                "servers_set_mode",
                &[
                    "mcp",
                    "call",
                    "servers_set_mode",
                    "--args",
                    r#"{"name":"alpha","mode":"direct","confirm":true}"#,
                ],
            ),
        ],
    ),
];

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn expand(world: &World, word: &str) -> String {
    word.replace("{home}", &world.path(&world.home))
        .replace("{data}", &world.path(&world.data))
        .replace("{base}", &world.path(&world.base))
}

fn run_ctl(world: &World, argv: &[String], stdin: Option<&str>) -> Run {
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
    Run {
        code: output.status.code().expect("exit code, not a signal"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn stem(case: &Case, step: &Step) -> String {
    if step.label.is_empty() {
        golden::file_stem(case.id)
    } else {
        format!("{}.{}", golden::file_stem(case.id), step.label)
    }
}

fn changed_files(
    world: &World,
    before: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
) -> Vec<String> {
    let after = world.snapshot();
    after
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
            normalize::world_roots(world)
                .iter()
                .fold(p, |acc, root| acc.replace(root, "<WORLD>"))
        })
        .collect()
}

fn run_case(case: &Case) {
    let world = World::new(&golden::file_stem(case.id));
    for step in case.steps {
        if let Some(prepare) = step.hook {
            prepare(&world);
            continue;
        }
        let argv: Vec<String> = step.argv.iter().map(|w| expand(&world, w)).collect();
        let before = world.snapshot();
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
        for canary in [FAKE_SECRET, SECRET_KEY] {
            assert!(
                !run.stdout.contains(canary) && !run.stderr.contains(canary),
                "{name}: leaked a secret value"
            );
        }
        if !step.writes {
            let changed = changed_files(&world, &before);
            assert!(
                changed.is_empty(),
                "{name}: a read or dry-run step changed the world: {changed:?}"
            );
        }
        let stem = stem(case, step);
        if step.golden && !BUMP_PENDING.contains(&stem.as_str()) {
            golden::assert_golden(
                &stem,
                &json!({
                    "argv": step.argv,
                    "exitCode": step.exit,
                    "envelope": normalize::normalize(&world, &envelope),
                }),
            );
        }
    }
}

#[test]
fn every_case_matches_its_golden_envelope() {
    let mut failures = Vec::new();
    for case in CASES {
        if let Err(payload) = std::panic::catch_unwind(|| run_case(case)) {
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
    let mut seen = BTreeSet::new();
    let mut owned: BTreeSet<String> = BTreeSet::new();
    for case in CASES {
        assert!(
            ids.contains(case.id),
            "case `{}` is not a registry command",
            case.id
        );
        assert!(seen.insert(case.id), "case `{}` appears twice", case.id);
        let labels: BTreeSet<&str> = case.steps.iter().map(|s| s.label).collect();
        assert_eq!(
            labels.len(),
            case.steps.len(),
            "{}: step labels repeat",
            case.id
        );
        for step in case.steps.iter().filter(|step| step.golden) {
            owned.insert(stem(case, step));
        }
        for step in case.steps.iter().filter(|step| step.hook.is_none()) {
            assert!(
                ids.iter().any(|id| {
                    let words: Vec<&str> = id.split(' ').collect();
                    step.argv.len() >= words.len() && step.argv[..words.len()] == words[..]
                }),
                "{} {}: argv starts with no registry command",
                case.id,
                step.label
            );
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
    let ids = command_ids();
    let with_case: BTreeSet<&str> = CASES.iter().map(|case| case.id).collect();
    let missing: Vec<&String> = ids
        .iter()
        .filter(|id| !with_case.contains(id.as_str()))
        .collect();
    eprintln!(
        "ctl contract: {} commands, {} with a golden case, {} uncovered",
        ids.len(),
        ids.len() - missing.len(),
        missing.len()
    );
    assert!(missing.is_empty(), "no golden envelope for: {missing:?}");
}

#[test]
fn every_command_is_either_read_only_tested_or_given_a_reason() {
    let table: BTreeSet<String> = COMMANDS.iter().map(|c| c.path.join(" ")).collect();
    assert_eq!(table.len(), COMMANDS.len(), "command paths are unique");
    let classified = rows();
    let leaves: BTreeSet<String> = command_ids().into_iter().collect();
    let in_rows: BTreeSet<String> = classified.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(
        leaves, in_rows,
        "every leaf command has one row and no row is stale"
    );
    let world = World::new("smoke");
    let before = world.snapshot();
    for row in &classified {
        match row.smoke {
            Smoke::Skip(reason) => {
                assert!(
                    !reason.trim().is_empty(),
                    "{}: not run by the smoke test and no reason given",
                    row.id
                );
                assert_ne!(
                    row.tier,
                    Tier::Read,
                    "{}: a read command is run, not skipped",
                    row.id
                );
            }
            Smoke::Read(extra) => {
                let mut argv: Vec<String> = row.id.split(' ').map(String::from).collect();
                argv.extend(extra.iter().map(|w| w.to_string()));
                let run = run_ctl(&world, &argv, None);
                assert!(
                    run.code == 0 || run.code == 1,
                    "{}: exit {} {:?}",
                    row.id,
                    run.code,
                    run.stderr
                );
                let envelope: Value = serde_json::from_str(run.stdout.trim())
                    .unwrap_or_else(|e| panic!("{}: not one envelope: {e}", row.id));
                assert_eq!(envelope["ok"], run.code == 0, "{}", row.id);
                assert_eq!(
                    changed_files(&world, &before),
                    Vec::<String>::new(),
                    "{}: the smoke run changed the world",
                    row.id
                );
            }
        }
    }
}

#[test]
fn help_lists_every_command_and_exits_zero_without_writing() {
    let world = World::new("help");
    let before = world.snapshot();
    for command in COMMANDS {
        for flag in ["--help", "-h"] {
            let mut argv: Vec<String> = command.path.iter().map(|w| w.to_string()).collect();
            argv.push(flag.into());
            let run = run_ctl_plain(&world, &argv);
            assert_eq!(run.code, 0, "{argv:?}: {:?}", run.stderr);
            assert!(run.stdout.contains("Usage: toolportctl"), "{argv:?}");
            assert!(run.stdout.contains(command.summary), "{argv:?}");
        }
    }
    assert_eq!(changed_files(&world, &before), Vec::<String>::new());
}

fn run_ctl_plain(world: &World, argv: &[String]) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .args(argv)
        .env_clear()
        .current_dir(&world.home)
        .stdin(Stdio::null());
    for (key, value) in world.env() {
        command.env(key, value);
    }
    let output = command.output().expect("run toolportctl");
    Run {
        code: output.status.code().expect("exit code"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn exit_codes_and_the_version_envelope_follow_the_documented_contract() {
    let world = World::new("exit-codes");
    let json = |argv: &[&str]| {
        let argv: Vec<String> = argv.iter().map(|w| w.to_string()).collect();
        let run = run_ctl(&world, &argv, None);
        let envelope: Value = serde_json::from_str(run.stdout.trim()).unwrap();
        (run, envelope)
    };
    let (run, version) = json(&["--version"]);
    assert_eq!(run.code, 0);
    assert_eq!(version["command"], "version");
    assert_eq!(version["data"]["name"], "toolportctl");
    for argv in [
        vec!["nope"],
        vec!["status", "extra"],
        vec!["--bogus"],
        vec!["mcp", "serve"],
    ] {
        let (run, envelope) = json(&argv);
        assert_eq!(run.code, 2, "{argv:?}: {envelope}");
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["error"]["code"], "usage");
    }
    let bare = run_ctl_plain(&world, &[]);
    assert_eq!(bare.code, 2);
    assert!(bare.stdout.contains("Usage: toolportctl"));
    let (run, unhealthy) = json(&["mcp", "doctor"]);
    assert_eq!(run.code, 1);
    assert_eq!(unhealthy["error"]["code"], "unhealthy");
    assert!(unhealthy["data"]["checks"].is_array());
}

#[test]
fn data_dir_flag_points_the_command_at_another_data_dir() {
    let world = World::new("data-dir");
    let elsewhere = world.base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .args(["--json", "--data-dir"])
        .arg(&elsewhere)
        .arg("status")
        .env_clear()
        .current_dir(&world.home);
    for (key, value) in world
        .env()
        .into_iter()
        .filter(|(k, _)| *k != "TOOLPORT_DATA_DIR")
    {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["data"]["registry"]["exists"], false);
    assert_eq!(
        envelope["data"]["dataDir"],
        elsewhere.to_string_lossy().as_ref()
    );
}

#[test]
fn a_secret_key_never_shows_in_any_golden() {
    for entry in std::fs::read_dir(golden::root()).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for secret in [FAKE_SECRET, SECRET_KEY] {
            assert!(
                !text.contains(secret),
                "golden {} contains a secret value",
                entry.path().display()
            );
        }
    }
}
