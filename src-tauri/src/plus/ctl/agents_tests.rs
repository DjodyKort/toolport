use super::agents_golden_tests::{copy_dir, fixture};
use super::output::{table, table_min, wrap_cell};
use super::run_with;
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
        let base = DataDirFx::with_data_subdir("ctl-agents", tag, "data");
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

    fn sync(&self, extra: &[&str]) {
        let repo = self.arg("repo");
        let mut list = vec!["agents", "sync", "--path", &repo];
        list.extend_from_slice(extra);
        let (code, out, err) = run(&list);
        assert_eq!(code, 0, "{list:?}: {out}{err}");
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

fn lock_entry(hash: &str) -> Value {
    json!({"source": "local", "version": null, "hash": hash, "clients_synced": [],
           "warnings": [], "output_files": {}, "hooks_installed": {}})
}

fn lock_with_agents(agents: Value) -> String {
    serde_json::to_string_pretty(&json!({
        "version": 1, "synced_at": "t", "scope": "project", "output_root": "",
        "skills": {}, "rules": {}, "agents": agents, "styles": {}, "active_styles": {},
    }))
    .unwrap()
}

#[test]
fn wrap_cell_breaks_at_spaces_and_folds_words_longer_than_the_column() {
    assert_eq!(wrap_cell("", 10), "");
    assert_eq!(wrap_cell("short", 10), "short");
    assert_eq!(wrap_cell("abcde fghij", 11), "abcde fghij");
    assert_eq!(wrap_cell("one two three four", 9), "one two\nthree\nfour");
    assert_eq!(wrap_cell("abcdefghij", 4), "abcd\nefgh\nij");
    assert_eq!(wrap_cell("ab abcdefghij", 4), "ab\nabcd\nefgh\nij");
    assert_eq!(wrap_cell("a\nb c", 10), "a\nb c");
}

#[test]
fn table_min_pads_a_column_to_its_floor_and_keeps_multi_line_cells_aligned() {
    let rows = vec![vec!["x".to_string()]];
    assert_eq!(
        table_min(&["A"], &rows, &[5]),
        "┏━━━━━━━┓\n┃ A     ┃\n┡━━━━━━━┩\n│ x     │\n└───────┘"
    );
    let rows = vec![vec!["one\ntwo".to_string(), "b".to_string()]];
    assert_eq!(
        table(&["A", "B"], &rows),
        "┏━━━━━┳━━━┓\n┃ A   ┃ B ┃\n┡━━━━━╇━━━┩\n│ one │ b │\n│ two │   │\n└─────┴───┘"
    );
}

#[test]
fn ls_json_carries_the_agents_and_equals_the_handler_data() {
    let fx = Fx::new("ls-json", "basic");
    let repo = fx.arg("repo");
    let (code, v, err) = cli(&["agents", "ls", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["command"], "agents ls");
    assert_eq!(v["ok"], true);
    let data = &v["data"];
    assert_eq!(names(&data["agents"]), ["helper", "planner", "reviewer"]);
    assert_eq!(data["agents"][2]["model"], "sonnet");
    assert_eq!(data["agents"][2]["tools"], json!(["Read", "Grep", "Glob"]));
    assert_eq!(data["agents"][0]["model"], Value::Null);
    assert_eq!(data["discoveryWarnings"], json!([]));
    let served = crate::plus::dispatch("plus.agents.list", json!({"repo_path": repo})).unwrap();
    assert_eq!(&served, data);
}

#[test]
fn repo_is_an_alias_of_path_and_the_repository_is_found_from_a_subdirectory() {
    let fx = Fx::new("ls-alias", "basic");
    let (code, by_path, _) = cli(&["agents", "ls", "--path", &fx.arg("repo")]);
    assert_eq!(code, 0);
    let (code, by_repo, _) = cli(&["agents", "ls", "--repo", &fx.arg("repo")]);
    assert_eq!(code, 0);
    assert_eq!(by_path, by_repo);
    let (code, below, _) = cli(&["agents", "ls", "--path", &fx.arg("repo/agents/helper")]);
    assert_eq!(code, 0);
    assert_eq!(below["data"]["repo"], by_path["data"]["repo"]);
}

#[test]
fn lint_and_audit_exit_codes_and_json_follow_their_findings() {
    let fx = Fx::new("lint-audit", "lint");
    let repo = fx.arg("repo");
    let (code, v, _) = cli(&["agents", "lint", "--path", &repo]);
    assert_eq!(code, 1);
    let data = &v["data"];
    assert_eq!(data["errors"], 4);
    assert_eq!(data["warnings"], 4);
    assert_eq!(data["infos"], 1);
    assert_eq!(data["agentCount"], 5);
    assert!(data["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["level"] == "error" && m["name"] == "conflict"));
    let (code, out, _) = run(&["agents", "lint", "--path", &repo]);
    assert_eq!(code, 1);
    assert!(out.contains("  E conflict: Tools in both allowed and disallowed:"), "{out}");

    drop(fx);
    let audited = Fx::new("audit-high", "audit");
    let repo = audited.arg("repo");
    let (code, v, _) = cli(&["agents", "audit", "--path", &repo]);
    assert_eq!(code, 1);
    assert!(v["data"]["high"].as_u64().unwrap() > 0);
    assert_eq!(v["data"]["clean"], false);
    let first = &v["data"]["findings"][0];
    assert_eq!(first["severity"], "high");
    assert_eq!(first["agent"], "injector");
    let served = crate::plus::dispatch("plus.agents.audit", json!({"repo_path": repo})).unwrap();
    assert_eq!(served, v["data"]);
}

#[test]
fn diff_reports_new_modified_and_removed_agents_against_the_lock() {
    let fx = Fx::new("diff", "basic");
    let repo = fx.arg("repo");
    let (code, v, _) = cli(&["agents", "diff", "--path", &repo]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["noLockfile"], true);
    assert_eq!(v["data"]["new"], json!(["helper", "planner", "reviewer"]));

    fx.sync(&["--project"]);
    let (_, v, _) = cli(&["agents", "diff", "--path", &repo]);
    assert_eq!(v["data"]["clean"], true);
    assert_eq!(v["data"]["unchanged"], 3);

    let helper = fx.repo.join("agents/helper/AGENT.md");
    let mut text = std::fs::read_to_string(&helper).unwrap();
    text.push_str("more\n");
    std::fs::write(helper, text).unwrap();
    std::fs::remove_dir_all(fx.repo.join("agents/reviewer")).unwrap();
    fx.put(
        "repo/agents/newone/AGENT.md",
        "---\nname: newone\ndescription: A new agent for testing the diff output\n---\nbody\n",
    );
    let (code, v, _) = cli(&["agents", "diff", "--path", &repo]);
    assert_eq!(code, 0);
    let data = &v["data"];
    assert_eq!(data["new"], json!(["newone"]));
    assert_eq!(data["modified"], json!(["helper"]));
    assert_eq!(data["removed"], json!(["reviewer"]));
    assert_eq!(data["unchanged"], 1);
    assert_eq!(data["clean"], false);
}

#[test]
fn status_reads_the_user_level_lock_and_checks_the_home_outputs() {
    let fx = Fx::new("status-global", "basic");
    let repo = fx.arg("repo");
    fx.sync(&[]);
    assert!(fx.home.join(".claude/agents/helper.md").is_file());
    assert!(!fx.repo.join("mcpm-skills.lock").exists());
    assert!(fx.root.join("data/mcpm-skills.lock").is_file());

    let (code, v, err) = cli(&["agents", "status", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["drift"], false);
    assert_eq!(v["data"]["lockedCount"], 3);
    assert_eq!(fx.normalised(v["data"]["outputRoot"].as_str().unwrap()), "<root>/home");
    assert_eq!(v["data"]["outputs"].as_array().unwrap().len(), 12);

    std::fs::remove_file(fx.home.join(".claude/agents/helper.md")).unwrap();
    let (code, v, _) = cli(&["agents", "status", "--path", &repo, "--strict"]);
    assert_eq!(code, 1);
    assert_eq!(v["data"]["drift"], true);
    let missing: Vec<&Value> = v["data"]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["present"] == false)
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["name"], "helper");
    assert_eq!(missing[0]["client"], "claude-code");
}

#[test]
fn global_sync_writes_the_user_level_paths_and_keeps_the_repository_untouched() {
    let fx = Fx::new("sync-global", "basic");
    let before = tree_snapshot(&fx.repo);
    fx.sync(&["--client", "claude-code", "--client", "cursor"]);
    assert_eq!(tree_snapshot(&fx.repo), before);
    assert!(fx.home.join(".claude/agents/planner.md").is_file());
    assert!(fx.home.join(".cursor/agents/planner.md").is_file());
    assert!(!fx.home.join(".gemini").exists());
    let lock = fx.text("data/mcpm-skills.lock");
    assert!(lock.contains("\"scope\": \"global\""), "{lock}");
}

#[test]
fn dry_run_reports_the_real_removals_and_writes_nothing() {
    let fx = Fx::new("dry-run", "basic");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    let before = fx.snapshot();

    let (code, v, err) = cli(&["agents", "clean", "--project", "--path", &repo, "--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(fx.snapshot(), before);
    let planned: Vec<String> = strs(&v["data"]["removed"]).iter().map(|s| s.to_string()).collect();
    assert_eq!(planned.len(), 16);
    let (code, out, _) = run(&["agents", "clean", "--project", "--path", &repo, "--dry-run"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("  Would remove .claude/agents/helper.md\n"), "{out}");
    assert!(out.ends_with("\nWould clean 16 file(s).\n"), "{out}");
    assert_eq!(fx.snapshot(), before);

    let (code, v, _) = cli(&[
        "agents",
        "uninstall",
        "helper",
        "--project",
        "--path",
        &repo,
        "--dry-run",
    ]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(v["data"]["lockUpdated"], true);
    assert_eq!(strs(&v["data"]["outputs"]).len(), 6);
    let (_, out, _) = run(&[
        "agents",
        "uninstall",
        "helper",
        "--project",
        "--path",
        &repo,
        "--dry-run",
    ]);
    assert!(out.contains("  Would remove agents/helper\n"), "{out}");
    assert!(
        out.ends_with("\nWould uninstall 'helper' and remove 6 output file(s).\n"),
        "{out}"
    );
    assert_eq!(fx.snapshot(), before);

    let (_, v, _) = cli(&["agents", "clean", "--project", "--path", &repo]);
    assert_eq!(v["data"]["dryRun"], false);
    let removed: Vec<String> = strs(&v["data"]["removed"]).iter().map(|s| s.to_string()).collect();
    assert_eq!(removed, planned);
    assert_ne!(fx.snapshot(), before);
}

#[test]
fn sync_and_add_dry_runs_write_nothing_at_either_scope() {
    let fx = Fx::new("dry-sync", "basic");
    let repo = fx.arg("repo");
    let before = fx.snapshot();
    for scope in ["--project", "--global"] {
        let (code, out, err) = run(&["agents", "sync", "--path", &repo, scope, "--dry-run"]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("Dry run -- no files will be written."), "{out}");
        assert!(out.contains("(dry run) Synced 3 agent(s) to"), "{out}");
        assert_eq!(fx.snapshot(), before, "{scope}");
    }
    let (code, v, _) = cli(&["agents", "sync", "--path", &repo, "--dry-run"]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(v["data"]["scope"], "global");
    assert_eq!(fx.snapshot(), before);

    let (code, out, _) = run(&["agents", "add", "fresh", "--path", &repo, "--dry-run"]);
    assert_eq!(code, 0);
    assert_eq!(
        fx.normalised(&out),
        "Would create agent 'fresh':\n  <root>/repo/agents/fresh/AGENT.md\n"
    );
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn add_writes_the_mcpm_template_that_parses_and_refuses_what_already_exists() {
    let fx = Fx::new("add", "basic");
    let repo = fx.arg("repo");
    let (code, v, err) = cli(&["agents", "add", "code-reviewer", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["dryRun"], false);
    let text = fx.text("repo/agents/code-reviewer/AGENT.md");
    assert!(text.starts_with("---\nname: code-reviewer\n"), "{text}");
    assert!(text.contains("model: sonnet\n") && text.contains("tools: [Read, Grep, Glob]\n"));
    let (_, v, _) = cli(&["agents", "ls", "--path", &repo]);
    assert_eq!(
        names(&v["data"]["agents"]),
        ["code-reviewer", "helper", "planner", "reviewer"]
    );
    assert_eq!(v["data"]["discoveryWarnings"], json!([]));

    let before = fx.snapshot();
    for (name, needle) in [
        ("helper", "Agent 'helper' already exists."),
        ("Bad_Name", "Invalid agent name 'Bad_Name'"),
        ("a--b", "consecutive hyphens"),
        ("../escape", "Invalid agent name '../escape'"),
        ("..", "Invalid agent name '..'"),
    ] {
        let (code, out, err) = run(&["agents", "add", name, "--path", &repo]);
        assert_eq!(code, 1, "{name}: {out}{err}");
        assert!(err.contains(needle), "{name}: {err}");
    }
    assert_eq!(fx.snapshot(), before);
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
    let victims = [
        "victim.md",
        "victim.toml",
        "victim.agent.md",
        "victim-abs.md",
        "victim-abs.toml",
        "victim-abs.agent.md",
    ];
    for victim in victims {
        fx.put(victim, "keep");
    }
    for dir in [
        ".claude/agents/x",
        ".codex/agents/x",
        ".cursor/agents/x",
        ".gemini/agents/x",
        ".github/agents/x",
    ] {
        std::fs::create_dir_all(fx.repo.join(dir)).unwrap();
    }
    fx.put("repo/.roomodes", "{\"customModes\": []}");
    let mut agents = serde_json::Map::new();
    for name in &tampered {
        agents.insert(name.clone(), lock_entry("sha256:0"));
    }
    fx.put("repo/mcpm-skills.lock", &lock_with_agents(Value::Object(agents)));
    let before = fx.snapshot();

    let (code, v, err) = cli(&["agents", "clean", "--project", "--path", &repo, "--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["removed"], json!([]));
    assert_eq!(v["data"]["managed"], json!([]));
    let mut ignored = strs(&v["data"]["ignored"]);
    ignored.sort_unstable();
    let mut expected: Vec<&str> = tampered.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(ignored, expected);
    assert_eq!(fx.snapshot(), before);

    let (code, out, _) = run(&["agents", "clean", "--project", "--path", &repo]);
    assert_eq!(code, 0);
    assert!(out.starts_with("No managed agents in lockfile.\n"), "{out}");
    assert!(
        out.contains("Ignored lock entry with invalid name: ../../../victim\n"),
        "{out}"
    );
    assert_eq!(fx.snapshot(), before, "nothing inside or outside the root was touched");
    for victim in victims {
        assert_eq!(fx.text(victim), "keep", "{victim}");
    }
}

#[test]
fn clean_still_cleans_valid_names_beside_tampered_ones() {
    let fx = Fx::new("clean-mixed", "basic");
    let repo = fx.arg("repo");
    fx.sync(&["--project", "--client", "claude-code"]);
    fx.put("victim.md", "keep");
    let lock = fx.text("repo/mcpm-skills.lock");
    let tampered = lock.replacen(
        "\"agents\": {",
        "\"agents\": {\n    \"../../victim\": {\"source\": \"local\", \"version\": null, \"hash\": \"sha256:0\", \"clients_synced\": [], \"warnings\": [], \"output_files\": {}, \"hooks_installed\": {}},",
        1,
    );
    assert_ne!(lock, tampered);
    fx.put("repo/mcpm-skills.lock", &tampered);

    let (code, out, err) = run(&["agents", "clean", "--project", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("  Removed .claude/agents/helper.md\n"), "{out}");
    assert!(
        out.contains("Ignored lock entry with invalid name: ../../victim"),
        "{out}"
    );
    assert_eq!(fx.text("victim.md"), "keep");
    assert!(!fx.repo.join(".claude/agents/planner.md").exists());
}

#[test]
fn status_skips_tampered_lock_names() {
    let fx = Fx::new("status-tamper", "basic");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    fx.put("victim.md", "keep");
    let lock = fx.text("repo/mcpm-skills.lock");
    let tampered = lock.replacen(
        "\"agents\": {",
        "\"agents\": {\n    \"../victim\": {\"source\": \"local\", \"version\": null, \"hash\": \"sha256:0\", \"clients_synced\": [\"claude-code\"], \"warnings\": [], \"output_files\": {}, \"hooks_installed\": {}},",
        1,
    );
    assert_ne!(lock, tampered);
    fx.put("repo/mcpm-skills.lock", &tampered);
    let (code, v, err) = cli(&["agents", "status", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["drift"], false);
    assert!(names(&v["data"]["outputs"]).iter().all(|n| !n.contains("victim")));
}

#[test]
fn a_tampered_agent_name_in_agent_md_is_skipped_and_writes_nothing_outside() {
    let fx = Fx::new("name-tamper", "basic");
    let repo = fx.arg("repo");
    fx.put(
        "repo/agents/evil/AGENT.md",
        "---\nname: ../../escape\ndescription: Name tries to leave the repository\n---\nbody\n",
    );
    fx.put(
        "repo/agents/Bad_Name/AGENT.md",
        "---\nname: Bad_Name\ndescription: An uppercase name is refused by the parser\n---\nbody\n",
    );
    let (code, out, _) = run(&["agents", "ls", "--path", &repo]);
    assert_eq!(code, 0);
    let out = fx.normalised(&out);
    assert!(
        out.starts_with(
            "Failed to parse <root>/repo/agents/Bad_Name/AGENT.md: name must be lowercase"
        ),
        "{out}"
    );
    assert!(out.contains("Failed to parse <root>/repo/agents/evil/AGENT.md"), "{out}");
    assert!(out.contains("Found 3 agent(s) in <root>/repo"), "{out}");

    let (code, v, _) = cli(&["agents", "ls", "--path", &repo]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["discoveryWarnings"].as_array().unwrap().len(), 2);
    assert_eq!(names(&v["data"]["agents"]), ["helper", "planner", "reviewer"]);

    let before = tree_snapshot(&fx.root.join("home"));
    fx.sync(&["--project"]);
    assert!(!fx.root.join("escape.md").exists());
    assert!(!fx.repo.join(".claude/agents/evil.md").exists());
    assert_eq!(tree_snapshot(&fx.root.join("home")), before);
    let (_, v, _) = cli(&["agents", "sync", "--path", &repo, "--project", "--dry-run"]);
    assert_eq!(v["data"]["foundCount"], 3);
    assert_eq!(v["data"]["discoveryWarnings"].as_array().unwrap().len(), 2);
}

#[test]
fn roomodes_is_removed_as_a_whole_file_like_mcpm_even_with_foreign_modes() {
    let fx = Fx::new("roomodes", "basic");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    let roomodes = fx.repo.join(".roomodes");
    let text = std::fs::read_to_string(&roomodes).unwrap();
    assert!(text.contains("helper"), "{text}");
    let (code, out, err) = run(&["agents", "clean", "--project", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("  Removed .roomodes\n"), "{out}");
    assert!(!roomodes.exists());
}

#[test]
fn uninstall_one_agent_updates_the_lock_and_drops_the_source() {
    let fx = Fx::new("uninstall", "basic");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    let (code, out, err) = run(&["agents", "uninstall", "planner", "--project", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("  Removed .claude/agents/planner.md\n"), "{out}");
    assert!(out.contains("  Removed agents/planner\n"), "{out}");
    assert!(out.ends_with("Uninstalled 'planner' and removed 6 output file(s).\n"), "{out}");
    assert!(!fx.repo.join("agents/planner").exists());
    assert!(fx.repo.join("agents/helper/AGENT.md").is_file());
    assert!(fx.repo.join(".claude/agents/helper.md").is_file());
    assert!(!fx.repo.join(".claude/agents/planner.md").exists());
    let lock = fx.text("repo/mcpm-skills.lock");
    assert!(lock.contains("\"helper\"") && !lock.contains("\"planner\""));
}

#[test]
fn global_uninstall_cleans_home_outputs_and_updates_the_data_dir_lock() {
    let fx = Fx::new("uninstall-global", "basic");
    let repo = fx.arg("repo");
    fx.sync(&["--client", "claude-code"]);
    let (code, out, err) = run(&["agents", "uninstall", "planner", "--path", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        fx.normalised(&out),
        "  Removed <root>/home/.claude/agents/planner.md\n  Removed agents/planner\n\nUninstalled 'planner' and removed 1 output file(s).\n"
    );
    assert!(fx.home.join(".claude/agents/helper.md").is_file());
    assert!(!fx.home.join(".claude/agents/planner.md").exists());
    let lock = fx.text("data/mcpm-skills.lock");
    assert!(lock.contains("\"helper\"") && !lock.contains("\"planner\""));
}

#[test]
fn uninstall_refuses_unknown_invalid_and_escaping_names() {
    let fx = Fx::new("uninstall-bad", "basic");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    fx.put("outside/keep.txt", "keep");
    let before = fx.snapshot();
    for (name, needle) in [
        ("nope", "Agent 'nope' not found."),
        ("../outside", "Invalid agent name"),
        ("..", "Invalid agent name"),
        ("Bad_Name", "Invalid agent name"),
        ("a--b", "consecutive hyphens"),
    ] {
        let (code, out, err) = run(&["agents", "uninstall", name, "--project", "--path", &repo]);
        assert_eq!(code, 1, "{name}: {out}{err}");
        assert!(err.contains(needle), "{name}: {err}");
    }
    let absolute = fx.arg("outside");
    let (code, _, err) = run(&["agents", "uninstall", &absolute, "--project", "--path", &repo]);
    assert_eq!(code, 1);
    assert!(err.contains("Invalid agent name"), "{err}");
    assert_eq!(fx.snapshot(), before);
}

#[cfg(unix)]
#[test]
fn uninstall_refuses_an_agent_directory_that_links_outside_the_repository() {
    let fx = Fx::new("uninstall-link", "basic");
    let repo = fx.arg("repo");
    fx.put("outside/keep.txt", "keep");
    std::os::unix::fs::symlink(fx.root.join("outside"), fx.repo.join("agents/escape")).unwrap();
    let before = fx.snapshot();
    let (code, _, err) = run(&["agents", "uninstall", "escape", "--project", "--path", &repo]);
    assert_eq!(code, 1);
    assert!(err.contains("resolves outside the repository"), "{err}");
    assert_eq!(fx.snapshot(), before);
    assert_eq!(fx.text("outside/keep.txt"), "keep");
}

#[test]
fn usage_errors_exit_2_and_touch_nothing() {
    let fx = Fx::new("usage", "basic");
    let repo = fx.arg("repo");
    let before = fx.snapshot();
    let cases: Vec<Vec<&str>> = vec![
        vec!["agents"],
        vec!["agents", "nosuch"],
        vec!["agents", "ls", "extra", "--path", &repo],
        vec!["agents", "add", "--path", &repo],
        vec!["agents", "add", "a", "b", "--path", &repo],
        vec!["agents", "uninstall", "--path", &repo],
        vec!["agents", "status", "--project", "--path", &repo],
        vec!["agents", "clean", "--global", "--project", "--path", &repo],
        vec!["agents", "sync", "--global", "--project", "--path", &repo],
        vec!["agents", "sync", "--client"],
        vec!["agents", "ls", "--strict", "--path", &repo],
    ];
    for list in cases {
        let (code, out, err) = run(&list);
        assert_eq!(code, 2, "{list:?}: {out}{err}");
        assert!(err.starts_with("toolportctl: "), "{list:?}: {err}");
    }
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn plus_handlers_reject_missing_names_and_missing_repositories() {
    let fx = Fx::new("handlers", "empty");
    let nowhere = fx.arg("nowhere");
    for command in ["plus.agents.add", "plus.agents.uninstall"] {
        let err = crate::plus::dispatch(command, json!({"repo_path": fx.arg("repo")})).unwrap_err();
        assert_eq!(err, "name is required", "{command}");
    }
    for command in [
        "plus.agents.list",
        "plus.agents.lint",
        "plus.agents.audit",
        "plus.agents.diff",
        "plus.agents.status",
        "plus.agents.sync",
    ] {
        let err = crate::plus::dispatch(command, json!({"repo_path": nowhere})).unwrap_err();
        assert_eq!(err, "no skills repository found", "{command}");
    }
    let empty = crate::plus::dispatch("plus.agents.sync", json!({"repo_path": fx.arg("repo")}))
        .unwrap();
    assert_eq!(empty["foundCount"], 0);
    assert_eq!(empty["agents"], json!([]));
    assert!(!fx.root.join("data/mcpm-skills.lock").exists());
}

#[test]
fn ls_text_shows_the_wrapped_description_column_of_every_row() {
    let fx = Fx::new("ls-wrap", "wrap");
    let (code, out, err) = run(&["agents", "ls", "--path", &fx.arg("repo")]);
    assert_eq!(code, 0, "{err}");
    let widths: Vec<usize> = out
        .lines()
        .filter(|l| l.starts_with(['┏', '┃', '┡', '│', '└']))
        .map(|l| l.chars().count())
        .collect();
    assert!(widths.len() > 6);
    assert!(widths.windows(2).all(|w| w[0] == w[1]), "ragged table: {out}");
}
