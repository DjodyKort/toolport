use super::api::{self, Args};
use super::compare::{compare, keep_files};
use crate::fsx::{enter, Paths, PathsGuard};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use toolport_plus_api::memory::MemoryHost;

struct World {
    base: PathBuf,
    _paths: PathsGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("skills-compare-{tag}-{}", std::process::id()));
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
            "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\n# Nacht\n",
        );
        world.put("skills/nacht/SETUP.md", "# setup\nstep one\nstep two\n");
        world.put("skills/nacht/references/guide.md", "guide\n");
        world
    }

    fn repo(&self) -> PathBuf {
        self.base.join("repo")
    }

    fn installed(&self, rel: &str) -> PathBuf {
        self.base.join("home/.claude/skills/nacht").join(rel)
    }

    fn put(&self, rel: &str, body: &str) {
        let path = self.repo().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn put_installed(&self, rel: &str, body: &[u8]) {
        let path = self.installed(rel);
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

    fn sync(&self, args: Args) -> Value {
        api::sync(&args, Some(&MemoryHost::default())).unwrap()
    }

    fn plan_op(&self, path_end: &str) -> Option<String> {
        let data = self.sync(Args {
            dry_run: true,
            ..self.args()
        });
        data["plan"]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["path"].as_str().unwrap().ends_with(path_end))
            .map(|c| c["op"].as_str().unwrap().to_string())
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn file<'a>(data: &'a Value, path: &str) -> &'a Value {
    data["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no {path} in {data}"))
}

