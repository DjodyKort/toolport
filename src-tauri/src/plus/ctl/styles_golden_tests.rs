//! Replays the recorded mcpm reference outputs (`tests/fixtures/styles/mcpm`, produced by
//! `generator/gen.sh` against the Python reference on the synthetic repositories in
//! `tests/fixtures/styles/repos`) through `toolportctl styles ...`. Text, exit code, the files
//! left behind, the content of every new or changed file and the lockfile are compared; every
//! difference from mcpm is a named rule in `expected`, an unlisted difference fails the test.

use super::agents_golden_tests::copy_dir as copy_tree;
use super::run_with;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const FIXTURES: &str = "tests/fixtures/styles";
const EMPTY_LOCK: &str = r#"{"version": 1, "synced_at": "x", "scope": "", "output_root": "", "skills": {}, "rules": {}, "agents": {}, "styles": {}, "active_styles": {}}"#;
const FOREIGN_MODES: &str = r#"{"customModes": [{"slug": "reviewer", "name": "Reviewer", "roleDefinition": "Reviews", "groups": ["read"]}, {"slug": "style-old", "name": "Old", "roleDefinition": "Old style", "customInstructions": "x", "groups": ["read"]}]}"#;
const NEW_STYLE: &str =
    "---\nname: newone\ndescription: A new style for testing the diff output\n---\nbody\n";

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

fn applied(fx: &Fx) {
    fx.cli_ok(&["apply", "concise", "--project"]);
}

fn synced_then_teacher(fx: &Fx) {
    synced(fx);
    fx.cli_ok(&["apply", "teacher", "--project"]);
}

fn synced_then_teacher_on_cursor(fx: &Fx) {
    synced(fx);
    fx.cli_ok(&["apply", "teacher", "--client", "cursor", "--project"]);
}

fn changed(fx: &Fx) {
    synced(fx);
    let concise = fx.root.join("repo/styles/concise/STYLE.md");
    let mut text = std::fs::read_to_string(&concise).unwrap();
    text.push_str("changed\n");
    std::fs::write(concise, text).unwrap();
    fx.put("repo/styles/newone/STYLE.md", NEW_STYLE);
    std::fs::remove_dir_all(fx.root.join("repo/styles/terse")).unwrap();
}

fn foreign_modes(fx: &Fx) {
    fx.put("repo/.roomodes", FOREIGN_MODES);
}

fn foreign_modes_synced(fx: &Fx) {
    foreign_modes(fx);
    synced(fx);
}

fn source_removed(fx: &Fx) {
    synced(fx);
    std::fs::remove_dir_all(fx.root.join("repo/styles/terse")).unwrap();
}

fn kind_rules(fx: &Fx) {
    fx.put("repo/.rules", "Be kind.\n");
}

fn concise_on_zed(fx: &Fx) {
    fx.cli_ok(&["apply", "concise", "--client", "zed", "--project"]);
}

fn kind_rules_on_zed(fx: &Fx) {
    kind_rules(fx);
    concise_on_zed(fx);
}

fn concise_on_aider(fx: &Fx) {
    fx.cli_ok(&["apply", "concise", "--client", "aider", "--project"]);
}

fn removed(fx: &Fx) {
    applied(fx);
    fx.cli_ok(&["remove", "--project"]);
}

fn empty_lock(fx: &Fx) {
    fx.put("repo/mcpm-skills.lock", EMPTY_LOCK);
}

fn zed_without_lock(fx: &Fx) {
    concise_on_zed(fx);
    std::fs::remove_file(fx.root.join("repo/mcpm-skills.lock")).unwrap();
}

fn synced_and_applied(fx: &Fx) {
    synced(fx);
    applied(fx);
}

fn cleaned(fx: &Fx) {
    synced(fx);
    fx.cli_ok(&["clean", "--project"]);
}

