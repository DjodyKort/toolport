//! Replays the recorded mcpm `skills sync` outputs (`tests/fixtures/skills-collisions/mcpm`,
//! produced by `generator/gen.sh` against the Python reference on the synthetic repository and
//! overlays beside it) through `toolportctl skills sync`. Text, exit code and the files left behind
//! are compared; every difference from mcpm is named in `expected` or `squash`, an unlisted
//! difference fails the test.

use super::agents_golden_tests::copy_dir;
use super::run_with;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const FIXTURES: &str = "tests/fixtures/skills-collisions";

type Setup = fn(&Fx);

struct Case {
    name: &'static str,
    overlay: Option<&'static str>,
    setup: Setup,
}

const fn case(name: &'static str, overlay: Option<&'static str>, setup: Setup) -> Case {
    Case {
        name,
        overlay,
        setup,
    }
}

fn nothing(_: &Fx) {}

fn migrated(fx: &Fx) {
    fx.cli_ok(&["--project", "--client", "claude-code", "--migrate"]);
}

fn db_helper_removed(fx: &Fx) {
    fx.cli_ok(&["--project", "--client", "claude-code"]);
    std::fs::remove_dir_all(fx.root.join("repo/skills/db-helper")).unwrap();
}

const CASES: &[Case] = &[
    case("clean-project", None, nothing),
    case("clean-global", None, nothing),
    case("warn-project", Some("shadow"), nothing),
    case("warn-project-dry-run", Some("shadow"), nothing),
    case("no-migrate-project", Some("shadow"), nothing),
    case("migrate-project", Some("shadow"), nothing),
    case("migrate-project-dry-run", Some("shadow"), nothing),
    case("migrate-global", Some("shadow-home"), nothing),
    case("migrate-twice", Some("shadow"), migrated),
    case("warn-cursor-flat", Some("cursor-flat"), nothing),
    case("warnings-aider", None, nothing),
    case("stale-project", None, db_helper_removed),
    case("append-agents-md", None, nothing),
    case("append-zed", None, nothing),
];

struct Fx {
    _base: DataDirFx,
    root: PathBuf,
    home: PathBuf,
}

impl Fx {
    fn new(tag: &str, overlay: Option<&str>) -> Self {
        let base = DataDirFx::with_data_subdir("ctl-skills-collisions", tag, "data");
        let root = base.dir.canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        copy_dir(&fixture("repos/basic"), &root.join("repo"));
        if let Some(overlay) = overlay {
            copy_dir(&fixture("overlays").join(overlay), &root);
        }
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        Self {
            _base: base,
            root,
            home,
        }
    }

    fn at(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.root.clone(), |path, part| path.join(part))
    }

    fn repo(&self) -> String {
        self.at("repo").to_string_lossy().into_owned()
    }

    fn sync_args(&self, extra: &[&str]) -> Vec<String> {
        let mut list = vec!["skills".into(), "sync".into(), "--repo".into(), self.repo()];
        list.extend(extra.iter().map(|s| s.to_string()));
        list
    }

    fn cli_ok(&self, extra: &[&str]) {
        let (code, text) = run(&self.sync_args(extra));
        assert_eq!(code, 0, "setup {extra:?}: {text}");
    }

    fn expand(&self, arg: &str) -> String {
        arg.replace("{repo}", &self.repo())
            .replace("{home}", &self.home.to_string_lossy())
            .replace("{root}", &self.root.to_string_lossy())
    }

    fn normalised(&self, text: &str) -> String {
        let stamp = Regex::new(r"[0-9]{8}T[0-9]{6}Z").unwrap();
        stamp
            .replace_all(&text.replace(self.root.to_string_lossy().as_ref(), "<root>"), "<stamp>")
            .into_owned()
    }

    fn files(&self) -> BTreeSet<String> {
        tree_snapshot(&self.root)
            .into_iter()
            .filter(|(_, content)| content.is_some())
            .map(|(path, _)| match path.strip_prefix("data/") {
                Some(rest) => format!("home/.config/mcpm/{rest}"),
                None => path,
            })
            .map(|path| self.normalised(&path))
            .collect()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
    }
}

fn run(list: &[String]) -> (i32, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(list, &mut out, &mut err);
    let mut text = String::from_utf8(out).unwrap();
    text.push_str(&String::from_utf8(err).unwrap());
    (code, text)
}

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES).join(rel)
}

fn recorded(case: &str, file: &str) -> String {
    std::fs::read_to_string(fixture("mcpm").join(case).join(file)).unwrap()
}

