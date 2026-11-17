use super::api::{self, Args};
use super::files::skill_files;
use super::lint::{lint_outputs, Consequence};
use super::parser::{discover_skills, Skill};
use super::transpiler::{Capabilities, TranspileResult, Transpiler, TranspilerRegistry};
use crate::fsx::{enter, Paths, PathsGuard};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use toolport_plus_api::memory::MemoryHost;

struct World {
    base: PathBuf,
    _paths: PathsGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("skills-checks-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        for dir in ["repo", "home", "data"] {
            fs::create_dir_all(base.join(dir)).unwrap();
        }
        let paths = enter(Paths {
            home: Some(base.join("home")),
            data_dir: Some(base.join("data")),
        });
        let world = World {
            base,
            _paths: paths,
        };
        world.put(
            "skills/nacht/SKILL.md",
            "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\nSee [setup](SETUP.md), [guide](references/guide.md) and [old](references/old.md).\n",
        );
        world.put("skills/nacht/SETUP.md", "# setup\n");
        world.put("skills/nacht/references/guide.md", "guide\n");
        world.put("skills/nacht/templates/x/.gitignore", "*.tmp\n");
        world
    }

    fn repo(&self) -> PathBuf {
        self.base.join("repo")
    }

    fn installed(&self) -> PathBuf {
        self.base.join("home/.claude/skills/nacht")
    }

    fn put(&self, rel: &str, body: &str) {
        let path = self.repo().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn args(&self) -> Args {
        Args {
            repo: Some(self.repo()),
            name: "nacht".into(),
            ..Args::default()
        }
    }

    fn sync(&self, apply_held: bool) {
        let args = Args {
            apply_held,
            ..self.args()
        };
        api::sync(&args, Some(&MemoryHost::default())).unwrap();
    }

    fn files(&self) -> Value {
        skill_files(&self.args()).unwrap()
    }

    fn lint(&self) -> Vec<Value> {
        let data = api::lint(&self.args()).unwrap();
        data["messages"].as_array().unwrap().clone()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn row<'a>(rows: &'a [Value], code: &str) -> &'a Value {
    rows.iter()
        .find(|m| m["code"] == code)
        .unwrap_or_else(|| panic!("no {code} row in {rows:?}"))
}

fn status_of(data: &Value, path: &str) -> String {
    data["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no {path} in {data}"))["status"]
        .as_str()
        .unwrap()
        .to_string()
}

fn make_unreadable(w: &World) {
    w.put(
        "skills/broken/SKILL.md",
        "---\nname: broken\ndescription: ok\n  bad: [unclosed\n---\n\nbody\n",
    );
}

#[test]
fn test01_the_description_limits_ride_on_the_lint_result_and_on_the_row() {
    let w = World::new("limits");
    let long = "x".repeat(1200);
    w.put(
        "skills/nacht/SKILL.md",
        &format!("---\nname: nacht\ndescription: \"{long}\"\n---\n\n# Nacht\n"),
    );
    let data = api::lint(&w.args()).unwrap();
    assert_eq!(data["limits"]["specDescription"], 1024);
    assert_eq!(data["limits"]["description"], 1536);
    let rows = data["messages"].as_array().unwrap();
    let over = row(rows, "description-over-spec");
    assert_eq!(over["level"], "warning");
    assert_eq!(over["consequence"], "over-spec");
    assert_eq!(over["meter"]["length"], 1200);
    assert_eq!(over["meter"]["specLimit"], 1024);
    assert_eq!(over["meter"]["limit"], 1536);
}

#[test]
fn test02_a_description_of_1536_characters_parses_and_one_more_is_cut() {
    let w = World::new("cut");
    let edge = "x".repeat(1536);
    w.put(
        "skills/nacht/SKILL.md",
        &format!("---\nname: nacht\ndescription: \"{edge}\"\n---\n\n# Nacht\n"),
    );
    assert!(row(&w.lint(), "description-over-spec")["meter"]["length"] == 1536);
    let over = "x".repeat(1537);
    w.put(
        "skills/nacht/SKILL.md",
        &format!("---\nname: nacht\ndescription: \"{over}\"\n---\n\n# Nacht\n"),
    );
    let data = api::lint(&w.args()).unwrap();
    assert!(data["errors"].as_u64().unwrap() >= 1);
    let rows = data["messages"].as_array().unwrap();
    let cut = row(rows, "description-over-limit");
    assert_eq!(cut["consequence"], "cut");
    assert_eq!(cut["level"], "error");
    assert_eq!(cut["meter"]["length"], 1537);
}

