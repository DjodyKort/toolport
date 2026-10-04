//! Replays the recorded mcpm reference outputs (`tests/fixtures/agents/mcpm`, produced by
//! `generator/gen.sh` against the Python reference on the synthetic repositories in
//! `tests/fixtures/agents/repos`) through `toolportctl agents ...`. Text, exit code, the files
//! left behind and the lockfile are compared; every difference from mcpm is a named rule in
//! `expected`, an unlisted difference fails the test.

use super::run_with;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const FIXTURES: &str = "tests/fixtures/agents";
const EMPTY_LOCK: &str = r#"{"version": 1, "synced_at": "x", "scope": "", "output_root": "", "skills": {}, "rules": {}, "agents": {}, "styles": {}, "active_styles": {}}"#;

type Setup = fn(&Fx);

struct Case {
    name: &'static str,
    fixture: &'static str,
    setup: Setup,
}

const fn case(name: &'static str, fixture: &'static str, setup: Setup) -> Case {
    Case {
        name,
        fixture,
        setup,
    }
}

fn nothing(_: &Fx) {}

fn synced(fx: &Fx) {
    fx.cli_ok(&["sync", "--project"]);
}

fn synced_global(fx: &Fx) {
    fx.cli_ok(&["sync", "--global"]);
}

fn empty_lock(fx: &Fx) {
    fx.put("repo/mcpm-skills.lock", EMPTY_LOCK);
}

fn planner_deleted(fx: &Fx) {
    synced(fx);
    std::fs::remove_file(fx.root.join("repo/.claude/agents/planner.md")).unwrap();
}

fn edited(fx: &Fx) {
    synced(fx);
    let helper = fx.root.join("repo/agents/helper/AGENT.md");
    let mut text = std::fs::read_to_string(&helper).unwrap();
    text.push_str("changed\n");
    std::fs::write(helper, text).unwrap();
    fx.put(
        "repo/agents/newone/AGENT.md",
        "---\nname: newone\ndescription: A new agent for testing the diff output\n---\nbody\n",
    );
    std::fs::remove_dir_all(fx.root.join("repo/agents/reviewer")).unwrap();
}

fn source_removed(fx: &Fx) {
    synced(fx);
    std::fs::remove_dir_all(fx.root.join("repo/agents/reviewer")).unwrap();
}

fn cleaned(fx: &Fx) {
    synced(fx);
    fx.cli_ok(&["clean", "--project"]);
}

const CASES: &[Case] = &[
    case("ls", "basic", nothing),
    case("ls-wrap", "wrap", nothing),
    case("ls-empty", "empty", nothing),
    case("ls-norepo", "empty", nothing),
    case("lint-warn", "basic", nothing),
    case("lint-clean", "clean", nothing),
    case("lint-errors", "lint", nothing),
    case("lint-none", "empty", nothing),
    case("lint-norepo", "empty", nothing),
    case("audit-clean", "basic", nothing),
    case("audit-findings", "audit", nothing),
    case("audit-medium", "audit-medium", nothing),
    case("audit-none", "empty", nothing),
    case("audit-norepo", "empty", nothing),
    case("diff-nolock", "basic", nothing),
    case("diff-clean", "basic", synced),
    case("diff-changes", "basic", edited),
    case("diff-norepo", "empty", nothing),
    case("status-nolock", "basic", nothing),
    case("status-strict-nolock", "basic", nothing),
    case("status-ok", "basic", synced),
    case("status-drift", "basic", planner_deleted),
    case("status-strict-drift", "basic", planner_deleted),
    case("status-strict-ok", "basic", synced),
    case("status-noagents", "basic", empty_lock),
    case("sync-project", "basic", nothing),
    case("sync-dry-run", "basic", nothing),
    case("sync-client", "basic", nothing),
    case("sync-global", "basic", nothing),
    case("sync-global-dry-run", "basic", nothing),
    case("sync-none", "empty", nothing),
    case("sync-norepo", "empty", nothing),
    case("sync-stale", "basic", source_removed),
    case("sync-clean-repo", "clean", nothing),
    case("clean-nolock", "basic", nothing),
    case("clean-project", "basic", synced),
    case("clean-client", "basic", synced),
    case("clean-unknown-client", "basic", synced),
    case("clean-global", "basic", synced_global),
    case("clean-noagents", "basic", empty_lock),
    case("clean-twice", "basic", cleaned),
    case("uninstall", "basic", synced),
    case("uninstall-nolock", "basic", nothing),
    case("uninstall-missing", "basic", synced),
    case("add", "empty", nothing),
    case("add-exists", "basic", nothing),
    case("add-invalid", "basic", nothing),
    case("add-double-dash", "basic", nothing),
    case("add-fresh-path", "empty", nothing),
];

