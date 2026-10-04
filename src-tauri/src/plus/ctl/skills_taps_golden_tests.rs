//! Replays the recorded mcpm reference outputs (`tests/fixtures/skills-taps/mcpm`, produced by
//! `generator/gen.sh` against `mcpm sync tap|search|install` on local bare repositories built from
//! `tests/fixtures/skills-taps/repos`) through `toolportctl skills tap|search|install`. Text, exit
//! code, the files left behind and the tap index are compared; every difference from mcpm is a
//! named rule in `expected`, an unlisted difference fails the test. The GitHub URLs a `user/repo`
//! tap expands to are pointed at the local remotes, so nothing reaches a network.
//!
//! Deviations from mcpm replayed here (the others are pinned where they are named):
//! - DEV-SKL10-1: errors are `toolportctl: <message>` on stderr with exit 1, mcpm prints them on
//!   stdout and exits 0 (tap-add-duplicate, tap-add-missing-remote, tap-remove-missing,
//!   install-missing-skill, install-bad-spec, install-empty-tap).
//! - DEV-SKL10-2: `tap update <unknown>` is "not found", mcpm updates every tap (tap-update-unknown).
//! - DEV-SKL10-3: a failed pull exits 1 and names git's reason (tap-update-failed).
//! - DEV-SKL10-4: a blocked audit exits 1 (install-audit-blocked).
//! - DEV-SKL10-5: `@version` is ignored with a visible note (install-version).
//! - DEV-SKL10-6: an install that registers the tap says so (install-autotap and the audit cases).
//! - DEV-SKL10-12: the index is `taps.json` with `{repo, url}` beside the registry, the clones in
//!   `taps/` beside it; the files are mapped to mcpm's names and the index is compared by
//!   (name, repo, url).
//! - DEV-SKL10-13: a SKILL.md that fails to parse is one `Failed to parse <path>: <reason>` line,
//!   not mcpm's pydantic report (every case that reads the `escape` skill).
//! Not replayed here: DEV-SKL10-7 (`--dry-run`, `skills_taps_tests::dry_runs_leave_the_tree_unchanged`),
//! DEV-SKL10-8 (tap names, `a_malicious_tap_name_is_rejected_before_anything_is_written`),
//! DEV-SKL10-9 (install specs and skill names, `a_skill_named_with_a_traversal_is_never_installed`),
//! DEV-SKL10-10 (no symlinks or `.git` copied, `tap_ops_tests::install_keeps_file_modes_and_leaves_symlinks_and_git_data_behind`),
//! DEV-SKL10-11 (sources, `a_hostile_tap_source_is_rejected`).

use super::run_with;
use crate::plus::skills::tap_fixtures::{fixture_root, Remotes};
use crate::plus::skills::tap_handlers::TEST_GIT;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

const REMOTES: [&str; 5] = [
    "acme/skills",
    "acme/audit",
    "acme/extras",
    "acme/empty",
    "acme/mild",
];
const NEW_ONE: &str = "---\nname: new-one\ndescription: Newly added\n---\nbody\n";

type Setup = fn(&Fx);

struct Case {
    name: &'static str,
    setup: Setup,
}

const fn case(name: &'static str, setup: Setup) -> Case {
    Case { name, setup }
}

fn nothing(_: &Fx) {}

fn added(fx: &Fx) {
    fx.cli_ok(&["tap", "add", "acme/skills"]);
}

fn both(fx: &Fx) {
    added(fx);
    fx.cli_ok(&["tap", "add", "acme/extras", "--name", "extra"]);
}

fn upstream(fx: &Fx) {
    added(fx);
    fx.remotes
        .commit_file("acme/skills", "skills/new-one/SKILL.md", NEW_ONE);
}

fn updated(fx: &Fx) {
    upstream(fx);
    fx.cli_ok(&["tap", "update"]);
}

fn remote_removed(fx: &Fx) {
    added(fx);
    std::fs::remove_dir_all(fx.remotes.remote("acme/skills")).unwrap();
}