#[test]
fn test03_a_lock_that_syncs_agents_md_turns_the_spec_warning_into_an_error() {
    let w = World::new("others");
    let long = "x".repeat(1100);
    w.put(
        "skills/nacht/SKILL.md",
        &format!("---\nname: nacht\ndescription: \"{long}\"\n---\n\n# Nacht\n"),
    );
    let project = Args {
        global: false,
        ..w.args()
    };
    let level = |args: &Args| {
        let data = api::lint(args).unwrap();
        row(
            data["messages"].as_array().unwrap(),
            "description-over-spec",
        )["level"]
            .clone()
    };
    assert_eq!(level(&project), "warning");
    let sync = Args {
        clients: Some(vec!["claude-code".into(), "agents-md".into()]),
        ..project.clone()
    };
    api::sync(&sync, Some(&MemoryHost::default())).unwrap();
    assert_eq!(level(&project), "error");
}

#[test]
fn test04_links_carry_their_line_and_target_and_files_not_copied_their_reason() {
    let w = World::new("links");
    w.put("skills/nacht/.DS_Store", "x");
    let rows = w.lint();
    let dead = row(&rows, "link-missing");
    assert_eq!(dead["consequence"], "dead-link");
    assert_eq!(dead["target"], "references/old.md");
    assert_eq!(dead["line"], 6);
    assert_eq!(dead["file"], "SKILL.md");
    let skipped = row(&rows, "file-not-copied");
    assert_eq!(skipped["level"], "info");
    assert_eq!(skipped["file"], ".DS_Store");
    assert!(skipped["reason"].as_str().unwrap().contains("deny-list"));
}

#[test]
fn test05_a_link_out_of_the_folder_is_its_own_row() {
    let w = World::new("outside");
    w.put("shared/rubric.md", "r\n");
    w.put(
        "skills/nacht/SKILL.md",
        "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\nSee [r](../../shared/rubric.md).\n",
    );
    let rows = w.lint();
    let outside = row(&rows, "link-outside-folder");
    assert_eq!(outside["target"], "../../shared/rubric.md");
    assert_eq!(outside["consequence"], "dead-link");
    assert_eq!(outside["fixable"], "move-link");
}

#[test]
fn test06_unparsable_frontmatter_names_the_line_and_hides_the_skill() {
    let w = World::new("yaml");
    make_unreadable(&w);
    let data = api::lint(&Args {
        names: None,
        ..w.args()
    })
    .unwrap();
    let rows = data["messages"].as_array().unwrap();
    let broken = row(rows, "frontmatter-invalid-yaml");
    assert_eq!(broken["name"], "broken");
    assert_eq!(broken["consequence"], "hidden");
    assert_eq!(broken["level"], "error");
    let line = broken["line"].as_u64().unwrap();
    assert!((3..=4).contains(&line), "line {line}");
}

#[test]
fn test07_a_missing_frontmatter_and_a_missing_name_are_hidden_rows() {
    let w = World::new("hidden");
    w.put("skills/plain/SKILL.md", "no frontmatter\n");
    w.put(
        "skills/noname/SKILL.md",
        "---\ndescription: A skill without a name for the test\n---\n\nbody\n",
    );
    let rows = w.lint();
    assert_eq!(row(&rows, "frontmatter-missing")["line"], 1);
    assert_eq!(row(&rows, "frontmatter-name")["consequence"], "hidden");
}

/// The emitter before the quoting fix: the description spliced between quotes as it is.
struct LegacyEmitter;

impl Transpiler for LegacyEmitter {
    fn client_key(&self) -> &str {
        "claude-code"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::PER_FILE
    }

    fn transpile(&self, skill: &Skill, root: &Path) -> Result<TranspileResult, String> {
        Ok(TranspileResult {
            output_path: self.get_output_path(skill, root),
            content: format!(
                "---\nname: {}\ndescription: \"{}\"\n---\n\n{}\n",
                skill.name(),
                skill.frontmatter.description,
                skill.body
            ),
            warnings: Vec::new(),
        })
    }

    fn get_output_path(&self, skill: &Skill, root: &Path) -> PathBuf {
        root.join(skill.name()).join("SKILL.md")
    }
}