fn empty_modes(fx: &Fx) {
    synced(fx);
    fx.put("repo/.roomodes", r#"{"customModes": []}"#);
}

fn fresh_style(fx: &Fx) {
    fx.cli_ok(&["add", "fresh"]);
}

const CASES: &[Case] = &[
    case("ls", "basic", nothing),
    case("ls-empty", "empty", nothing),
    case("ls-norepo", "empty", nothing),
    case("ls-synced", "basic", synced),
    case("ls-applied", "basic", synced_then_teacher),
    case("ls-partial", "partial", nothing),
    case("ls-subdir", "basic", nothing),
    case("lint-warn", "lint", nothing),
    case("lint-clean", "basic", nothing),
    case("lint-none", "empty", nothing),
    case("lint-norepo", "empty", nothing),
    case("lint-partial", "partial", nothing),
    case("diff-nolock", "basic", nothing),
    case("diff-clean", "basic", synced),
    case("diff-changes", "basic", changed),
    case("diff-applied", "basic", applied),
    case("diff-nolock-none", "empty", nothing),
    case("diff-norepo", "empty", nothing),
    case("status-nolock", "basic", nothing),
    case("status-synced", "basic", synced),
    case("status-applied", "basic", applied),
    case("status-both", "basic", synced_then_teacher_on_cursor),
    case("status-norepo", "empty", nothing),
    case("sync-project", "basic", nothing),
    case("sync-dry-run", "basic", nothing),
    case("sync-client", "basic", nothing),
    case("sync-client-roo", "basic", nothing),
    case("sync-client-tier2", "basic", nothing),
    case("sync-none", "empty", nothing),
    case("sync-norepo", "empty", nothing),
    case("sync-twice", "basic", synced),
    case("sync-roomodes-merge", "basic", foreign_modes),
    case("sync-after-apply", "basic", applied),
    case("sync-stale", "basic", source_removed),
    case("sync-partial", "partial", nothing),
    case("sync-dry-run-roomodes", "basic", foreign_modes),
    case("apply", "basic", nothing),
    case("apply-dry-run", "basic", nothing),
    case("apply-client", "basic", nothing),
    case("apply-tier1-client", "basic", nothing),
    case("apply-unknown-client", "basic", nothing),
    case("apply-unknown-style", "basic", nothing),
    case("apply-nostyles", "empty", nothing),
    case("apply-replace", "basic", applied),
    case("apply-replace-client", "basic", applied),
    case("apply-same-twice", "basic", applied),
    case("apply-dry-run-replace", "basic", applied),
    case("apply-zed-existing", "basic", kind_rules),
    case("apply-zed-twice", "basic", concise_on_zed),
    case("apply-huge", "lint", nothing),
    case("apply-after-sync", "basic", synced),
    case("apply-norepo", "empty", nothing),
    case("remove", "basic", applied),
    case("remove-dry-run", "basic", applied),
    case("remove-client", "basic", applied),
    case("remove-noactive", "basic", synced),
    case("remove-nolock", "basic", nothing),
    case("remove-client-none", "basic", concise_on_aider),
    case("remove-unknown-client", "basic", applied),
    case("remove-zed-kept", "basic", kind_rules_on_zed),
    case("remove-zed-only", "basic", concise_on_zed),
    case("remove-twice", "basic", removed),
    case("remove-norepo", "empty", nothing),
    case("clean-both", "basic", synced_and_applied),
    case("clean-synced", "basic", synced),
    case("clean-applied", "basic", applied),
    case("clean-nolock", "basic", nothing),
    case("clean-empty-lock", "basic", empty_lock),
    case("clean-twice", "basic", cleaned),
    case("clean-zed-nolock", "basic", zed_without_lock),
    case("clean-roomodes-foreign", "basic", foreign_modes_synced),
    case("clean-roomodes-only", "basic", empty_modes),
    case("clean-norepo", "empty", nothing),
    case("add", "empty", nothing),
    case("add-exists", "basic", nothing),
    case("add-invalid", "basic", nothing),
    case("add-double-dash", "basic", nothing),
    case("add-subdir", "basic", nothing),
    case("add-norepo", "empty", nothing),
    case("add-then-ls", "basic", fresh_style),
];

struct Fx {
    _base: DataDirFx,
    root: PathBuf,
    home: PathBuf,
    fixture: &'static str,
}

impl Fx {
    fn new(case: &Case) -> Self {
        let base = DataDirFx::with_data_subdir("ctl-styles-golden", case.name, "data");
        let root = base.dir.canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        copy_tree(&fixture("repos").join(case.fixture), &root.join("repo"));
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        Self {
            _base: base,
            root,
            home,
            fixture: case.fixture,
        }
    }

    fn put(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn cli_ok(&self, list: &[&str]) {
        let repo = self.root.join("repo").to_string_lossy().into_owned();
        let mut args = vec!["styles".to_string(), list[0].to_string(), "--path".into(), repo];
        args.extend(list[1..].iter().map(|s| s.to_string()));
        let (code, text) = run(&args);
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

    fn mapped(path: &str) -> String {
        match path.strip_prefix("data/") {
            Some(rest) => format!("home/.config/mcpm/{rest}"),
            None => path.to_string(),
        }
    }

    fn files(&self) -> BTreeSet<String> {
        tree_snapshot(&self.root)
            .into_iter()
            .filter(|(_, content)| content.is_some())
            .map(|(path, _)| Self::mapped(&path))
            .collect()
    }

    /// The content of every file that is new or changed compared with the repository fixture,
    /// in the layout `gen.sh` records.
    fn delta(&self) -> String {
        let pristine = fixture("repos").join(self.fixture);
        let mut out = String::new();
        for (path, content) in tree_snapshot(&self.root) {
            let Some(content) = content else { continue };
            let path = Self::mapped(&path);
            if path.ends_with("mcpm-skills.lock") {
                continue;
            }
            let unchanged = path
                .strip_prefix("repo/")
                .and_then(|rel| std::fs::read(pristine.join(rel)).ok())
                .is_some_and(|original| original == content);
            if unchanged {
                continue;
            }
            out.push_str(&format!("=== {path} ===\n"));
            out.push_str(&self.normalised(&String::from_utf8_lossy(&content)));
            out.push('\n');
        }
        out
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

/// The command line `toolportctl` takes for the mcpm arguments: mcpm always works in the
/// repository, `toolportctl` defaults to the user level (DEV-SKL9-1), so the project scope is
/// explicit.
fn ours(fx: &Fx, mcpm: &[String]) -> Vec<String> {
    let mut list = vec!["styles".to_string()];
    list.extend(mcpm.iter().map(|a| fx.expand(a)));
    if matches!(mcpm[0].as_str(), "sync" | "apply" | "remove" | "clean") {
        list.push("--project".into());
    }
    list
}

const NO_REPO: &str = "toolportctl: no skills repository found";

/// What `toolportctl` is expected to print for the mcpm output: mcpm's text with the hints
/// pointing at `toolportctl` (D-042) and every other intended difference named.
fn expected(name: &str, mcpm: &str, code: i32) -> (String, i32) {
    let text = mcpm
        .trim_end()
        .replace("'mcpm styles ", "'toolportctl styles ");
    let dry_run = mcpm_args(name).iter().any(|a| a == "--dry-run");
    match name {
        n if n.ends_with("-norepo") => (NO_REPO.into(), 1),
        "add-exists" | "apply-unknown-style" | "apply-nostyles" => {
            (format!("toolportctl: {text}"), 1)
        }
        "add-invalid" | "add-double-dash" => {
            let styled = mcpm_args(name)[1].clone();
            let reason = text
                .lines()
                .find_map(|l| l.trim().strip_prefix("Value error, "))
                .expect("validation reason")
                .trim_end();
            (
                format!("toolportctl: Invalid style name '{styled}': {reason}"),
                1,
            )
        }
        "remove" | "remove-dry-run" => {
            let targets = text.lines().filter(|l| l.contains("Removing style '")).count();
            let verb = if dry_run { "Would remove" } else { "Removed" };
            let head = text
                .rsplit_once("\n\nDone.")
                .expect("done line")
                .0
                .to_string();
            (
                format!("{head}\n\nDone. {verb} active style from {targets} client(s)."),
                code,
            )
        }
        _ => (text, code),
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
    let (want, want_code) = expected(name, &mcpm_text, mcpm_code);
    if text.trim_end() != want.trim_end() {
        failures.push(format!(
            "{name}: text\n--- expected\n{want}\n--- actual\n{}",
            text.trim_end()
        ));
    }
    if code != want_code {
        failures.push(format!("{name}: exit {code}, expected {want_code}"));
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
    let want_delta = recorded(name, "files.txt").unwrap_or_default();
    if fx.delta() != want_delta {
        failures.push(format!(
            "{name}: file content\n--- expected\n{want_delta}\n--- actual\n{}",
            fx.delta()
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
    let mut replayed: Vec<String> = CASES.iter().map(|c| c.name.to_string()).collect();
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
fn mcpm_reports_zero_clients_for_a_remove_without_client_and_the_port_counts_them() {
    for name in ["remove", "remove-dry-run"] {
        let text = recorded(name, "output.txt").unwrap();
        assert!(
            text.trim_end().ends_with("from 0 client(s)."),
            "{name}: {text}"
        );
        assert!(text.contains("Removing style 'concise' from zed"), "{name}");
    }
    let one = recorded("remove-client", "output.txt").unwrap();
    assert!(one.trim_end().ends_with("from 1 client(s)."), "{one}");
}