fn installed(fx: &Fx) {
    added(fx);
    let target = fx.root.join("target").to_string_lossy().into_owned();
    fx.cli_ok(&["install", "@acme/skills", "--path", &target]);
}

const CASES: &[Case] = &[
    case("tap-ls-empty", nothing),
    case("tap-add", nothing),
    case("tap-add-name", nothing),
    case("tap-add-duplicate", added),
    case("tap-add-missing-remote", nothing),
    case("tap-ls", both),
    case("tap-remove", added),
    case("tap-remove-missing", nothing),
    case("tap-update-none", nothing),
    case("tap-update", both),
    case("tap-update-one", both),
    case("tap-update-pulls", upstream),
    case("tap-update-unknown", both),
    case("tap-update-failed", remote_removed),
    case("search", both),
    case("search-tag", added),
    case("search-case", added),
    case("search-none", added),
    case("search-no-taps", nothing),
    case("search-after-update", updated),
    case("install-all", added),
    case("install-one", added),
    case("install-rule", added),
    case("install-autotap", nothing),
    case("install-twice", installed),
    case("install-missing-skill", added),
    case("install-bad-spec", nothing),
    case("install-version", added),
    case("install-audit-blocked", nothing),
    case("install-no-audit", nothing),
    case("install-audit-medium", nothing),
    case("install-empty-tap", nothing),
    case("install-after-update", updated),
];

struct Fx {
    _base: DataDirFx,
    root: PathBuf,
    remotes: Remotes,
}

impl Fx {
    fn new(case: &Case) -> Self {
        let base = DataDirFx::with_data_subdir("ctl-taps-golden", case.name, "data");
        let root = base.dir.canonicalize().unwrap();
        let remotes = Remotes::new(&root.join("_infra"));
        for repo in REMOTES {
            remotes.publish(repo);
        }
        TEST_GIT.with(|g| *g.borrow_mut() = Some(Rc::new(remotes.rewrite())));
        Self {
            _base: base,
            root,
            remotes,
        }
    }

    fn cli_ok(&self, list: &[&str]) {
        let (code, text) = run(&args_with_group(
            &list.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        ));
        assert_eq!(code, 0, "setup {list:?}: {text}");
    }

    fn expand(&self, arg: &str) -> String {
        arg.replace("{root}", &self.root.to_string_lossy())
            .replace("{home}", &self.root.join("home").to_string_lossy())
            .replace("{target}", &self.root.join("target").to_string_lossy())
    }

    fn normalised(&self, text: &str) -> String {
        text.replace(self.root.to_string_lossy().as_ref(), "<root>")
    }

    /// The files left behind under mcpm's names: its tap index and taps directory live under
    /// `home/.config/mcpm`, ours beside the registry in `data`; git internals, the lock file and
    /// the fake remotes are not part of the comparison.
    fn files(&self) -> BTreeSet<String> {
        tree_snapshot(&self.root)
            .into_iter()
            .filter(|(path, content)| {
                content.is_some()
                    && !path.starts_with("_infra/")
                    && !path.contains("/.git/")
                    && path != "data/taps.json.lock"
            })
            .map(|(path, _)| match path.as_str() {
                "data/taps.json" => "home/.config/mcpm/taps_index.json".to_string(),
                other => match other.strip_prefix("data/") {
                    Some(rest) => format!("home/.config/mcpm/{rest}"),
                    None => path,
                },
            })
            .collect()
    }

    fn index(&self) -> Option<BTreeMap<String, (String, String)>> {
        let text = std::fs::read_to_string(self.root.join("data/taps.json")).ok()?;
        index_of(&serde_json::from_str(&text).unwrap())
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        TEST_GIT.with(|g| *g.borrow_mut() = None);
    }
}