#[test]
fn test08_a_rejected_emitted_shape_points_at_the_line_of_the_source_file() {
    let w = World::new("column0");
    w.put(
        "skills/nacht/SKILL.md",
        "---\nname: nacht\ndescription: |\n  Runs night jobs.\n  Use when work should continue unattended.\n---\n\n# Nacht\n",
    );
    let mut registry = TranspilerRegistry::new();
    registry.register(Box::new(LegacyEmitter));
    let result = lint_outputs(&discover_skills(&w.repo()), &registry);
    let detail = result.messages[0].detail.as_ref().unwrap();
    assert_eq!(detail.code, "frontmatter-rejected");
    assert_eq!(detail.consequence, Consequence::Hidden);
    assert_eq!(detail.line, Some(5));
}

#[cfg(unix)]
mod hooks {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn with_hook(w: &World, mode: u32) {
        w.put(
            "skills/nacht/SKILL.md",
            "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\nhooks:\n  Stop:\n    command: scripts/stop.sh\n---\n\n# Nacht\n",
        );
        w.put("skills/nacht/scripts/stop.sh", "#!/bin/sh\n");
        fs::set_permissions(
            w.repo().join("skills/nacht/scripts/stop.sh"),
            fs::Permissions::from_mode(mode),
        )
        .unwrap();
    }

