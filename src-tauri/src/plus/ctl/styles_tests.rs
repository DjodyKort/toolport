use super::run_with;
use super::agents_golden_tests::copy_dir;
use super::styles_golden_tests::fixture;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

struct Fx {
    _base: DataDirFx,
    root: PathBuf,
    home: PathBuf,
    repo: PathBuf,
}

impl Fx {
    fn new(tag: &str, fixture_repo: &str) -> Self {
        let base = DataDirFx::with_data_subdir("ctl-styles", tag, "data");
        let root = base.dir.canonicalize().unwrap();
        let (home, repo) = (root.join("home"), root.join("repo"));
        std::fs::create_dir_all(&home).unwrap();
        copy_dir(&fixture("repos").join(fixture_repo), &repo);
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        Self {
            _base: base,
            root,
            home,
            repo,
        }
    }

    fn put(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn arg(&self, rel: &str) -> String {
        self.root.join(rel).to_string_lossy().into_owned()
    }

    fn text(&self, rel: &str) -> String {
        std::fs::read_to_string(self.root.join(rel)).unwrap()
    }

    fn snapshot(&self) -> BTreeMap<String, Option<Vec<u8>>> {
        tree_snapshot(&self.root)
    }

    fn normalised(&self, text: &str) -> String {
        text.replace(self.root.to_string_lossy().as_ref(), "<root>")
    }

    fn ctl(&self, group_args: &[&str]) -> (i32, String, String) {
        let repo = self.arg("repo");
        let mut list = vec!["styles", group_args[0], "--path", &repo];
        list.extend_from_slice(&group_args[1..]);
        run(&list)
    }

    fn ok(&self, group_args: &[&str]) {
        let (code, out, err) = self.ctl(group_args);
        assert_eq!(code, 0, "{group_args:?}: {out}{err}");
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
    }
}

fn run(list: &[&str]) -> (i32, String, String) {
    let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&list, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn cli(list: &[&str]) -> (i32, Value, String) {
    let mut full = vec!["--json"];
    full.extend_from_slice(list);
    let (code, out, err) = run(&full);
    (
        code,
        serde_json::from_str(out.trim()).unwrap_or(Value::Null),
        err,
    )
}

fn strs(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect()
}

fn names(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect()
}

fn lock_entry() -> Value {
    json!({"source": "local", "version": null, "hash": "sha256:0", "clients_synced": [],
           "warnings": [], "output_files": {}, "hooks_installed": {}})
}

fn lock_with_styles(styles: Value) -> String {
    serde_json::to_string_pretty(&json!({
        "version": 1, "synced_at": "t", "scope": "project", "output_root": "",
        "skills": {}, "rules": {}, "agents": {}, "styles": styles, "active_styles": {},
    }))
    .unwrap()
}

#[test]
fn ls_json_carries_the_styles_and_equals_the_handler_data() {
    let fx = Fx::new("ls-json", "basic");
    let repo = fx.arg("repo");
    let (code, v, err) = cli(&["styles", "ls", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["command"], "styles ls");
    assert_eq!(v["ok"], true);
    let data = &v["data"];
    assert_eq!(names(&data["styles"]), ["concise", "teacher", "terse"]);
    assert_eq!(data["styles"][0]["keepCodingInstructions"], true);
    assert_eq!(data["styles"][1]["keepCodingInstructions"], false);
    assert_eq!(data["styles"][0]["synced"], false);
    assert_eq!(data["lockfilePresent"], false);
    assert_eq!(data["active"], json!([]));
    assert_eq!(data["discoveryWarnings"], json!([]));
    let served = crate::plus::dispatch("plus.styles.list", json!({"repo_path": repo})).unwrap();
    assert_eq!(&served, data);
}

#[test]
fn repo_is_an_alias_of_path_and_the_repository_is_found_from_a_subdirectory() {
    let fx = Fx::new("ls-alias", "basic");
    let (code, by_path, _) = cli(&["styles", "ls", "--path", &fx.arg("repo")]);
    assert_eq!(code, 0);
    let (code, by_repo, _) = cli(&["styles", "ls", "--repo", &fx.arg("repo")]);
    assert_eq!(code, 0);
    assert_eq!(by_path, by_repo);
    let (code, below, _) = cli(&["styles", "ls", "--path", &fx.arg("repo/styles/concise")]);
    assert_eq!(code, 0);
    assert_eq!(below["data"]["repo"], by_path["data"]["repo"]);
}

#[test]
fn ls_text_cuts_long_descriptions_and_lists_the_active_styles() {
    let fx = Fx::new("ls-text", "basic");
    fx.ok(&["apply", "teacher", "--project", "--client", "cursor"]);
    let (code, out, err) = fx.ctl(&["ls"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("Explains every step like a patient teacher who checks unders..."),
        "{out}"
    );
    assert!(out.contains("│ concise │"), "{out}");
    assert!(out.ends_with("\n\nActive styles (Tier 2 clients):\n  cursor: teacher\n"), "{out}");
}

#[test]
fn lint_exit_codes_and_json_follow_the_findings() {
    let fx = Fx::new("lint", "lint");
    let repo = fx.arg("repo");
    let (code, v, _) = cli(&["styles", "lint", "--path", &repo]);
    assert_eq!(code, 1);
    let data = &v["data"];
    assert_eq!(data["styleCount"], 8);
    assert_eq!(data["errors"], 4);
    assert_eq!(data["warnings"], 7);
    assert_eq!(data["infos"], 0);
    assert!(data["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["level"] == "warning" && m["name"] == "huge"));
    let served = crate::plus::dispatch("plus.styles.lint", json!({"repo_path": repo})).unwrap();
    assert_eq!(&served, data);
    let (code, out, _) = run(&["styles", "lint", "--path", &repo]);
    assert_eq!(code, 1);
    assert!(out.ends_with("\n  4 error(s), 7 warning(s)\n"), "{out}");

    drop(fx);
    let clean = Fx::new("lint-clean", "basic");
    let (code, out, _) = clean.ctl(&["lint"]);
    assert_eq!(code, 0);
    assert_eq!(out, "All 3 style(s) passed lint checks.\n");
    drop(clean);
    let (code, out, _) = Fx::new("lint-none", "empty").ctl(&["lint"]);
    assert_eq!(code, 0);
    assert_eq!(out, "No styles found to lint.\n");
}

#[test]
fn diff_reports_new_modified_and_removed_styles_against_the_lock() {
    let fx = Fx::new("diff", "basic");
    let repo = fx.arg("repo");
    let (code, v, _) = cli(&["styles", "diff", "--path", &repo]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["noLockfile"], true);
    assert_eq!(v["data"]["new"], json!(["concise", "teacher", "terse"]));

    fx.ok(&["sync", "--project"]);
    let (_, v, _) = cli(&["styles", "diff", "--path", &repo]);
    assert_eq!(v["data"]["clean"], true);
    assert_eq!(v["data"]["unchanged"], 3);

    let concise = fx.repo.join("styles/concise/STYLE.md");
    let mut text = std::fs::read_to_string(&concise).unwrap();
    text.push_str("more\n");
    std::fs::write(concise, text).unwrap();
    std::fs::remove_dir_all(fx.repo.join("styles/terse")).unwrap();
    fx.put(
        "repo/styles/newone/STYLE.md",
        "---\nname: newone\ndescription: A new style for testing the diff output\n---\nbody\n",
    );
    let (code, v, _) = cli(&["styles", "diff", "--path", &repo]);
    assert_eq!(code, 0);
    let data = &v["data"];
    assert_eq!(data["new"], json!(["newone"]));
    assert_eq!(data["modified"], json!(["concise"]));
    assert_eq!(data["removed"], json!(["terse"]));
    assert_eq!(data["unchanged"], 1);
    assert_eq!(data["clean"], false);
}

#[test]
fn sync_defaults_to_the_user_level_and_leaves_the_repository_untouched() {
    let fx = Fx::new("sync-global", "basic");
    let before = tree_snapshot(&fx.repo);
    fx.ok(&["sync", "--client", "claude-code"]);
    assert_eq!(tree_snapshot(&fx.repo), before);
    for style in ["concise", "teacher", "terse"] {
        assert!(fx.home.join(format!(".claude/output-styles/{style}.md")).is_file());
    }
    assert!(!fx.home.join(".roomodes").exists());
    assert!(fx.root.join("data/mcpm-skills.lock").is_file());

    let (code, out, _) = fx.ctl(&["sync", "--client", "claude-code"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("Global mode -- writing to user-level paths.\n"), "{out}");
    let (_, v, _) = cli(&["styles", "sync", "--path", &fx.arg("repo"), "--dry-run"]);
    assert_eq!(v["data"]["scope"], "global");
    assert_eq!(fx.normalised(v["data"]["outputRoot"].as_str().unwrap()), "<root>/home");
}

#[test]
fn project_sync_writes_the_repository_and_the_lock_beside_it() {
    let fx = Fx::new("sync-project", "basic");
    let (code, v, err) = cli(&["styles", "sync", "--path", &fx.arg("repo"), "--project"]);
    assert_eq!(code, 0, "{err}");
    let data = &v["data"];
    assert_eq!(data["scope"], "project");
    assert_eq!(data["styleCount"], 3);
    assert_eq!(data["clientCount"], 2);
    assert_eq!(strs(&data["styles"][0]["clientsSynced"]), ["claude-code", "roomodes-style"]);
    let outputs: Vec<String> = strs(&data["outputs"]).iter().map(|s| fx.normalised(s)).collect();
    assert!(outputs.contains(&"<root>/repo/.claude/output-styles/terse.md".to_string()));
    assert!(outputs.contains(&"<root>/repo/.roomodes".to_string()));
    assert!(fx.repo.join("mcpm-skills.lock").is_file());
    assert!(!fx.root.join("data/mcpm-skills.lock").exists());
    assert!(!fx.home.join(".claude").exists());
}

#[test]
fn status_reads_the_user_level_lock_when_the_repository_has_none() {
    let fx = Fx::new("status-global", "basic");
    let repo = fx.arg("repo");
    let (_, before, _) = cli(&["styles", "status", "--path", &repo]);
    assert_eq!(before["data"]["lockfilePresent"], false);
    let (_, out, _) = run(&["styles", "status", "--path", &repo]);
    assert_eq!(out, "No lockfile found. Run 'toolportctl styles sync' first.\n");

    fx.ok(&["sync", "--client", "claude-code"]);
    fx.ok(&["apply", "terse", "--client", "windsurf"]);
    let (code, v, err) = cli(&["styles", "status", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    let data = &v["data"];
    assert_eq!(data["lockfilePresent"], true);
    assert_eq!(data["native"][0]["name"], "Claude Code");
    assert_eq!(data["native"][0]["styles"], json!(["concise", "teacher", "terse"]));
    assert_eq!(data["native"][1]["styles"], json!([]));
    assert_eq!(data["applyRemove"].as_array().unwrap().len(), 13);
    let windsurf = data["applyRemove"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["client"] == "windsurf")
        .unwrap();
    assert_eq!(windsurf["active"], "terse");
    let served = crate::plus::dispatch("plus.styles.status", json!({"repo_path": repo})).unwrap();
    assert_eq!(&served, data);
}

#[test]
fn dry_runs_write_nothing_at_either_scope_and_predict_the_real_run() {
    let fx = Fx::new("dry-run", "basic");
    let repo = fx.arg("repo");
    let before = fx.snapshot();
    for scope in ["--project", "--global"] {
        let (code, out, err) = run(&["styles", "sync", "--path", &repo, scope, "--dry-run"]);
        assert_eq!(code, 0, "{err}");
        assert!(out.starts_with("Dry run -- no files will be written.\n"), "{out}");
        assert!(out.contains("(dry run) Synced 3 style(s) to 2 native client(s)."), "{out}");
        let (code, out, _) = run(&["styles", "apply", "concise", "--path", &repo, scope, "--dry-run"]);
        assert_eq!(code, 0);
        assert!(out.contains("(dry run) Applied style 'concise' to 13 client(s)."), "{out}");
        assert_eq!(fx.snapshot(), before, "{scope}");
    }
    let (code, out, _) = run(&["styles", "add", "fresh", "--path", &repo, "--dry-run"]);
    assert_eq!(code, 0);
    assert_eq!(
        fx.normalised(&out),
        "Would create style 'fresh':\n  <root>/repo/styles/fresh/STYLE.md\n"
    );
    assert_eq!(fx.snapshot(), before);

    fx.ok(&["sync", "--project"]);
    fx.ok(&["apply", "concise", "--project"]);
    let applied = fx.snapshot();

    let (code, v, err) = cli(&["styles", "remove", "--path", &repo, "--project", "--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(v["data"]["removed"].as_array().unwrap().len(), 13);
    let (_, out, _) = run(&["styles", "remove", "--path", &repo, "--project", "--dry-run"]);
    assert!(out.starts_with("Dry run -- no files will be removed.\n\n(dry run) Removing style 'concise' from aider\n"), "{out}");
    assert!(out.ends_with("\nDone. Would remove active style from 13 client(s).\n"), "{out}");
    assert_eq!(fx.snapshot(), applied);

    let (code, v, _) = cli(&["styles", "clean", "--path", &repo, "--project", "--dry-run"]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(v["data"]["lockUpdated"], false);
    let planned: Vec<String> = strs(&v["data"]["removed"]).iter().map(|s| s.to_string()).collect();
    assert_eq!(planned.len(), 17);
    let (_, out, _) = run(&["styles", "clean", "--path", &repo, "--project", "--dry-run"]);
    assert!(out.starts_with("  Would remove: "), "{out}");
    assert!(out.ends_with("\nWould clean 17 style file(s) from clients.\n"), "{out}");
    assert_eq!(fx.snapshot(), applied);

    let (_, v, _) = cli(&["styles", "clean", "--path", &repo, "--project"]);
    assert_eq!(v["data"]["dryRun"], false);
    assert_eq!(v["data"]["lockUpdated"], true);
    let removed: Vec<String> = strs(&v["data"]["removed"]).iter().map(|s| s.to_string()).collect();
    assert_eq!(removed, planned);
    assert_ne!(fx.snapshot(), applied);
    assert!(removed.iter().all(|p| !std::path::Path::new(p).exists()));
}

#[test]
fn add_writes_the_mcpm_template_that_parses_and_refuses_what_already_exists() {
    let fx = Fx::new("add", "basic");
    let repo = fx.arg("repo");
    let (code, v, err) = cli(&["styles", "add", "pirate", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["dryRun"], false);
    let text = fx.text("repo/styles/pirate/STYLE.md");
    assert!(text.starts_with("---\nname: pirate\n"), "{text}");
    assert!(text.contains("keep-coding-instructions: true\n"), "{text}");
    let (_, v, _) = cli(&["styles", "ls", "--path", &repo]);
    assert_eq!(names(&v["data"]["styles"]), ["concise", "pirate", "teacher", "terse"]);
    assert_eq!(v["data"]["discoveryWarnings"], json!([]));

    let before = fx.snapshot();
    for (name, needle) in [
        ("concise", "Style 'concise' already exists at"),
        ("Bad_Name", "Invalid style name 'Bad_Name'"),
        ("a--b", "consecutive hyphens"),
        ("../escape", "Invalid style name '../escape'"),
        ("..", "Invalid style name '..'"),
    ] {
        let (code, out, err) = run(&["styles", "add", name, "--path", &repo]);
        assert_eq!(code, 1, "{name}: {out}{err}");
        assert!(err.contains(needle), "{name}: {err}");
    }
    assert_eq!(fx.snapshot(), before);
    assert!(!fx.root.join("escape").exists());
}

#[test]
fn apply_reports_the_replaced_styles_and_the_native_clients() {
    let fx = Fx::new("apply", "basic");
    let repo = fx.arg("repo");
    let (code, v, err) = cli(&["styles", "apply", "concise", "--path", &repo, "--project"]);
    assert_eq!(code, 0, "{err}");
    let data = &v["data"];
    assert_eq!(data["appliedCount"], 13);
    assert_eq!(data["replaced"], json!([]));
    assert_eq!(data["applied"].as_array().unwrap().len(), 13);
    assert_eq!(data["nativeClients"], json!([]));
    assert_eq!(fx.normalised(data["applied"][12]["path"].as_str().unwrap()), "<root>/repo/.rules");

    let (_, v, _) = cli(&[
        "styles", "apply", "teacher", "--path", &repo, "--project", "--client", "cursor", "--dry-run",
    ]);
    assert_eq!(v["data"]["replaced"], json!([{"client": "cursor", "style": "concise"}]));
    assert_eq!(v["data"]["appliedCount"], 1);
    assert_eq!(v["data"]["dryRun"], true);
    let lock = fx.text("repo/mcpm-skills.lock");
    assert!(!lock.contains("teacher"), "a dry run leaves the lock alone");

    let (_, out, _) = run(&[
        "styles", "apply", "teacher", "--path", &repo, "--project", "--client", "cursor",
    ]);
    assert!(out.starts_with("Replacing active style 'concise' on cursor\n"), "{out}");
    let (_, v, _) = cli(&["styles", "ls", "--path", &repo]);
    let active: Vec<String> = v["data"]["active"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| format!("{}={}", p["client"], p["style"]))
        .collect();
    assert!(active.contains(&"\"cursor\"=\"teacher\"".to_string()), "{active:?}");
    assert!(active.contains(&"\"zed\"=\"concise\"".to_string()), "{active:?}");

    let (code, out, _) = run(&[
        "styles", "apply", "concise", "--path", &repo, "--project", "--client", "claude-code",
    ]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with("Note: 'claude-code' supports native style toggling. Use 'toolportctl styles sync' instead"),
        "{out}"
    );
    assert!(out.contains("Applied style 'concise' to 12 client(s)."), "{out}");
}

#[test]
fn apply_refuses_an_unknown_style_and_a_repository_without_styles() {
    let fx = Fx::new("apply-bad", "basic");
    let before = fx.snapshot();
    let (code, out, err) = fx.ctl(&["apply", "nope", "--project"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        err,
        "toolportctl: Style 'nope' not found. Available: concise, teacher, terse\n"
    );
    assert_eq!(fx.snapshot(), before);
    drop(fx);
    let empty = Fx::new("apply-none", "empty");
    let (code, _, err) = empty.ctl(&["apply", "nope", "--project"]);
    assert_eq!(code, 1);
    assert_eq!(err, "toolportctl: Style 'nope' not found. Available: none\n");
}

#[test]
fn remove_counts_the_clients_it_cleared_which_mcpm_reports_as_zero() {
    let fx = Fx::new("remove", "basic");
    let repo = fx.arg("repo");
    fx.ok(&["apply", "concise", "--project"]);
    let (code, out, err) = run(&["styles", "remove", "--path", &repo, "--project", "--client", "cursor"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.ends_with("\nDone. Removed active style from 1 client(s).\n"), "{out}");
    assert!(!fx.repo.join(".cursor/rules/mcpm-output-style/RULE.md").exists());
    assert!(fx.repo.join(".windsurf/rules/mcpm-output-style.md").is_file());

    let (code, v, _) = cli(&["styles", "remove", "--path", &repo, "--project"]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["removed"].as_array().unwrap().len(), 12);
    assert_eq!(v["data"]["active"], json!([]));
    assert!(!fx.repo.join(".windsurf/rules/mcpm-output-style.md").exists());

    let (code, out, _) = run(&["styles", "remove", "--path", &repo, "--project"]);
    assert_eq!(code, 0);
    assert_eq!(out, "No active styles to remove.\n");
    fx.ok(&["apply", "concise", "--project", "--client", "aider"]);
    let (code, out, _) = run(&["styles", "remove", "--path", &repo, "--project", "--client", "cursor"]);
    assert_eq!(code, 0);
    assert_eq!(out, "No active style on client 'cursor'.\n");
}

#[test]
fn remove_and_clean_keep_what_the_user_wrote_in_a_shared_zed_rules_file() {
    let fx = Fx::new("zed", "basic");
    fx.put("repo/.rules", "Be kind.\n");
    fx.ok(&["apply", "concise", "--project", "--client", "zed"]);
    assert!(fx.text("repo/.rules").contains("mcpm-style:start"));
    fx.ok(&["remove", "--project", "--client", "zed"]);
    assert_eq!(fx.text("repo/.rules"), "Be kind.\n");
    fx.ok(&["apply", "concise", "--project", "--client", "zed"]);
    fx.ok(&["clean", "--project"]);
    assert_eq!(fx.text("repo/.rules"), "Be kind.\n");
}

#[test]
fn clean_ignores_tampered_lock_names_and_never_leaves_the_root() {
    let fx = Fx::new("clean-tamper", "empty");
    let repo = fx.arg("repo");
    let absolute = fx.root.join("victim-abs").to_string_lossy().into_owned();
    let tampered = [
        "../../../victim".to_string(),
        "x/../../../../victim".to_string(),
        absolute,
    ];
    let victims = ["victim.md", "victim-abs.md"];
    for victim in victims {
        fx.put(victim, "keep");
    }
    std::fs::create_dir_all(fx.repo.join(".claude/output-styles")).unwrap();
    let mut styles = serde_json::Map::new();
    for name in &tampered {
        styles.insert(name.clone(), lock_entry());
    }
    fx.put("repo/mcpm-skills.lock", &lock_with_styles(Value::Object(styles)));
    let before = fx.snapshot();

    let (code, v, err) = cli(&["styles", "clean", "--project", "--path", &repo, "--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["removed"], json!([]));
    assert_eq!(v["data"]["managed"], json!([]));
    let mut ignored = strs(&v["data"]["ignored"]);
    ignored.sort_unstable();
    let mut expected: Vec<&str> = tampered.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(ignored, expected);
    assert_eq!(fx.snapshot(), before);

    let (code, out, _) = run(&["styles", "clean", "--project", "--path", &repo]);
    assert_eq!(code, 0);
    assert!(out.starts_with("No style files found to clean.\n"), "{out}");
    assert!(out.contains("Ignored lock entry with invalid name: ../../../victim\n"), "{out}");
    for victim in victims {
        assert_eq!(fx.text(victim), "keep", "{victim}");
    }
    let mut after = fx.snapshot();
    let mut kept = before;
    after.remove("repo/mcpm-skills.lock");
    kept.remove("repo/mcpm-skills.lock");
    assert_eq!(after, kept, "nothing but the lock changed");
}

#[test]
fn clean_still_cleans_valid_names_beside_tampered_ones() {
    let fx = Fx::new("clean-mixed", "basic");
    let repo = fx.arg("repo");
    fx.ok(&["sync", "--project", "--client", "claude-code"]);
    fx.put("victim.md", "keep");
    let lock = fx.text("repo/mcpm-skills.lock");
    let tampered = lock.replacen(
        "\"styles\": {",
        "\"styles\": {\n    \"../../victim\": {\"source\": \"local\", \"version\": null, \"hash\": \"sha256:0\", \"clients_synced\": [], \"warnings\": [], \"output_files\": {}, \"hooks_installed\": {}},",
        1,
    );
    assert_ne!(lock, tampered);
    fx.put("repo/mcpm-skills.lock", &tampered);

    let (code, out, err) = run(&["styles", "clean", "--project", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("  Removed: "), "{out}");
    assert!(out.contains("Ignored lock entry with invalid name: ../../victim"), "{out}");
    assert_eq!(fx.text("victim.md"), "keep");
    assert!(!fx.repo.join(".claude/output-styles/teacher.md").exists());
}

#[test]
fn a_tampered_style_name_in_style_md_is_skipped_and_writes_nothing_outside() {
    let fx = Fx::new("name-tamper", "basic");
    let repo = fx.arg("repo");
    fx.put(
        "repo/styles/evil/STYLE.md",
        "---\nname: ../../escape\ndescription: Name tries to leave the repository\n---\nbody\n",
    );
    fx.put(
        "repo/styles/Bad_Name/STYLE.md",
        "---\nname: Bad_Name\ndescription: An uppercase name is refused by the parser\n---\nbody\n",
    );
    let (code, out, _) = run(&["styles", "ls", "--path", &repo]);
    assert_eq!(code, 0);
    let out = fx.normalised(&out);
    assert!(out.contains("Failed to parse <root>/repo/styles/Bad_Name/STYLE.md"), "{out}");
    assert!(out.contains("Failed to parse <root>/repo/styles/evil/STYLE.md"), "{out}");

    let (_, v, _) = cli(&["styles", "ls", "--path", &repo]);
    assert_eq!(v["data"]["discoveryWarnings"].as_array().unwrap().len(), 2);
    assert_eq!(names(&v["data"]["styles"]), ["concise", "teacher", "terse"]);

    let before = tree_snapshot(&fx.root.join("home"));
    fx.ok(&["sync", "--project"]);
    fx.ok(&["apply", "concise", "--project"]);
    assert!(!fx.root.join("escape.md").exists());
    assert!(!fx.repo.join(".claude/output-styles/evil.md").exists());
    assert_eq!(tree_snapshot(&fx.root.join("home")), before);
}

#[test]
fn partial_repositories_list_the_directories_without_a_style_file() {
    let fx = Fx::new("partial", "partial");
    let (code, out, err) = fx.ctl(&["ls"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("Style directory nostyle has no STYLE.md, skipping\n"), "{out}");
    let (_, v, _) = cli(&["styles", "sync", "--path", &fx.arg("repo"), "--project"]);
    assert_eq!(v["data"]["foundCount"], 1);
    assert_eq!(v["data"]["discoveryWarnings"].as_array().unwrap().len(), 1);
}

#[test]
fn usage_errors_exit_2_and_touch_nothing() {
    let fx = Fx::new("usage", "basic");
    let repo = fx.arg("repo");
    let before = fx.snapshot();
    let cases: Vec<Vec<&str>> = vec![
        vec!["styles"],
        vec!["styles", "nosuch"],
        vec!["styles", "ls", "extra", "--path", &repo],
        vec!["styles", "add", "--path", &repo],
        vec!["styles", "add", "a", "b", "--path", &repo],
        vec!["styles", "apply", "--path", &repo],
        vec!["styles", "apply", "a", "b", "--path", &repo],
        vec!["styles", "remove", "extra", "--path", &repo],
        vec!["styles", "clean", "--global", "--project", "--path", &repo],
        vec!["styles", "sync", "--global", "--project", "--path", &repo],
        vec!["styles", "sync", "--client"],
        vec!["styles", "clean", "--client", "cursor", "--path", &repo],
        vec!["styles", "ls", "--dry-run", "--path", &repo],
        vec!["styles", "status", "--project", "--path", &repo],
    ];
    for list in cases {
        let (code, out, err) = run(&list);
        assert_eq!(code, 2, "{list:?}: {out}{err}");
        assert!(err.starts_with("toolportctl: "), "{list:?}: {err}");
    }
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn a_missing_repository_fails_every_command_except_a_user_level_clean() {
    let fx = Fx::new("norepo", "empty");
    let nowhere = fx.arg("nowhere");
    for list in [
        vec!["styles", "ls", "--path", &nowhere],
        vec!["styles", "lint", "--path", &nowhere],
        vec!["styles", "diff", "--path", &nowhere],
        vec!["styles", "status", "--path", &nowhere],
        vec!["styles", "sync", "--path", &nowhere],
        vec!["styles", "apply", "x", "--path", &nowhere],
        vec!["styles", "remove", "--path", &nowhere],
        vec!["styles", "add", "x", "--path", &nowhere],
        vec!["styles", "clean", "--project", "--path", &nowhere],
    ] {
        let (code, out, err) = run(&list);
        assert_eq!(code, 1, "{list:?}: {out}");
        assert_eq!(err, "toolportctl: no skills repository found\n", "{list:?}");
    }
    let (code, out, _) = run(&["styles", "clean", "--path", &nowhere]);
    assert_eq!(code, 0);
    assert_eq!(out, "No style files found to clean.\n");
}

#[test]
fn plus_handlers_reject_missing_names_and_missing_repositories() {
    let fx = Fx::new("handlers", "empty");
    let nowhere = fx.arg("nowhere");
    for command in ["plus.styles.add", "plus.styles.apply"] {
        let err = crate::plus::dispatch(command, json!({"repo_path": fx.arg("repo")})).unwrap_err();
        assert_eq!(err, "name is required", "{command}");
    }
    for command in [
        "plus.styles.list",
        "plus.styles.lint",
        "plus.styles.diff",
        "plus.styles.status",
        "plus.styles.sync",
        "plus.styles.remove",
    ] {
        let err = crate::plus::dispatch(command, json!({"repo_path": nowhere})).unwrap_err();
        assert_eq!(err, "no skills repository found", "{command}");
    }
    let err = crate::plus::dispatch("plus.styles.add", json!({"repo_path": nowhere, "name": "x"}))
        .unwrap_err();
    assert_eq!(err, "no skills repository found");
    let err = crate::plus::dispatch(
        "plus.styles.add",
        json!({"repo_path": fx.arg("repo"), "name": "../x"}),
    )
    .unwrap_err();
    assert!(err.starts_with("Invalid style name '../x'"), "{err}");
    let empty = crate::plus::dispatch("plus.styles.sync", json!({"repo_path": fx.arg("repo")}))
        .unwrap();
    assert_eq!(empty["foundCount"], 0);
    assert_eq!(empty["styles"], json!([]));
    assert!(!fx.root.join("data/mcpm-skills.lock").exists());
}

#[test]
fn json_output_of_every_mutating_command_reports_dry_run() {
    let fx = Fx::new("json-dry", "basic");
    let repo = fx.arg("repo");
    for list in [
        vec!["styles", "sync", "--path", &repo, "--project"],
        vec!["styles", "apply", "concise", "--path", &repo, "--project"],
        vec!["styles", "remove", "--path", &repo, "--project"],
        vec!["styles", "clean", "--path", &repo, "--project"],
        vec!["styles", "add", "fresh", "--path", &repo],
    ] {
        let (code, v, err) = cli(&list);
        assert_eq!(code, 0, "{list:?}: {err}");
        assert_eq!(v["data"]["dryRun"], false, "{list:?}");
        assert_eq!(v["command"], format!("styles {}", list[1]));
    }
}