fn index_of(doc: &Value) -> Option<BTreeMap<String, (String, String)>> {
    Some(
        doc.as_object()?
            .iter()
            .map(|(name, tap)| {
                (
                    name.clone(),
                    (
                        tap["repo"].as_str().unwrap().to_string(),
                        tap["url"].as_str().unwrap().to_string(),
                    ),
                )
            })
            .collect(),
    )
}

fn args_with_group(args: &[String]) -> Vec<String> {
    let mut list = vec!["skills".to_string()];
    list.extend(args.iter().cloned());
    list
}

fn run(list: &[String]) -> (i32, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(list, &mut out, &mut err);
    let mut text = String::from_utf8(out).unwrap();
    text.push_str(&String::from_utf8(err).unwrap());
    (code, text)
}

fn fixture(rel: &str) -> PathBuf {
    fixture_root().join(rel)
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

enum Expected {
    Text(String, i32),
    Starts(String, i32),
}

/// A table row in rich's box drawing with runs of padding and rule characters collapsed, because
/// the path column is as wide as the scratch path, which differs between mcpm's layout and ours.
fn squash(text: &str) -> String {
    text.lines()
        .map(|line| {
            if !line.starts_with(['┏', '┃', '┡', '│', '└']) {
                return line.to_string();
            }
            let mut out = String::new();
            let mut prev = '\0';
            for c in line.chars() {
                let c = if matches!(c, '━' | '─') { '-' } else { c };
                if matches!(c, ' ' | '-') && prev == c {
                    continue;
                }
                out.push(c);
                prev = c;
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// mcpm logs a pydantic report for a SKILL.md that fails validation; ours is the one line
/// `Failed to parse <path>: <reason>` (DEV-SKL8-5 style).
fn parse_failures_in_one_line(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        match line.strip_suffix(": 1 validation error for SkillFrontmatter") {
            Some(head) if line.starts_with("Failed to parse ") => {
                let reason = lines[at + 2]
                    .trim_start()
                    .strip_prefix("Value error, ")
                    .and_then(|r| r.split(" [type=").next())
                    .expect("validation reason");
                out.push(format!("{head}: {reason}"));
                at += 4;
            }
            _ => {
                out.push(line.to_string());
                at += 1;
            }
        }
    }
    out.join("\n")
}

/// mcpm's text with the hints pointing at `toolportctl` (D-042), its tap directory at ours and
/// the parse reports in one line.
fn mapped(mcpm: &str) -> String {
    parse_failures_in_one_line(mcpm.trim_end())
        .replace("mcpm sync tap ", "toolportctl skills tap ")
        .replace("mcpm sync install ", "toolportctl skills install ")
        .replace("'mcpm skills sync'", "'toolportctl skills sync'")
        .replace("<root>/home/.config/mcpm/taps/", "<root>/data/taps/")
}

/// Cases whose tap `install` registers on its own, with the line ours adds for it.
const AUTO_TAPS: &[(&str, &str)] = &[
    ("install-autotap", "acme/skills"),
    ("install-audit-blocked", "acme/audit"),
    ("install-no-audit", "acme/audit"),
    ("install-audit-medium", "acme/mild"),
];

fn after_resolving(text: &str, extra: &str) -> String {
    let (first, rest) = text.split_once('\n').expect("resolving line");
    format!("{first}\n{extra}\n{rest}")
}

fn expected(name: &str, mcpm: &str, code: i32) -> Expected {
    let mut text = mapped(mcpm);
    match name {
        "tap-add-duplicate" => Expected::Text("toolportctl: Tap 'acme-skills' already exists".into(), 1),
        "tap-add-missing-remote" => Expected::Starts(
            "toolportctl: Failed to clone https://github.com/acme/nonexistent.git: fatal: '<root>/_infra/remotes/acme/nonexistent.git' does not appear to be a git repository".into(),
            1,
        ),
        "tap-remove-missing" | "tap-update-unknown" => Expected::Text(
            format!("toolportctl: Tap '{}' not found.", mcpm_args(name).last().unwrap()),
            1,
        ),
        "tap-update-failed" => Expected::Text(text, 1),
        "install-bad-spec" => Expected::Text(
            "toolportctl: invalid install spec \"acme\": expected @user/repo[/skill][@version]".into(),
            1,
        ),
        "install-missing-skill" | "install-empty-tap" => {
            let mut lines: Vec<&str> = text.lines().skip(1).collect();
            let error = lines
                .pop()
                .and_then(|l| l.strip_prefix("Error: "))
                .expect("error line");
            let mut out = vec![format!("toolportctl: {error}")];
            out.extend(lines.iter().map(|l| l.to_string()));
            Expected::Text(out.join("\n"), 1)
        }
        _ => {
            if let Some((_, repo)) = AUTO_TAPS.iter().find(|(case, _)| *case == name) {
                let tap = repo.replace('/', "-");
                text = after_resolving(
                    &text,
                    &format!("Added tap '{tap}' (https://github.com/{repo}.git)."),
                );
            }
            if name == "install-version" {
                text = after_resolving(
                    &text,
                    "Version '1.2.0' is ignored: a tap installs its checked-out head.",
                );
            }
            let code = if name == "install-audit-blocked" { 1 } else { code };
            Expected::Text(text, code)
        }
    }
}

/// Ours adds the reason git gave, over several lines, to a `Failed to update` row; mcpm prints
/// none.
fn without_reasons(name: &str, text: &str) -> String {
    if name != "tap-update-failed" {
        return text.to_string();
    }
    let first = text.lines().next().unwrap_or_default();
    first.split(": ").next().unwrap_or(first).to_string()
}

fn replay(case: &Case) -> Vec<String> {
    let mut failures = Vec::new();
    let name = case.name;
    let fx = Fx::new(case);
    (case.setup)(&fx);
    let mcpm: Vec<String> = mcpm_args(name).iter().map(|a| fx.expand(a)).collect();
    let (code, text) = run(&args_with_group(&mcpm));
    let text = without_reasons(name, &fx.normalised(&text));
    let mcpm_text = recorded(name, "output.txt").expect("output.txt");
    let mcpm_code: i32 = recorded(name, "exit.txt").unwrap().trim().parse().unwrap();
    let (want, want_code, prefix_only) = match expected(name, &mcpm_text, mcpm_code) {
        Expected::Text(want, code) => (want, code, false),
        Expected::Starts(want, code) => (want, code, true),
    };
    let same = if prefix_only {
        text.trim_end().starts_with(want.trim_end())
    } else {
        squash(text.trim_end()) == squash(want.trim_end())
    };
    if !same {
        failures.push(format!(
            "{name}: text\n--- expected{}\n{want}\n--- actual\n{}",
            if prefix_only { " (prefix)" } else { "" },
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
    let want_index =
        recorded(name, "index.json").map(|t| index_of(&serde_json::from_str(&t).unwrap()).unwrap());
    if want_index != fx.index() {
        failures.push(format!(
            "{name}: tap index\n  expected {want_index:?}\n  actual   {:?}",
            fx.index()
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
fn the_recorded_cases_exercise_each_command_and_its_failure_paths() {
    let has = |case: &str, needle: &str| recorded(case, "output.txt").unwrap().contains(needle);
    assert!(has("tap-add-duplicate", "already exists"));
    assert!(has("tap-update-failed", "Failed to update acme-skills"));
    assert!(has(
        "install-audit-blocked",
        "Security audit found high-severity issues"
    ));
    assert!(has("install-audit-medium", "(none high severity)"));
    assert!(has("install-twice", "already exists"));
    assert!(has("search-no-taps", "No taps registered"));
    assert_eq!(
        recorded("tap-update-unknown", "output.txt")
            .unwrap()
            .lines()
            .count(),
        2,
        "mcpm updates every tap for a name it does not know"
    );
    assert_eq!(
        recorded("install-bad-spec", "exit.txt").unwrap().trim(),
        "0"
    );
}
