use crate::plus::ctl::run_with;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

// Expected texts are verbatim mcpm output (rich, no terminal) with `mcpm` replaced by
// `toolportctl` in hints (D-042); paths are normalised to <root>.

const STATUS_OK: &str = r#"┏━━━━━━━┳━━━━━━━━━━━━━┳━━━━━━━━┓
┃ Skill ┃ Client      ┃ Status ┃
┡━━━━━━━╇━━━━━━━━━━━━━╇━━━━━━━━┩
│ alpha │ claude-code │ ok     │
│ beta  │ claude-code │ ok     │
│ gamma │ claude-code │ ok     │
└───────┴─────────────┴────────┘

All output files in sync."#;

const STATUS_DRIFT: &str = r#"┏━━━━━━━┳━━━━━━━━━━━━━┳━━━━━━━━━┓
┃ Skill ┃ Client      ┃ Status  ┃
┡━━━━━━━╇━━━━━━━━━━━━━╇━━━━━━━━━┩
│ alpha │ claude-code │ ok      │
│ beta  │ claude-code │ missing │
│ gamma │ claude-code │ ok      │
└───────┴─────────────┴─────────┘

Drift detected. Run 'toolportctl skills sync' to update."#;

const CLEAN_ALL: &str = r#"  Removed AGENTS.md
  Removed .mcpm/skills/alpha/SKILL.md
  Removed .mcpm/skills/beta/SKILL.md
  Removed .mcpm/skills/gamma/SKILL.md
  Removed .amazonq/rules/alpha.md
  Removed .amazonq/rules/beta.md
  Removed .amazonq/rules/gamma.md
  Removed .claude/skills/alpha/SKILL.md
  Removed .claude/skills/beta/SKILL.md
  Removed .clinerules/alpha.md
  Removed .clinerules/beta.md
  Removed .clinerules/gamma.md
  Removed .agents/skills/alpha/SKILL.md
  Removed .agents/skills/beta/SKILL.md
  Removed .agents/skills/gamma/SKILL.md
  Removed .continue/rules/alpha.md
  Removed .continue/rules/beta.md
  Removed .continue/rules/gamma.md
  Removed .cursor/rules/alpha/RULE.md
  Removed .cursor/rules/beta/RULE.md
  Removed .cursor/rules/gamma/RULE.md
  Removed .gemini/skills/alpha/SKILL.md
  Removed .gemini/skills/beta/SKILL.md
  Removed .gemini/skills/gamma/SKILL.md
  Removed .goose/skills/alpha/SKILL.md
  Removed .goose/skills/beta/SKILL.md
  Removed .aiassistant/rules/alpha.md
  Removed .aiassistant/rules/beta.md
  Removed .aiassistant/rules/gamma.md
  Removed .roo/rules/alpha.md
  Removed .roo/rules/beta.md
  Removed .roo/rules/gamma.md
  Removed .trae/rules/alpha.md
  Removed .trae/rules/beta.md
  Removed .trae/rules/gamma.md
  Removed .windsurf/rules/alpha.md
  Removed .windsurf/rules/beta.md
  Removed .windsurf/rules/gamma.md
  Removed .rules
  Removed mcpm-skills.lock

Cleaned 40 file(s)."#;

const UNINSTALL_BETA: &str = r#"  Removed AGENTS.md
  Removed .mcpm/skills/beta/SKILL.md
  Removed .amazonq/rules/beta.md
  Removed .claude/skills/beta/SKILL.md
  Removed .clinerules/beta.md
  Removed .agents/skills/beta/SKILL.md
  Removed .continue/rules/beta.md
  Removed .cursor/rules/beta/RULE.md
  Removed .gemini/skills/beta/SKILL.md
  Removed .goose/skills/beta/SKILL.md
  Removed .aiassistant/rules/beta.md
  Removed .roo/rules/beta.md
  Removed .trae/rules/beta.md
  Removed .windsurf/rules/beta.md
  Removed .rules
  Removed skills/beta

Uninstalled 'beta' and removed 15 output file(s)."#;