#[test]
fn test01_a_file_that_differs_comes_back_with_both_copies_and_a_unified_diff() {
    let w = World::new("differs");
    w.put_installed("SETUP.md", b"# setup\nstep one\nmy own step\n");
    let data = compare(&w.args(), None).unwrap();
    assert_eq!(data["differing"], 1);
    let row = file(&data, "SETUP.md");
    assert_eq!(row["status"], "differs");
    assert_eq!(row["library"]["text"], "# setup\nstep one\nstep two\n");
    assert_eq!(row["installed"]["text"], "# setup\nstep one\nmy own step\n");
    assert_eq!(
        row["diff"],
        "--- library\n+++ installed\n@@ -1,3 +1,3 @@\n # setup\n step one\n-step two\n+my own step\n"
    );
    assert_eq!(
        (row["added"].as_u64(), row["removed"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(row["binary"], false);
}

#[test]
fn test02_a_hand_edit_of_skill_md_is_compared_with_what_the_sync_would_write() {
    let w = World::new("edited");
    w.sync(w.args());
    let installed = fs::read_to_string(w.installed("SKILL.md")).unwrap();
    fs::write(
        w.installed("SKILL.md"),
        format!("{installed}Added by hand.\n"),
    )
    .unwrap();
    let data = compare(&w.args(), None).unwrap();
    let row = file(&data, "SKILL.md");
    assert_eq!(row["status"], "hand-edited");
    assert!(row["diff"].as_str().unwrap().contains("+Added by hand."));
    assert_eq!(row["added"], 1);
}

#[test]
fn test03_file_narrows_the_answer_and_an_unknown_file_is_not_found() {
    let w = World::new("narrow");
    w.put_installed("SETUP.md", b"changed\n");
    w.put_installed("references/guide.md", b"changed too\n");
    assert_eq!(compare(&w.args(), None).unwrap()["differing"], 2);
    let one = compare(&w.args(), Some("references/guide.md")).unwrap();
    assert_eq!(one["differing"], 1);
    assert_eq!(one["files"][0]["path"], "references/guide.md");
    let error = compare(&w.args(), Some("nope.md")).unwrap_err();
    assert!(
        error.message.contains("no file nope.md"),
        "{}",
        error.message
    );
}

#[test]
fn test04_a_binary_file_is_flagged_and_carries_no_text() {
    let w = World::new("binary");
    w.put_installed("SETUP.md", &[0, 159, 146, 150, 0]);
    let data = compare(&w.args(), None).unwrap();
    let row = file(&data, "SETUP.md");
    assert_eq!(row["binary"], true);
    assert_eq!(row["diff"], Value::Null);
    assert_eq!(row["installed"]["text"], Value::Null);
    assert_eq!(row["installed"]["size"], 5);
    assert_eq!(row["library"]["text"], "# setup\nstep one\nstep two\n");
}

#[test]
fn test05_keep_stops_the_hold_until_the_file_changes_again() {
    let w = World::new("keep");
    w.put_installed("SETUP.md", b"mine\n");
    assert_eq!(w.plan_op("SETUP.md").as_deref(), Some("differs"));
    let dry = keep_files(&w.args(), &["SETUP.md".into()], true).unwrap();
    assert_eq!(dry["count"], 1);
    assert_eq!(w.plan_op("SETUP.md").as_deref(), Some("differs"));
    keep_files(&w.args(), &["SETUP.md".into()], false).unwrap();
    assert_eq!(w.plan_op("SETUP.md"), None);
    w.sync(w.args());
    assert_eq!(
        fs::read_to_string(w.installed("SETUP.md")).unwrap(),
        "mine\n"
    );
    w.put_installed("SETUP.md", b"mine, edited again\n");
    assert_eq!(w.plan_op("SETUP.md").as_deref(), Some("differs"));
}

#[test]
fn test06_keep_refuses_a_file_the_sync_does_not_hold() {
    let w = World::new("keep-refused");
    let error = keep_files(&w.args(), &["references/guide.md".into()], false).unwrap_err();
    assert!(
        error.message.contains("is not held back"),
        "{}",
        error.message
    );
    let none = keep_files(&w.args(), &[], false).unwrap_err();
    assert!(none.message.contains("name the files"), "{}", none.message);
}

#[test]
fn test07_take_replaces_only_the_named_file_and_backs_up_the_other_edit() {
    let w = World::new("take");
    w.put_installed("SETUP.md", b"mine\n");
    w.put_installed("references/guide.md", b"also mine\n");
    w.sync(Args {
        take: vec!["nacht/SETUP.md".into()],
        ..w.args()
    });
    assert_eq!(
        fs::read_to_string(w.installed("SETUP.md")).unwrap(),
        "# setup\nstep one\nstep two\n"
    );
    assert_eq!(
        fs::read_to_string(w.installed("references/guide.md")).unwrap(),
        "also mine\n"
    );
    assert_eq!(w.plan_op("references/guide.md").as_deref(), Some("differs"));
}

#[test]
fn test08_an_unattended_run_refuses_take() {
    let w = World::new("unattended");
    let error = api::sync(
        &Args {
            unattended: true,
            take: vec!["nacht/SETUP.md".into()],
            ..w.args()
        },
        None,
    )
    .unwrap_err();
    assert!(error.message.contains("take"), "{}", error.message);
}

#[test]
fn test09_only_syncs_the_named_skill_and_leaves_the_rest_of_the_lock_alone() {
    let w = World::new("only");
    w.put(
        "skills/other/SKILL.md",
        "---\nname: other\ndescription: \"Another skill. Use it when asked.\"\n---\n\n# Other\n",
    );
    let only_nacht = Args {
        only: Some(vec!["nacht".into()]),
        ..w.args()
    };
    w.sync(only_nacht.clone());
    assert!(w.installed("SKILL.md").is_file());
    assert!(!w.base.join("home/.claude/skills/other").exists());
    w.sync(w.args());
    assert!(w.base.join("home/.claude/skills/other/SKILL.md").is_file());

    fs::remove_dir_all(w.repo().join("skills/other")).unwrap();
    w.put("skills/nacht/SETUP.md", "# setup changed\n");
    let data = w.sync(only_nacht);
    let paths: Vec<&str> = data["plan"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, [".claude/skills/nacht/SETUP.md"]);
    assert!(w.base.join("home/.claude/skills/other/SKILL.md").is_file());
    let full = w.sync(Args {
        dry_run: true,
        ..w.args()
    });
    assert_eq!(full["plan"]["counts"]["remove"], 1);
}