/// The command line `toolportctl` takes for the mcpm arguments: `--path` is `--repo`, and mcpm
/// defaults to the project scope while `toolportctl` defaults to the user level (DEV-SKL7-1), so
/// the project scope is explicit.
fn ours(fx: &Fx, mcpm: &[String]) -> Vec<String> {
    let mut list = vec!["skills".to_string()];
    list.extend(mcpm.iter().map(|a| match a.as_str() {
        "--path" => "--repo".to_string(),
        other => fx.expand(other),
    }));
    if !mcpm.iter().any(|a| a == "--global") {
        list.push("--project".into());
    }
    list
}

/// mcpm's text with the hint naming `toolportctl` (D-042).
fn expected(mcpm: &str) -> String {
    mcpm.trim_end()
        .replace("Run mcpm skills resolve", "Run toolportctl skills resolve")
}

/// Rich pads each table to the widest cell, and a scratch root is not as long as the one of this
/// run; the padding is collapsed on both sides so the paths in a cell cannot move a column.
fn squash(text: &str) -> String {
    let pad = Regex::new(r" +").unwrap();
    let fill = Regex::new(r"━+|─+").unwrap();
    text.trim_end()
        .lines()
        .map(|line| {
            if line.starts_with(['┏', '┃', '┡', '│', '└']) {
                let line = fill.replace_all(line, |c: &regex::Captures| c[0].chars().next().unwrap().to_string());
                pad.replace_all(&line, " ").into_owned()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn replay(case: &Case) -> Vec<String> {
    let name = case.name;
    let fx = Fx::new(name, case.overlay);
    (case.setup)(&fx);
    let mcpm: Vec<String> = recorded(name, "args.txt").lines().map(String::from).collect();
    let (code, text) = run(&ours(&fx, &mcpm));
    let (text, want) = (squash(&fx.normalised(&text)), squash(&expected(&recorded(name, "output.txt"))));
    let mut failures = Vec::new();
    if text != want {
        failures.push(format!("{name}: text\n--- expected\n{want}\n--- actual\n{text}"));
    }
    let want_code: i32 = recorded(name, "exit.txt").trim().parse().unwrap();
    if code != want_code {
        failures.push(format!("{name}: exit {code}, expected {want_code}"));
    }
    let want_tree: BTreeSet<String> = recorded(name, "tree.txt").lines().map(String::from).collect();
    if fx.files() != want_tree {
        failures.push(format!(
            "{name}: files\n  only in mcpm: {:?}\n  only here: {:?}",
            want_tree.difference(&fx.files()).collect::<Vec<_>>(),
            fx.files().difference(&want_tree).collect::<Vec<_>>()
        ));
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
    let mut replayed: Vec<String> = CASES.iter().map(|c| c.name.to_string()).collect();
    replayed.sort();
    assert_eq!(recorded, replayed);
}

#[test]
fn every_sync_matches_the_recorded_mcpm_output() {
    let failures: Vec<String> = CASES.iter().flat_map(replay).collect();
    assert!(
        failures.is_empty(),
        "{} difference(s) from mcpm:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

fn json_run(fx: &Fx, extra: &[&str]) -> Value {
    let mut list = fx.sync_args(extra);
    list.insert(0, "--json".into());
    let (code, text) = run(&list);
    assert_eq!(code, 0, "{text}");
    serde_json::from_str(text.trim()).unwrap()
}

#[test]
fn json_keeps_its_keys_and_adds_the_collision_report() {
    let fx = Fx::new("json-warn", Some("shadow"));
    let v = json_run(&fx, &["--project", "--client", "claude-code"]);
    assert_eq!(v["command"], "skills sync");
    let data = &v["data"];
    for key in [
        "repo",
        "dryRun",
        "globalMode",
        "outputRoot",
        "syncedAt",
        "skillCount",
        "ruleCount",
        "cleaned",
    ] {
        assert!(!data[key].is_null(), "{key} is gone: {data}");
    }
    assert_eq!(data["skillCount"], 2);
    assert_eq!(data["ruleCount"], 1);
    assert_eq!(data["globalMode"], false);
    assert_eq!(data["clientCount"], 1);
    assert_eq!(data["replaced"], 0);
    assert_eq!(data["kept"], 2);
    assert_eq!(
        data["backupRoot"],
        json!(fx.at("repo/.mcpm-backups").to_string_lossy())
    );
    let names: Vec<&str> = data["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["code-review", "db-helper", "python-style"]);
    let review = &data["entries"][0];
    assert_eq!(review["type"], "skill");
    assert_eq!(review["clientsSynced"], json!(["claude-code"]));
    let shadow = fx.at("repo/.claude/commands/code-review.md");
    assert_eq!(
        review["warnings"],
        json!([format!(
            "claude-code: shadowed by existing file at {}",
            shadow.display()
        )])
    );
    assert_eq!(data["entries"][2]["type"], "rule");
    assert_eq!(data["entries"][2]["warnings"], json!([]));
    let first = &data["collisions"][0];
    assert_eq!(first["skill"], "code-review");
    assert_eq!(first["client"], "claude-code");
    assert_eq!(first["action"], "kept");
    assert_eq!(first["collisionPath"], json!(shadow.to_string_lossy()));
    assert_eq!(first["backupPath"], Value::Null);
    assert_eq!(data["collisions"].as_array().unwrap().len(), 2);
}

#[test]
fn json_without_a_collision_reports_empty_rows() {
    let fx = Fx::new("json-clean", None);
    let data = json_run(&fx, &["--project", "--client", "claude-code"])["data"].clone();
    assert_eq!(data["collisions"], json!([]));
    assert_eq!(data["replaced"], 0);
    assert_eq!(data["kept"], 0);
    assert_eq!(data["entries"].as_array().unwrap().len(), 3);
}

#[test]
fn the_ipc_handler_and_the_cli_serve_the_same_report() {
    let fx = Fx::new("json-ipc", Some("shadow"));
    let mut data = json_run(&fx, &["--project", "--client", "claude-code"])["data"].clone();
    let mut served = crate::plus::dispatch(
        "plus.skills.sync",
        json!({"repo_path": fx.repo(), "global_mode": false, "client_keys": ["claude-code"]}),
    )
    .unwrap();
    data["syncedAt"] = Value::Null;
    served["syncedAt"] = Value::Null;
    assert_eq!(served, data);
}

#[test]
fn migrate_backs_up_the_shadowing_file_and_lists_the_backup() {
    let fx = Fx::new("migrate-backup", Some("shadow"));
    let command = fx.at("repo/.claude/commands/code-review.md");
    let original = std::fs::read_to_string(&command).unwrap();
    let mut list = fx.sync_args(&["--project", "--client", "claude-code", "--migrate"]);
    list.insert(0, "--json".into());
    let (code, text) = run(&list);
    assert_eq!(code, 0, "{text}");
    let data: Value = serde_json::from_str(text.trim()).unwrap();
    let data = &data["data"];
    assert_eq!(data["replaced"], 2);
    assert_eq!(data["kept"], 0);
    assert!(!command.exists());
    let backup = PathBuf::from(data["collisions"][0]["backupPath"].as_str().unwrap());
    assert_eq!(data["collisions"][0]["action"], "replaced");
    assert!(backup.starts_with(fx.at("repo/.mcpm-backups/.claude/commands")));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
    let index: Value = serde_json::from_str(
        &std::fs::read_to_string(fx.at("repo/.mcpm-backups/INDEX.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(index["backups"].as_array().unwrap().len(), 2);
    assert_eq!(
        index["backups"][0]["original_path"],
        json!(command.to_string_lossy())
    );
    assert_eq!(
        index["backups"][0]["backup_path"],
        json!(backup.to_string_lossy())
    );

    fx.cli_ok(&["--project", "--client", "claude-code", "--migrate"]);
    let (_, text) = run(&fx.sync_args(&["--project", "--client", "claude-code"]));
    assert!(!text.contains("collision"), "{text}");
}

#[test]
fn the_text_lists_each_backup_and_the_summary() {
    let fx = Fx::new("migrate-text", Some("shadow"));
    let (code, text) = run(&fx.sync_args(&["--project", "--client", "claude-code", "--migrate"]));
    assert_eq!(code, 0, "{text}");
    let command = fx.at("repo/.claude/commands/code-review.md");
    let line = text
        .lines()
        .find(|l| l.starts_with("  Replaced "))
        .unwrap_or_else(|| panic!("{text}"));
    let (from, to) = line.trim_start_matches("  Replaced ").split_once(" → backup at ").unwrap();
    assert_eq!(from, command.to_string_lossy());
    assert!(Path::new(to).is_file(), "{line}");
    assert!(text.contains("Replaced 2 colliding file(s) (backed up)."), "{text}");
}

#[test]
fn migrate_and_no_migrate_are_mutually_exclusive() {
    let fx = Fx::new("exclusive", Some("shadow"));
    let before = tree_snapshot(&fx.root);
    let (code, text) = run(&fx.sync_args(&["--migrate", "--no-migrate"]));
    assert_eq!(code, 2, "{text}");
    assert!(
        text.contains("--migrate and --no-migrate are mutually exclusive"),
        "{text}"
    );
    assert_eq!(tree_snapshot(&fx.root), before);
}

#[test]
fn a_repository_without_skills_says_so() {
    let fx = Fx::new("no-skills", None);
    std::fs::remove_dir_all(fx.root.join("repo/skills")).unwrap();
    std::fs::remove_dir_all(fx.root.join("repo/rules")).unwrap();
    std::fs::create_dir_all(fx.root.join("repo/skills")).unwrap();
    let (code, text) = run(&fx.sync_args(&["--project", "--client", "claude-code"]));
    assert_eq!(code, 0, "{text}");
    assert_eq!(text.trim_end(), "No skills found in repository.");
}