/// mcpm crashes on every call of these (an AttributeError for `audit`, a TypeError for
/// `uninstall`); the recorded output is the crash line and the goldens above come from a run
/// with the one-line fix in `generator/mcpm_agents.py`.
const CRASHES: &[(&str, &str, &str)] = &[
    ("audit-crash", "audit-clean", "AttributeError"),
    ("uninstall-crash", "uninstall", "TypeError"),
];

struct Fx {
    _base: DataDirFx,
    root: PathBuf,
    home: PathBuf,
}

impl Fx {
    fn new(case: &Case) -> Self {
        let base = DataDirFx::with_data_subdir("ctl-agents-golden", case.name, "data");
        let root = base.dir.canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        copy_dir(&fixture("repos").join(case.fixture), &root.join("repo"));
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        Self {
            _base: base,
            root,
            home,
        }
    }

    fn put(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn cli_ok(&self, list: &[&str]) {
        let repo = self.root.join("repo").to_string_lossy().into_owned();
        let mut args = vec![list[0], "--path", repo.as_str()];
        args.extend_from_slice(&list[1..]);
        let (code, text) = run(&args_with_group(&args));
        assert_eq!(code, 0, "setup {list:?}: {text}");
    }

    fn expand(&self, arg: &str) -> String {
        arg.replace("{repo}", &self.root.join("repo").to_string_lossy())
            .replace("{home}", &self.home.to_string_lossy())
            .replace("{root}", &self.root.to_string_lossy())
    }

    fn normalised(&self, text: &str) -> String {
        text.replace(self.root.to_string_lossy().as_ref(), "<root>")
    }

    fn files(&self) -> BTreeSet<String> {
        tree_snapshot(&self.root)
            .into_iter()
            .filter(|(_, content)| content.is_some())
            .map(|(path, _)| match path.strip_prefix("data/") {
                Some(rest) => format!("home/.config/mcpm/{rest}"),
                None => path,
            })
            .collect()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
    }
}

fn args_with_group(args: &[&str]) -> Vec<String> {
    let mut list = vec!["agents".to_string()];
    list.extend(args.iter().map(|s| s.to_string()));
    list
}

fn run(list: &[String]) -> (i32, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(list, &mut out, &mut err);
    let mut text = String::from_utf8(out).unwrap();
    text.push_str(&String::from_utf8(err).unwrap());
    (code, text)
}

pub(super) fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub(super) fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES).join(rel)
}

fn recorded(case: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(fixture("mcpm").join(case).join(file)).ok()
}

fn mcpm_args(case: &str) -> Vec<String> {
    recorded(case, "args.txt")
        .expect("args.txt")
        .lines()
        .map(String::from)
        .collect()
}

/// The command line `toolportctl` takes for the mcpm arguments: mcpm defaults to the project
/// scope, `toolportctl` to the user level (DEV-SKL7-1), so the project scope is explicit.
fn ours(fx: &Fx, mcpm: &[String]) -> Vec<String> {
    let mut list = vec!["agents".to_string()];
    list.extend(mcpm.iter().map(|a| fx.expand(a)));
    let scoped = matches!(mcpm[0].as_str(), "sync" | "clean" | "uninstall");
    if scoped && !mcpm.iter().any(|a| a == "--global") {
        list.push("--project".into());
    }
    list
}

enum Expected {
    Text(String, i32),
}

const NO_REPO: &str = "toolportctl: no skills repository found";

