//! `toolportctl attention ls` over fixture worlds (MIG-GUI-11), through the real binary: one row
//! per feed kind at its level, a listing that never touches the network or starts an engine, and
//! no secret value in any row. The worlds are the shared fixtures of the other CLI suites; every
//! `claude`, engine and download tool on PATH is a stub that leaves a mark when it runs.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

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
#[path = "common/plugins_world.rs"]
mod plugins_world;
#[path = "common/sources_world.rs"]
mod sources_world;

use ctl_world::{write_json, CtlWorld, FAKE_SECRET};

const CANARY: &str = "CANARY-attention-4b1e";
const MARKED: [&str; 7] = ["claude", "headroom", "curl", "wget", "npx", "uvx", "node"];

fn world(tag: &str) -> CtlWorld {
    CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Servers, tasks, library, plugins and hooks, a stored login failure and stale compression
/// presets in one home: every feed that composes with the others has something to say.
fn full_world(tag: &str) -> (CtlWorld, plugins_world::PluginsWorld) {
    let world = world(tag);
    ctl_fixtures::tasks_home(&world);
    library_world::library_home(&world);
    library_world::behind_two(&world);
    let plugins = plugins_world::build_in(&world.base, &world.claude);
    write_json(
        &world.data.join("auth/status.json"),
        &json!({"version": 1, "servers": {"alpha": {
            "tracked": {"state": "needs_reauth", "reason": format!("http 401 {CANARY}"),
                        "since": now() - 3600, "transient": null},
            "lastProbeAt": now() - 60, "nextDueAt": now() + 600}}}),
    );
    write_json(
        &world.data.join("compression.json"),
        &json!({
            "provider": "headroom",
            "provider_version": {"pin": "0.9.0"},
            "presets": {"balanced": {"savings_profile": "balanced",
                                     "snapshot_version": "0.1.0", "port": 8787}}
        }),
    );
    (world, plugins)
}

struct Marks {
    dir: PathBuf,
    log: PathBuf,
}

/// Stubs first on PATH for every program a feed could start besides local `git`; `git` itself
/// is wrapped so a network subcommand fails and is logged.
fn marks(world: &CtlWorld) -> Marks {
    let dir = world.base.join("marked-bin");
    let log = world.base.join("marks.log");
    std::fs::create_dir_all(&dir).unwrap();
    for name in MARKED {
        exec::write_executable(
            &dir.join(name),
            &format!("#!/bin/sh\necho \"{name} $*\" >> '{}'\nexit 1\n", log.display()),
        );
    }
    exec::write_executable(
        &dir.join("git"),
        &format!(
            "#!/bin/sh\nfor a in \"$@\"; do\n  case \"$a\" in\n    fetch|ls-remote|pull|push|clone) echo \"git $*\" >> '{}'; echo 'fatal: network unreachable' >&2; exit 128 ;;\n  esac\ndone\nexec /usr/bin/git \"$@\"\n",
            log.display()
        ),
    );
    exec::write_executable(&world.claude, &format!("#!/bin/sh\necho \"claude $*\" >> '{}'\nexit 1\n", log.display()));
    Marks { dir, log }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(world: &CtlWorld, path_prefix: Option<&Path>, args: &[&str]) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .args(args)
        .env_clear()
        .current_dir(&world.home)
        .stdin(Stdio::null());
    for (key, value) in world.env() {
        let value = match (key, path_prefix) {
            ("PATH", Some(dir)) => format!("{}:{value}", dir.display()),
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

fn listed(world: &CtlWorld, path_prefix: Option<&Path>, extra: &[&str]) -> Value {
    let mut argv = vec!["--json", "attention", "ls"];
    argv.extend_from_slice(extra);
    let out = run(world, path_prefix, &argv);
    assert_eq!(out.code, 0, "{} / {}", out.stdout, out.stderr);
    let envelope: Value = serde_json::from_str(out.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(envelope["ok"], true, "{envelope}");
    envelope["data"].clone()
}

fn ids(data: &Value) -> Vec<String> {
    data["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect()
}

fn row<'a>(data: &'a Value, id: &str) -> &'a Value {
    data["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .unwrap_or_else(|| panic!("no row {id} in {:?}", ids(data)))
}

fn expect_row(data: &Value, id: &str, level: &str, from: &str, route: &str) {
    let item = row(data, id);
    assert_eq!(item["level"], level, "{id}: {item}");
    assert_eq!(item["from"], from, "{id}: {item}");
    assert_eq!(item["target"]["route"], route, "{id}: {item}");
    assert!(!item["title"].as_str().unwrap().is_empty(), "{id}");
    assert!(!item["detail"].as_str().unwrap().is_empty(), "{id}");
}

#[test]
fn one_world_has_one_row_per_feed_kind_at_its_level() {
    let (world, _) = full_world("attention-kinds");
    let data = listed(&world, None, &[]);

    expect_row(&data, "auth:alpha", "needs-you", "auth", "servers");
    expect_row(&data, "secrets:srv-alpha:missing", "needs-you", "doctor", "servers");
    expect_row(&data, "tasks:portal-token:waiting", "needs-you", "tasks", "tasks");
    expect_row(&data, "plugins:ecc@ecc:mcp", "look", "sources", "library");
    expect_row(&data, "compression:drift", "look", "compression", "tokens");
    expect_row(&data, "library:behind", "fyi", "sources", "library");
    expect_row(&data, "hooks:bash", "fyi", "context", "context");

    let counted = |level: &str| {
        data["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["level"] == level)
            .count()
    };
    assert_eq!(data["counts"]["needsYou"], counted("needs-you"));
    assert_eq!(data["counts"]["look"], counted("look"));
    assert_eq!(data["counts"]["fyi"], counted("fyi"));

    let order: Vec<&str> = data["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["level"].as_str().unwrap())
        .collect();
    let mut sorted = order.clone();
    sorted.sort_by_key(|level| ["needs-you", "look", "fyi"].iter().position(|l| l == level));
    assert_eq!(order, sorted, "needs-you first, then look, then fyi");

    let filtered = listed(&world, None, &["--level", "fyi"]);
    assert!(filtered["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["level"] == "fyi"));
    assert_eq!(filtered["counts"], data["counts"], "the filter leaves the counts alone");
}

#[test]
fn the_sources_feed_names_a_source_behind_and_a_second_library_clone() {
    let world = world("attention-sources");
    sources_world::build_in(&world.base);
    let data = listed(&world, None, &[]);
    let all = ids(&data);
    assert!(
        all.iter().any(|id| id.starts_with("source:") && id.ends_with(":behind")),
        "a source behind its remote: {all:?}"
    );
    expect_row(&data, "source:library:duplicate", "look", "sources", "library");
    let behind = all.iter().find(|id| id.ends_with(":behind")).unwrap();
    assert_eq!(row(&data, behind)["level"], "look");
    if let Some(item) = data["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "skills:invisible")
    {
        assert_eq!(item["level"], "look");
    }
}

#[test]
fn a_bundle_bound_to_a_folder_without_it_is_a_look_row_with_its_apply_command() {
    let world = world("attention-bundle");
    ctl_fixtures::bundle_home(&world);
    std::fs::create_dir_all(world.home.join("work/acme-erp/clients/acme-one/.git")).unwrap();
    let data = listed(&world, None, &[]);
    let found: Vec<&Value> = data["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["id"].as_str().unwrap().starts_with("bundle:acme-dev:"))
        .collect();
    assert!(!found.is_empty(), "{:?}", ids(&data));
    for item in found {
        assert_eq!(item["level"], "look");
        assert_eq!(item["from"], "context");
        assert_eq!(item["target"]["route"], "context");
        let command: Vec<&str> = item["action"]["command"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap())
            .collect();
        assert_eq!(&command[..4], ["toolportctl", "context", "bundle", "apply"]);
    }
}

#[test]
fn listing_touches_no_network_and_starts_no_engine() {
    let (world, _) = full_world("attention-offline");
    let marked = marks(&world);

    let data = listed(&world, Some(&marked.dir), &[]);
    assert!(
        ids(&data).len() >= 7,
        "the listing must have run its feeds, or the check proves nothing: {:?}",
        ids(&data)
    );
    let calls = std::fs::read_to_string(&marked.log).unwrap_or_default();
    assert_eq!(calls, "", "a feed started a program or a network transport");

    let fetch = run(&world, Some(&marked.dir), &["--json", "library", "status", "--fetch"]);
    assert_eq!(fetch.code, 0, "{}", fetch.stdout);
    assert!(
        std::fs::read_to_string(&marked.log).unwrap_or_default().contains("ls-remote"),
        "positive control: the wrapper records a network call when one is made"
    );
}

#[test]
fn no_row_carries_a_secret_value_or_a_credential() {
    let (world, _) = full_world("attention-leak");
    let lib = library_world::library(&world);
    let url = format!("https://library-user:{CANARY}@library.example.invalid/org/skills.git");
    ctl_fixtures::git(&lib, &world.home, &["remote", "set-url", "origin", &url]);
    let marked = marks(&world);

    for argv in [
        vec!["--json", "attention", "ls"],
        vec!["attention", "ls"],
        vec!["--json", "attention", "ls", "--level", "needs-you"],
    ] {
        let out = run(&world, Some(&marked.dir), &argv);
        assert_eq!(out.code, 0, "{argv:?}: {} / {}", out.stdout, out.stderr);
        for text in [&out.stdout, &out.stderr] {
            for secret in [CANARY, FAKE_SECRET, ctl_world::SECRET_KEY] {
                assert!(!text.contains(secret), "{argv:?} leaked {secret}");
            }
            assert!(!text.contains("library-user"), "{argv:?} printed the remote user");
        }
    }
}

#[test]
fn a_dismissal_hides_a_row_and_a_passed_date_brings_it_back() {
    let (world, _) = full_world("attention-dismiss");
    let id = "tasks:portal-token:waiting";
    assert!(ids(&listed(&world, None, &[])).contains(&id.to_string()));

    let hide = run(&world, None, &["--json", "attention", "dismiss", id, "--until", "2999-01-01"]);
    assert_eq!(hide.code, 0, "{}", hide.stdout);
    let hidden = listed(&world, None, &[]);
    assert!(!ids(&hidden).contains(&id.to_string()));
    assert_eq!(hidden["counts"]["needsYou"], 2, "the auth and secrets rows stay");

    let file = world.data.join("plus/attention.json");
    let stored: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    stored["dismissed"][id]["until"].as_str().unwrap();

    let expired = json!({"dismissed": {id: {"until": "2000-01-01", "at": "1999-12-31T00:00:00Z"}}});
    std::fs::write(&file, expired.to_string()).unwrap();
    assert!(
        ids(&listed(&world, None, &[])).contains(&id.to_string()),
        "a dismissal whose date has passed no longer hides the row"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        expired.to_string(),
        "reading never rewrites the file"
    );
}