const UNINSTALL_GAMMA: &str = r#"  Removed .mcpm/skills/gamma/SKILL.md
  Removed .amazonq/rules/gamma.md
  Removed .clinerules/gamma.md
  Removed .agents/skills/gamma/SKILL.md
  Removed .continue/rules/gamma.md
  Removed .cursor/rules/gamma/RULE.md
  Removed .gemini/skills/gamma/SKILL.md
  Removed .aiassistant/rules/gamma.md
  Removed .roo/rules/gamma.md
  Removed .trae/rules/gamma.md
  Removed .windsurf/rules/gamma.md
  Removed rules/gamma

Uninstalled 'gamma' and removed 11 output file(s)."#;

struct Fx {
    base: DataDirFx,
    root: PathBuf,
    home: PathBuf,
    repo: PathBuf,
}

impl std::ops::Deref for Fx {
    type Target = DataDirFx;
    fn deref(&self) -> &DataDirFx {
        &self.base
    }
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = DataDirFx::with_data_subdir("ctl-skills-state", tag, "data");
        let root = base.dir.canonicalize().unwrap();
        let (home, repo) = (root.join("home"), root.join("repo"));
        std::fs::create_dir_all(&home).unwrap();
        let fx = Self {
            base,
            root,
            home,
            repo,
        };
        fx.put("repo/mcpm-skills.yaml", "name: sample\n");
        fx.put(
            "repo/skills/alpha/SKILL.md",
            "---\nname: alpha\ndescription: A synthetic skill\n---\nAlpha body\n",
        );
        fx.put(
            "repo/skills/beta/SKILL.md",
            "---\nname: beta\ndescription: Another synthetic skill\n---\nBeta body\n",
        );
        fx.put(
            "repo/rules/gamma/SKILL.md",
            "---\nname: gamma\ndescription: A synthetic rule\nactivation: always\n---\nGamma body\n",
        );
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(fx.home.clone()));
        fx
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

    fn files(&self, rel: &str) -> Vec<String> {
        tree_snapshot(&self.root.join(rel))
            .into_iter()
            .filter(|(_, content)| content.is_some())
            .map(|(path, _)| path)
            .collect()
    }

    fn normalised(&self, text: &str) -> String {
        text.replace(self.root.to_string_lossy().as_ref(), "<root>")
    }

    fn sync(&self, extra: &[&str]) {
        let repo = self.arg("repo");
        let mut list = vec!["skills", "sync", "--repo", &repo];
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

fn rel_list(value: &Value, base: &str) -> Vec<String> {
    strs(value)
        .iter()
        .map(|p| {
            p.strip_prefix(base)
                .unwrap_or(p)
                .trim_start_matches('/')
                .to_string()
        })
        .collect()
}

const AFTER_CLEAN: [&str; 6] = [
    ".claude/rules/gamma.md",
    ".goose/rules/gamma.md",
    "mcpm-skills.yaml",
    "rules/gamma/SKILL.md",
    "skills/alpha/SKILL.md",
    "skills/beta/SKILL.md",
];

#[test]
fn status_without_a_lockfile_says_so_and_strict_fails() {
    let fx = Fx::new("status-none");
    let repo = fx.arg("repo");
    let before = fx.snapshot();
    let (code, out, err) = run(&["skills", "status", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "No lockfile found. Run 'toolportctl skills sync' first.\n"
    );
    let (code, out, _) = run(&["skills", "status", "--repo", &repo, "--strict"]);
    assert_eq!(code, 1);
    assert_eq!(
        out,
        "No lockfile found. Run 'toolportctl skills sync' first.\n"
    );
    let (code, v, _) = cli(&["skills", "status", "--repo", &repo]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["lockfilePresent"], false);
    assert_eq!(v["data"]["drift"], false);
    assert_eq!(v["data"]["outputs"], json!([]));
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn project_status_text_matches_mcpm_in_sync_and_with_drift() {
    let fx = Fx::new("status-project");
    let repo = fx.arg("repo");
    fx.sync(&["--project", "--client", "claude-code"]);

    let (code, out, err) = run(&["skills", "status", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, format!("{STATUS_OK}\n"));
    let (code, out, _) = run(&["skills", "status", "--repo", &repo, "--strict"]);
    assert_eq!(code, 0);
    assert_eq!(out, format!("{STATUS_OK}\n"));

    std::fs::remove_file(fx.repo.join(".claude/skills/beta/SKILL.md")).unwrap();
    let before = fx.snapshot();
    let (code, out, _) = run(&["skills", "status", "--repo", &repo]);
    assert_eq!(code, 0);
    assert_eq!(out, format!("{STATUS_DRIFT}\n"));
    let (code, out, _) = run(&["skills", "status", "--repo", &repo, "--strict"]);
    assert_eq!(code, 1);
    assert_eq!(out, format!("{STATUS_DRIFT}\n"));

    let (code, v, _) = cli(&["skills", "status", "--repo", &repo]);
    assert_eq!(code, 0);
    let data = &v["data"];
    assert_eq!(data["drift"], true);
    assert_eq!(data["lockedCount"], 3);
    assert_eq!(
        fx.normalised(data["outputRoot"].as_str().unwrap()),
        "<root>/repo"
    );
    assert_eq!(
        data["outputs"],
        json!([
            {"name": "alpha", "client": "claude-code", "present": true},
            {"name": "beta", "client": "claude-code", "present": false},
            {"name": "gamma", "client": "claude-code", "present": true},
        ])
    );
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn status_reads_the_user_level_lock_and_checks_the_home_outputs() {
    let fx = Fx::new("status-global");
    let repo = fx.arg("repo");
    fx.sync(&["--client", "claude-code"]);
    assert!(fx.home.join(".claude/skills/alpha/SKILL.md").is_file());
    assert!(!fx.repo.join("mcpm-skills.lock").exists());

    let (code, out, err) = run(&["skills", "status", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, format!("{STATUS_OK}\n"));

    std::fs::remove_file(fx.home.join(".claude/skills/alpha/SKILL.md")).unwrap();
    let (code, v, _) = cli(&["skills", "status", "--repo", &repo, "--strict"]);
    assert_eq!(code, 1);
    assert_eq!(v["data"]["drift"], true);
    assert_eq!(
        fx.normalised(v["data"]["outputRoot"].as_str().unwrap()),
        "<root>/home"
    );
    let missing: Vec<&Value> = v["data"]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["present"] == false)
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["name"], "alpha");
}

#[test]
fn status_client_filter_limits_the_rows() {
    let fx = Fx::new("status-client");
    let repo = fx.arg("repo");
    fx.sync(&["--project", "--client", "claude-code", "--client", "cursor"]);
    let (_, v, _) = cli(&["skills", "status", "--repo", &repo]);
    assert_eq!(v["data"]["outputs"].as_array().unwrap().len(), 6);
    let (_, v, _) = cli(&["skills", "status", "--repo", &repo, "--client", "cursor"]);
    let rows = v["data"]["outputs"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row["client"] == "cursor"));
    assert_eq!(v["data"]["targetedClients"], json!(["cursor"]));
}

#[test]
fn project_clean_text_matches_mcpm_and_keeps_what_mcpm_keeps() {
    let fx = Fx::new("clean-project");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    assert_eq!(fx.files("repo").len(), 46);

    let (code, out, err) = run(&["skills", "clean", "--project", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, format!("{CLEAN_ALL}\n"));
    assert_eq!(fx.files("repo"), AFTER_CLEAN);

    let (code, out, _) = run(&["skills", "clean", "--project", "--repo", &repo]);
    assert_eq!(code, 0);
    assert_eq!(out, "No lockfile found. Nothing to clean.\n");
}

#[test]
fn clean_for_one_client_keeps_the_lockfile_like_mcpm() {
    let fx = Fx::new("clean-client");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    let (code, out, err) = run(&[
        "skills",
        "clean",
        "--project",
        "--repo",
        &repo,
        "--client",
        "claude-code",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "  Removed .claude/skills/alpha/SKILL.md\n  Removed .claude/skills/beta/SKILL.md\n\nCleaned 2 file(s).\n"
    );
    assert!(fx.repo.join("mcpm-skills.lock").is_file());
    assert!(fx.repo.join(".cursor/rules/alpha/RULE.md").is_file());

    let (code, out, _) = run(&[
        "skills",
        "clean",
        "--project",
        "--repo",
        &repo,
        "--client",
        "nosuch",
    ]);
    assert_eq!(code, 0);
    assert_eq!(out, "No managed files found to clean.\n");
}

#[test]
fn clean_dry_run_reports_the_real_removals_and_writes_nothing() {
    let fx = Fx::new("clean-dry");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    let before = fx.snapshot();

    let (code, v, err) = cli(&["skills", "clean", "--project", "--repo", &repo, "--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(v["data"]["lockfileRemoved"], true);
    assert_eq!(fx.snapshot(), before);
    let planned = rel_list(&v["data"]["removed"], &fx.arg("repo"));

    let (code, out, _) = run(&["skills", "clean", "--project", "--repo", &repo, "--dry-run"]);
    assert_eq!(code, 0);
    let expected = CLEAN_ALL
        .replace("  Removed ", "  Would remove ")
        .replace("Cleaned 40 file(s).", "Would clean 40 file(s).");
    assert_eq!(out, format!("{expected}\n"));
    assert_eq!(fx.snapshot(), before);

    let (_, v, _) = cli(&["skills", "clean", "--project", "--repo", &repo]);
    assert_eq!(v["data"]["dryRun"], false);
    assert_eq!(rel_list(&v["data"]["removed"], &fx.arg("repo")), planned);
    assert_ne!(fx.snapshot(), before);
}

#[test]
fn global_clean_removes_home_outputs_and_the_data_dir_lock() {
    let fx = Fx::new("clean-global");
    let repo = fx.arg("repo");
    fx.sync(&["--client", "claude-code"]);
    let lock = fx.root.join("data/mcpm-skills.lock");
    assert!(lock.is_file());
    let before = fx.snapshot();

    let (code, out, _) = run(&["skills", "clean", "--repo", &repo, "--dry-run"]);
    assert_eq!(code, 0);
    assert_eq!(
        fx.normalised(&out),
        "  Would remove <root>/home/.claude/skills/alpha/SKILL.md\n  Would remove <root>/home/.claude/skills/beta/SKILL.md\n  Would remove mcpm-skills.lock\n\nWould clean 3 file(s).\n"
    );
    assert_eq!(fx.snapshot(), before);

    let (code, out, err) = run(&["skills", "clean", "--global", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        fx.normalised(&out),
        "  Removed <root>/home/.claude/skills/alpha/SKILL.md\n  Removed <root>/home/.claude/skills/beta/SKILL.md\n  Removed mcpm-skills.lock\n\nCleaned 3 file(s).\n"
    );
    assert!(!lock.exists());
    assert!(fx.files("home/.claude/skills").is_empty());
    assert!(fx.home.join(".claude/rules/gamma.md").is_file());
    assert!(fx.repo.join("skills/alpha/SKILL.md").is_file());
}

#[test]
fn clean_ignores_tampered_lock_names_and_never_leaves_the_root() {
    let fx = Fx::new("clean-tamper");
    let repo = fx.arg("repo");
    let absolute = fx.root.join("victim-abs").to_string_lossy().into_owned();
    let names = [
        "../../../victim".to_string(),
        "x/../../../../victim".to_string(),
        absolute,
    ];
    let victims = [
        "victim.md",
        "victim.toml",
        "victim.agent.md",
        "victim/SKILL.md",
        "victim/RULE.md",
        "victim-abs.md",
        "victim-abs/SKILL.md",
        "victim-abs/RULE.md",
    ];
    for victim in victims {
        fx.put(victim, "keep");
    }
    for dir in [
        ".windsurf/rules/x",
        ".claude/rules/x",
        ".claude/skills/x",
        ".cursor/rules/x",
    ] {
        std::fs::create_dir_all(fx.repo.join(dir)).unwrap();
    }
    let mut skills = serde_json::Map::new();
    for name in &names {
        skills.insert(
            name.clone(),
            json!({"source": "local", "version": null, "hash": "sha256:0", "clients_synced": [],
                   "warnings": [], "output_files": {}, "hooks_installed": {}}),
        );
    }
    fx.put(
        "repo/mcpm-skills.lock",
        &serde_json::to_string_pretty(&json!({
            "version": 1, "synced_at": "t", "scope": "project", "output_root": "",
            "skills": skills, "rules": skills, "agents": {}, "styles": {}, "active_styles": {},
        }))
        .unwrap(),
    );
    let before = fx.snapshot();

    let (code, v, err) = cli(&["skills", "clean", "--project", "--repo", &repo, "--dry-run"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["removed"], json!([]));
    assert_eq!(v["data"]["managed"], json!([]));
    let mut ignored: Vec<&str> = strs(&v["data"]["ignored"]);
    ignored.sort_unstable();
    ignored.dedup();
    let mut expected: Vec<&str> = names.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(ignored, expected);
    assert_eq!(fx.snapshot(), before);

    let (code, out, _) = run(&["skills", "clean", "--project", "--repo", &repo]);
    assert_eq!(code, 0);
    assert!(out.starts_with("No managed skills in lockfile.\n"), "{out}");
    assert!(
        out.contains("Ignored lock entry with invalid name: ../../../victim\n"),
        "{out}"
    );
    assert_eq!(
        fx.snapshot(),
        before,
        "nothing outside or inside the root was touched"
    );
    for victim in victims {
        assert_eq!(fx.text(victim), "keep", "{victim}");
    }
}

#[test]
fn clean_still_cleans_valid_names_beside_tampered_ones() {
    let fx = Fx::new("clean-mixed");
    let repo = fx.arg("repo");
    fx.sync(&["--project", "--client", "claude-code"]);
    fx.put("victim.md", "keep");
    std::fs::create_dir_all(fx.repo.join(".windsurf/rules")).unwrap();
    let lock = fx.text("repo/mcpm-skills.lock");
    let tampered = lock.replacen(
        "\"skills\": {",
        "\"skills\": {\n    \"../../victim\": {\"source\": \"local\", \"version\": null, \"hash\": \"sha256:0\", \"clients_synced\": [], \"warnings\": [], \"output_files\": {}, \"hooks_installed\": {}},",
        1,
    );
    assert_ne!(lock, tampered);
    fx.put("repo/mcpm-skills.lock", &tampered);

    let (code, out, err) = run(&["skills", "clean", "--project", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("  Removed .claude/skills/alpha/SKILL.md\n"),
        "{out}"
    );
    assert!(
        out.contains("Ignored lock entry with invalid name: ../../victim"),
        "{out}"
    );
    assert_eq!(fx.text("victim.md"), "keep");
    assert!(!fx.repo.join("mcpm-skills.lock").exists());
}

#[test]
fn uninstall_text_matches_mcpm_for_a_skill_and_a_rule() {
    let fx = Fx::new("uninstall");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);

    let (code, out, err) = run(&["skills", "uninstall", "beta", "--project", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, format!("{UNINSTALL_BETA}\n"));
    assert!(!fx.repo.join("skills/beta").exists());
    assert!(fx.repo.join("skills/alpha/SKILL.md").is_file());
    assert!(fx.repo.join(".claude/skills/alpha/SKILL.md").is_file());
    assert!(!fx.repo.join(".claude/skills/beta").exists());
    let lock = fx.text("repo/mcpm-skills.lock");
    assert!(lock.contains("\"alpha\"") && lock.contains("\"gamma\""));
    assert!(!lock.contains("\"beta\""));

    let (code, out, err) = run(&["skills", "uninstall", "gamma", "--project", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, format!("{UNINSTALL_GAMMA}\n"));
    assert!(!fx.repo.join("rules/gamma").exists());
    assert!(!fx.text("repo/mcpm-skills.lock").contains("\"gamma\""));
}

#[test]
fn uninstall_dry_run_reports_the_real_removals_and_writes_nothing() {
    let fx = Fx::new("uninstall-dry");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    let before = fx.snapshot();

    let (code, v, err) = cli(&[
        "skills",
        "uninstall",
        "beta",
        "--project",
        "--repo",
        &repo,
        "--dry-run",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(v["data"]["lockUpdated"], true);
    assert_eq!(fx.snapshot(), before);
    let planned = rel_list(&v["data"]["outputs"], &fx.arg("repo"));
    assert_eq!(planned.len(), 15);

    let (code, out, _) = run(&[
        "skills",
        "uninstall",
        "beta",
        "--project",
        "--repo",
        &repo,
        "--dry-run",
    ]);
    assert_eq!(code, 0);
    let expected = UNINSTALL_BETA
        .replace("  Removed ", "  Would remove ")
        .replace(
            "Uninstalled 'beta' and removed 15 output file(s).",
            "Would uninstall 'beta' and remove 15 output file(s).",
        );
    assert_eq!(out, format!("{expected}\n"));
    assert_eq!(fx.snapshot(), before);

    let (_, v, _) = cli(&["skills", "uninstall", "beta", "--project", "--repo", &repo]);
    assert_eq!(rel_list(&v["data"]["outputs"], &fx.arg("repo")), planned);
}

#[test]
fn global_uninstall_cleans_home_outputs_and_updates_the_data_dir_lock() {
    let fx = Fx::new("uninstall-global");
    let repo = fx.arg("repo");
    fx.sync(&["--client", "claude-code"]);
    let (code, out, err) = run(&["skills", "uninstall", "beta", "--repo", &repo]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        fx.normalised(&out),
        "  Removed <root>/home/.claude/skills/beta/SKILL.md\n  Removed skills/beta\n\nUninstalled 'beta' and removed 1 output file(s).\n"
    );
    assert!(fx.home.join(".claude/skills/alpha/SKILL.md").is_file());
    assert!(!fx.home.join(".claude/skills/beta").exists());
    assert!(!fx.repo.join("skills/beta").exists());
    let lock = fx.text("data/mcpm-skills.lock");
    assert!(lock.contains("\"alpha\"") && !lock.contains("\"beta\""));
}

#[test]
fn uninstall_refuses_unknown_invalid_and_escaping_names() {
    let fx = Fx::new("uninstall-bad");
    let repo = fx.arg("repo");
    fx.sync(&["--project"]);
    fx.put("outside/keep.txt", "keep");
    let before = fx.snapshot();
    for (name, needle) in [
        ("nope", "Skill 'nope' not found."),
        ("../outside", "Invalid skill name"),
        ("..", "Invalid skill name"),
        ("Bad_Name", "Invalid skill name"),
        ("a--b", "consecutive hyphens"),
    ] {
        let (code, out, err) = run(&["skills", "uninstall", name, "--project", "--repo", &repo]);
        assert_eq!(code, 1, "{name}: {out}{err}");
        assert!(err.contains(needle), "{name}: {err}");
    }
    let absolute = fx.arg("outside");
    let (code, _, err) = run(&[
        "skills",
        "uninstall",
        &absolute,
        "--project",
        "--repo",
        &repo,
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("Invalid skill name"), "{err}");
    assert_eq!(fx.snapshot(), before);
}

#[cfg(unix)]
#[test]
fn uninstall_refuses_a_skill_directory_that_links_outside_the_repository() {
    let fx = Fx::new("uninstall-link");
    let repo = fx.arg("repo");
    fx.put("outside/keep.txt", "keep");
    std::os::unix::fs::symlink(fx.root.join("outside"), fx.repo.join("skills/escape")).unwrap();
    let before = fx.snapshot();
    let (code, _, err) = run(&[
        "skills",
        "uninstall",
        "escape",
        "--project",
        "--repo",
        &repo,
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("resolves outside the repository"), "{err}");
    assert_eq!(fx.snapshot(), before);
    assert_eq!(fx.text("outside/keep.txt"), "keep");
}

#[test]
fn resolve_without_collisions_and_without_skills_says_so() {
    let fx = Fx::new("resolve-none");
    let repo = fx.arg("repo");
    let (code, out, err) = run(&[
        "skills",
        "resolve",
        "--repo",
        &repo,
        "--client",
        "claude-code",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "No collisions found.\n");
    fx.put("empty/mcpm-skills.yaml", "name: empty\n");
    let (code, out, _) = run(&["skills", "resolve", "--repo", &fx.arg("empty")]);
    assert_eq!(code, 0);
    assert_eq!(out, "No skills found in repository.\n");
    let (_, v, _) = cli(&["skills", "resolve", "--repo", &fx.arg("empty")]);
    assert_eq!(v["data"]["skillCount"], 0);
    assert_eq!(v["data"]["collisions"], json!([]));
}

#[test]
fn global_resolve_text_matches_mcpm_for_warn_dry_run_and_migrate() {
    let fx = Fx::new("resolve-global");
    let repo = fx.arg("repo");
    fx.put("home/.claude/commands/alpha.md", "hand written\n");
    let warn = "Found 1 collision(s).\n  ! collision: alpha (claude-code) — existing file at <root>/home/.claude/commands/alpha.md shadows synced skill\nLeft 1 file(s) in place.\n";
    let before = fx.snapshot();
    for extra in [
        vec![],
        vec!["--no-migrate"],
        vec!["--no-migrate", "--dry-run"],
    ] {
        let mut list = vec![
            "skills",
            "resolve",
            "--repo",
            &repo,
            "--client",
            "claude-code",
        ];
        list.extend(extra.iter().copied());
        let (code, out, err) = run(&list);
        assert_eq!(code, 0, "{list:?}: {err}");
        assert_eq!(fx.normalised(&out), warn, "{list:?}");
        assert_eq!(fx.snapshot(), before);
    }

    let (code, out, err) = run(&[
        "skills",
        "resolve",
        "--repo",
        &repo,
        "--client",
        "claude-code",
        "--migrate",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        fx.normalised(&out),
        "Found 1 collision(s).\n  (dry run) would replace <root>/home/.claude/commands/alpha.md (skill: alpha)\n"
    );
    assert_eq!(fx.snapshot(), before);

    let (code, out, err) = run(&[
        "skills",
        "resolve",
        "--repo",
        &repo,
        "--client",
        "claude-code",
        "--migrate",
    ]);
    assert_eq!(code, 0, "{err}");
    let out = fx.normalised(&out);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4, "{out}");
    assert_eq!(lines[0], "Found 1 collision(s).");
    let prefix = "  Replaced <root>/home/.claude/commands/alpha.md → backup at <root>/home/.mcpm-backups/.claude/commands/alpha.md.";
    assert!(lines[1].starts_with(prefix), "{out}");
    assert!(lines[1].ends_with('Z'), "{out}");
    assert_eq!(lines[2], "");
    assert_eq!(
        lines[3],
        "Replaced 1 file(s) (backed up under <root>/home/.mcpm-backups)."
    );
    assert!(!fx.home.join(".claude/commands/alpha.md").exists());
    let backups = fx.files("home/.mcpm-backups");
    assert_eq!(backups.len(), 2, "{backups:?}");
    let stamped = backups.iter().find(|p| p.contains("alpha.md.")).unwrap();
    assert_eq!(
        fx.text(&format!("home/.mcpm-backups/{stamped}")),
        "hand written\n"
    );
    assert!(fx
        .text("home/.mcpm-backups/INDEX.json")
        .contains("collision-with-synced-skill"));

    let (_, out, _) = run(&[
        "skills",
        "resolve",
        "--repo",
        &repo,
        "--client",
        "claude-code",
    ]);
    assert_eq!(out, "No collisions found.\n");
}

#[test]
fn project_resolve_works_inside_the_repository_and_reports_json() {
    let fx = Fx::new("resolve-project");
    let repo = fx.arg("repo");
    fx.put("repo/.claude/commands/beta.md", "hand written\n");
    let (code, v, err) = cli(&[
        "skills",
        "resolve",
        "--project",
        "--repo",
        &repo,
        "--migrate",
    ]);
    assert_eq!(code, 0, "{err}");
    let data = &v["data"];
    assert_eq!(data["scope"], "project");
    assert_eq!(data["migrate"], true);
    assert_eq!(data["replaced"], 1);
    assert_eq!(data["kept"], 0);
    assert_eq!(data["skillCount"], 3);
    let collision = &data["collisions"][0];
    assert_eq!(collision["skill"], "beta");
    assert_eq!(collision["client"], "claude-code");
    assert_eq!(collision["action"], "replaced");
    assert_eq!(
        fx.normalised(collision["collisionPath"].as_str().unwrap()),
        "<root>/repo/.claude/commands/beta.md"
    );
    assert!(fx
        .normalised(collision["backupPath"].as_str().unwrap())
        .starts_with("<root>/repo/.mcpm-backups/.claude/commands/beta.md."));
    assert!(!fx.repo.join(".claude/commands/beta.md").exists());
}

#[test]
fn state_commands_reject_bad_arguments_with_exit_two() {
    let fx = Fx::new("usage");
    let repo = fx.arg("repo");
    for list in [
        vec!["skills", "status", "--bogus"],
        vec!["skills", "status", "extra"],
        vec!["skills", "status", "--repo"],
        vec!["skills", "status", "--dry-run"],
        vec!["skills", "clean", "extra"],
        vec!["skills", "clean", "--strict"],
        vec!["skills", "clean", "--global", "--project"],
        vec!["skills", "clean", "--dry-run=yes"],
        vec!["skills", "uninstall"],
        vec!["skills", "uninstall", "a", "b"],
        vec!["skills", "uninstall", "a", "--client", "x"],
        vec!["skills", "resolve", "--migrate", "--no-migrate"],
        vec!["skills", "resolve", "stray"],
        vec!["skills", "sync", "--global", "--project"],
    ] {
        let (code, _, err) = cli(&list);
        assert_eq!(code, 2, "{list:?}: {err}");
    }
    assert_eq!(
        cli(&["skills", "sync", "--global", "--dry-run", "--repo", &repo]).0,
        0
    );
}

#[test]
fn plus_handlers_serve_the_same_data_as_the_cli() {
    let fx = Fx::new("handlers");
    let repo = fx.arg("repo");
    fx.sync(&["--project", "--client", "claude-code"]);
    let (_, v, _) = cli(&["skills", "status", "--repo", &repo]);
    let ipc = crate::plus::dispatch("plus.skills.status", json!({"repo_path": repo})).unwrap();
    assert_eq!(ipc, v["data"]);
    let tool =
        crate::plus::selfmcp::call_tool("skills_status", &json!({"repo_path": repo})).unwrap();
    assert_eq!(tool, ipc);

    let (_, v, _) = cli(&["skills", "clean", "--project", "--repo", &repo, "--dry-run"]);
    let ipc = crate::plus::dispatch(
        "plus.skills.clean",
        json!({"repo_path": repo, "global_mode": false, "dry_run": true}),
    )
    .unwrap();
    assert_eq!(ipc, v["data"]);

    let (_, v, _) = cli(&[
        "skills",
        "uninstall",
        "alpha",
        "--project",
        "--repo",
        &repo,
        "--dry-run",
    ]);
    let ipc = crate::plus::dispatch(
        "plus.skills.uninstall",
        json!({"repo_path": repo, "name": "alpha", "global_mode": false, "dry_run": true}),
    )
    .unwrap();
    assert_eq!(ipc, v["data"]);
    assert!(crate::plus::dispatch("plus.skills.uninstall", json!({"repo_path": repo})).is_err());

    let (_, v, _) = cli(&["skills", "resolve", "--project", "--repo", &repo]);
    let ipc = crate::plus::dispatch(
        "plus.skills.resolve",
        json!({"repo_path": repo, "global_mode": false}),
    )
    .unwrap();
    assert_eq!(ipc, v["data"]);
}