/// What `toolportctl` is expected to print for the mcpm output: mcpm's text with the hints
/// pointing at `toolportctl` (D-042) and every other intended difference named.
fn expected(name: &str, mcpm: &str, code: i32) -> Expected {
    let text = mcpm
        .trim_end()
        .replace("'mcpm agents ", "'toolportctl agents ");
    match name {
        "ls-norepo" | "lint-norepo" | "audit-norepo" | "diff-norepo" | "sync-norepo" => {
            Expected::Text(NO_REPO.into(), 1)
        }
        "add-exists" | "uninstall-missing" => Expected::Text(
            format!("toolportctl: {}", text.trim_start_matches("Error: ")),
            1,
        ),
        "add-invalid" | "add-double-dash" => {
            let name = text
                .split('\'')
                .nth(1)
                .expect("quoted agent name")
                .to_string();
            let reason = text
                .lines()
                .find_map(|l| l.trim().strip_prefix("Value error, "))
                .expect("validation reason")
                .trim_end();
            Expected::Text(
                format!("toolportctl: Invalid agent name '{name}': {reason}"),
                1,
            )
        }
        _ => Expected::Text(text, code),
    }
}

fn lock_json(path: &Path, fx: &Fx) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut value: Value = serde_json::from_str(&fx.normalised(&text)).unwrap();
    value["synced_at"] = Value::String("<ts>".into());
    Some(value)
}

fn recorded_lock(case: &str, file: &str) -> Option<Value> {
    recorded(case, file).map(|t| serde_json::from_str(&t).unwrap())
}

fn replay(case: &Case) -> Vec<String> {
    let mut failures = Vec::new();
    let name = case.name;
    let fx = Fx::new(case);
    (case.setup)(&fx);
    let mcpm = mcpm_args(name);
    let (code, text) = run(&ours(&fx, &mcpm));
    let text = fx.normalised(&text);
    let mcpm_text = recorded(name, "output.txt").expect("output.txt");
    let mcpm_code: i32 = recorded(name, "exit.txt").unwrap().trim().parse().unwrap();
    match expected(name, &mcpm_text, mcpm_code) {
        Expected::Text(want, want_code) => {
            if text.trim_end() != want.trim_end() {
                failures.push(format!(
                    "{name}: text\n--- expected\n{want}\n--- actual\n{}",
                    text.trim_end()
                ));
            }
            if code != want_code {
                failures.push(format!("{name}: exit {code}, expected {want_code}"));
            }
        }
    }
    let want_tree: BTreeSet<String> = recorded(name, "tree.txt")
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    if fx.files() != want_tree {
        failures.push(format!(
            "{name}: files\n  only in mcpm: {:?}\n  only here: {:?}",
            want_tree.difference(&fx.files()).collect::<Vec<_>>(),
            fx.files().difference(&want_tree).collect::<Vec<_>>()
        ));
    }
    for (file, path) in [
        ("lock-repo.json", fx.root.join("repo/mcpm-skills.lock")),
        ("lock-global.json", fx.root.join("data/mcpm-skills.lock")),
    ] {
        let (want, got) = (recorded_lock(name, file), lock_json(&path, &fx));
        if want != got {
            failures.push(format!("{name}: {file}\n  expected {want:?}\n  actual   {got:?}"));
        }
    }
    failures
}

#[test]
fn every_recorded_case_has_a_replay() {
    let mut recorded: Vec<String> = std::fs::read_dir(fixture("mcpm"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    recorded.sort();
    let mut replayed: Vec<String> = CASES
        .iter()
        .map(|c| c.name.to_string())
        .chain(CRASHES.iter().map(|(name, _, _)| name.to_string()))
        .collect();
    replayed.sort();
    assert_eq!(recorded, replayed);
}

#[test]
fn every_subcommand_matches_the_recorded_mcpm_output() {
    let failures: Vec<String> = CASES.iter().flat_map(replay).collect();
    assert!(
        failures.is_empty(),
        "{} difference(s) from mcpm:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn mcpm_crashes_on_audit_and_uninstall_and_the_port_does_not() {
    for (crash, golden, error) in CRASHES {
        let line = recorded(crash, "output.txt").unwrap();
        assert!(line.starts_with(error), "{crash}: {line}");
        assert_eq!(recorded(crash, "exit.txt").unwrap().trim(), "1");
        assert_eq!(mcpm_args(crash), mcpm_args(golden), "{crash}");
    }
}