    fn mode(w: &World) -> u32 {
        fs::metadata(w.repo().join("skills/nacht/scripts/stop.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    }

    #[test]
    fn test09_a_hook_script_without_the_execute_bit_is_fixable_with_chmod() {
        let w = World::new("hookrow");
        with_hook(&w, 0o644);
        let rows = w.lint();
        let hook = row(&rows, "hook-not-executable");
        assert_eq!(hook["consequence"], "hook-wont-run");
        assert_eq!(hook["fixable"], "chmod");
        assert_eq!(hook["file"], "scripts/stop.sh");
        assert_eq!(hook["level"], "error");
    }

    #[test]
    fn test10_fix_chmod_previews_then_applies_and_touches_nothing_else() {
        let w = World::new("fix");
        with_hook(&w, 0o640);
        let before = fs::read(w.repo().join("skills/nacht/SKILL.md")).unwrap();
        let preview = api::fix(
            &Args {
                dry_run: true,
                ..w.args()
            },
            "chmod",
        )
        .unwrap();
        assert_eq!(preview["count"], 1);
        assert_eq!(preview["fixed"][0]["from"], "0640");
        assert_eq!(preview["fixed"][0]["to"], "0750");
        assert_eq!(mode(&w), 0o640);
        let done = api::fix(&w.args(), "chmod").unwrap();
        assert_eq!(done["dryRun"], false);
        assert_eq!(mode(&w), 0o750);
        assert_eq!(
            fs::read(w.repo().join("skills/nacht/SKILL.md")).unwrap(),
            before
        );
        assert_eq!(api::fix(&w.args(), "chmod").unwrap()["count"], 0);
        assert!(!w.lint().iter().any(|m| m["code"] == "hook-not-executable"));
    }

    #[test]
    fn test11_fix_refuses_an_unknown_fix_and_an_unknown_skill() {
        let w = World::new("fixbad");
        with_hook(&w, 0o644);
        let unknown = api::fix(&w.args(), "delete").unwrap_err();
        assert!(unknown
            .message
            .contains("safe fixes are chmod and move-link"));
        let skills = Args {
            names: Some(vec!["nope".into()]),
            ..w.args()
        };
        let missing = api::fix(&skills, "chmod").unwrap_err();
        assert!(missing.message.contains("skill not found: nope"));
        assert_eq!(mode(&w), 0o644);
    }

    #[test]
    fn test12_fix_only_reaches_the_named_skills() {
        let w = World::new("fixone");
        with_hook(&w, 0o644);
        w.put(
            "skills/other/SKILL.md",
            "---\nname: other\ndescription: \"Other skill. Use when testing the chmod fix.\"\nhooks:\n  Stop:\n    command: run.sh\n---\n\n# Other\n",
        );
        w.put("skills/other/run.sh", "#!/bin/sh\n");
        let other = w.repo().join("skills/other/run.sh");
        fs::set_permissions(&other, fs::Permissions::from_mode(0o644)).unwrap();
        let args = Args {
            names: Some(vec!["other".into()]),
            ..w.args()
        };
        let done = api::fix(&args, "chmod").unwrap();
        assert_eq!(done["count"], 1);
        assert_eq!(done["fixed"][0]["skill"], "other");
        assert_eq!(mode(&w), 0o644);
    }
}

#[test]
fn test13_files_before_the_first_sync_would_all_be_copied() {
    let w = World::new("fresh");
    w.put("skills/nacht/.DS_Store", "x");
    let data = w.files();
    assert_eq!(data["skill"], "nacht");
    assert_eq!(status_of(&data, "SKILL.md"), "would-copy");
    assert_eq!(status_of(&data, "SETUP.md"), "would-copy");
    assert_eq!(status_of(&data, "references/guide.md"), "would-copy");
    assert_eq!(status_of(&data, "templates/x/.gitignore"), "would-copy");
    assert_eq!(status_of(&data, ".DS_Store"), "not-copied");
    let skipped = data["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == ".DS_Store")
        .unwrap();
    assert!(skipped["reason"].as_str().unwrap().contains("deny-list"));
    assert_eq!(data["counts"]["would-copy"], 4);
    assert_eq!(data["counts"]["not-copied"], 1);
    assert_eq!(data["files"][0]["path"], "SKILL.md");
    assert_eq!(data["installed"][0]["present"], false);
    assert!(data["denyList"]
        .as_array()
        .unwrap()
        .contains(&Value::from(".git/")));
    assert_eq!(data["description"]["limit"], 1536);
}

#[test]
fn test14_files_after_a_sync_are_copied_and_a_hand_edit_is_marked() {
    let w = World::new("edited");
    w.sync(false);
    let data = w.files();
    assert_eq!(data["counts"]["copied"], 4);
    assert_eq!(status_of(&data, "SETUP.md"), "copied");
    fs::write(w.installed().join("SETUP.md"), "# mine\n").unwrap();
    fs::write(w.repo().join("skills/nacht/SETUP.md"), "# new\n").unwrap();
    let edited = w.files();
    assert_eq!(status_of(&edited, "SETUP.md"), "hand-edited");
    assert_eq!(status_of(&edited, "references/guide.md"), "copied");
}

#[test]
fn test15_a_changed_source_would_copy_and_an_untracked_different_file_differs() {
    let w = World::new("differs");
    w.sync(false);
    fs::write(w.repo().join("skills/nacht/references/guide.md"), "v2\n").unwrap();
    assert_eq!(status_of(&w.files(), "references/guide.md"), "would-copy");
    let fresh = World::new("differs-untracked");
    fs::create_dir_all(fresh.installed()).unwrap();
    fs::write(fresh.installed().join("SETUP.md"), "by hand\n").unwrap();
    assert_eq!(status_of(&fresh.files(), "SETUP.md"), "differs");
}

#[test]
fn test16_a_file_dropped_from_the_library_is_removed_on_the_next_confirmed_sync() {
    let w = World::new("removal");
    w.sync(false);
    fs::remove_file(w.repo().join("skills/nacht/SETUP.md")).unwrap();
    let data = w.files();
    assert_eq!(status_of(&data, "SETUP.md"), "removed-on-next-sync");
    assert_eq!(data["counts"]["removed-on-next-sync"], 1);
    w.sync(true);
    assert!(data_without(&w.files(), "SETUP.md"));
}

fn data_without(data: &Value, path: &str) -> bool {
    !data["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["path"] == path)
}

#[test]
fn test17_a_file_the_sync_never_wrote_is_not_yours_and_stays() {
    let w = World::new("foreign");
    w.sync(false);
    fs::create_dir_all(w.installed().join("notes")).unwrap();
    fs::write(w.installed().join("notes/mine.md"), "mine\n").unwrap();
    let data = w.files();
    assert_eq!(status_of(&data, "notes/mine.md"), "not-yours");
    assert_eq!(data["counts"]["not-yours"], 1);
    w.sync(true);
    assert!(Path::new(&w.installed().join("notes/mine.md")).exists());
}

#[test]
fn test18_links_list_every_relative_link_with_what_the_sync_does_with_it() {
    let w = World::new("linkstate");
    w.put("skills/nacht/build/out.md", "x");
    w.put(
        "skills/nacht/SKILL.md",
        "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\nSee [a](references/guide.md), [b](build/out.md), [c](nope.md) and [d](../x.md).\n",
    );
    let data = w.files();
    let states: Vec<(String, String)> = data["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["target"].as_str().unwrap().to_string(),
                l["state"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("references/guide.md".into(), "installed".into()),
            ("build/out.md".into(), "not-installed".into()),
            ("nope.md".into(), "missing".into()),
            ("../x.md".into(), "outside-folder".into()),
        ]
    );
    assert_eq!(data["links"][0]["line"], 6);
}

#[test]
fn test19_files_of_an_unknown_skill_is_not_found_and_writes_nothing() {
    let w = World::new("unknown");
    let args = Args {
        name: "nope".into(),
        ..w.args()
    };
    let error = skill_files(&args).unwrap_err();
    assert!(error.message.contains("skill not found: nope"));
    assert!(!w.base.join("home/.claude").exists());
}
