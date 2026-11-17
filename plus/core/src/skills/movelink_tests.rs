use super::api::{self, Args};
use super::movelink::{move_link, DEFAULT_DEST};
use super::parser::find_skill;
use crate::fsx::{enter, Paths, PathsGuard};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

struct World {
    base: PathBuf,
    _paths: PathsGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("skills-movelink-{tag}-{}", std::process::id()));
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
            "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\nRead the [guide](../../shared/guide.md#setup) first.\nAlso <../../shared/guide.md> and [again](../../shared/guide.md).\n\n```\n[example](../../shared/guide.md)\n```\n\nInline `[code](../../shared/guide.md)` stays.\n",
        );
        world.put("shared/guide.md", "# guide\nsetup\n");
        world
    }

    fn repo(&self) -> PathBuf {
        self.base.join("repo")
    }

    fn put(&self, rel: &str, body: &str) {
        let path = self.repo().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn skill_text(&self) -> String {
        fs::read_to_string(self.repo().join("skills/nacht/SKILL.md")).unwrap()
    }

    fn args(&self, link: &str, dry_run: bool) -> Args {
        Args {
            repo: Some(self.repo()),
            names: Some(vec!["nacht".into()]),
            link: Some(link.into()),
            dry_run,
            ..Args::default()
        }
    }

    fn lint_row(&self) -> Option<Value> {
        let data = api::lint(&Args {
            repo: Some(self.repo()),
            ..Args::default()
        })
        .unwrap();
        data["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["code"] == "link-outside-folder")
            .cloned()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

const LINK: &str = "../../shared/guide.md";

#[test]
fn test01_the_lint_row_of_a_link_the_library_can_move_offers_the_fix() {
    let w = World::new("lint");
    let row = w.lint_row().unwrap();
    assert_eq!(row["fixable"], "move-link");
    assert_eq!(row["target"], LINK);
    w.put(
        "skills/nacht/SKILL.md",
        &w.skill_text().replace(LINK, "../../../outside.md"),
    );
    fs::write(w.base.join("outside.md"), "x").unwrap();
    assert_eq!(w.lint_row().unwrap().get("fixable"), None);
}

#[test]
fn test02_the_preview_lists_the_copy_and_the_lines_and_writes_nothing() {
    let w = World::new("preview");
    let before = w.skill_text();
    let data = api::fix(&w.args(LINK, true), "move-link").unwrap();
    assert_eq!(data["dryRun"], true);
    assert_eq!(data["to"], "references/guide.md");
    assert_eq!(data["count"], 2);
    assert_eq!(data["rewrites"][0]["line"], 6);
    assert_eq!(
        data["rewrites"][0]["after"],
        "Read the [guide](references/guide.md#setup) first."
    );
    assert_eq!(w.skill_text(), before);
    assert!(!w.repo().join("skills/nacht/references").exists());
}

#[test]
fn test03_the_fix_copies_the_file_and_rewrites_links_outside_code_only() {
    let w = World::new("apply");
    api::fix(&w.args(LINK, false), "move-link").unwrap();
    assert_eq!(
        fs::read_to_string(w.repo().join("skills/nacht/references/guide.md")).unwrap(),
        "# guide\nsetup\n"
    );
    let text = w.skill_text();
    assert!(text.contains("[guide](references/guide.md#setup)"));
    assert!(text.contains("Also <../../shared/guide.md>"));
    assert!(text.contains("[again](references/guide.md)"));
    assert!(text.contains("```\n[example](../../shared/guide.md)\n```"));
    assert!(text.contains("`[code](../../shared/guide.md)`"));
    assert!(w.repo().join("shared/guide.md").is_file());
    assert!(w.lint_row().is_none_or(|row| row["target"] != LINK));
}

#[test]
fn test04_the_same_content_in_the_folder_is_reused_and_other_content_is_a_conflict() {
    let w = World::new("exists");
    w.put("skills/nacht/references/guide.md", "# guide\nsetup\n");
    let data = api::fix(&w.args(LINK, false), "move-link").unwrap();
    assert_eq!(data["reused"], true);
    let w = World::new("conflict");
    w.put("skills/nacht/references/guide.md", "other\n");
    let error = api::fix(&w.args(LINK, false), "move-link").unwrap_err();
    assert!(
        error.message.contains("already exists"),
        "{}",
        error.message
    );
    assert!(w.skill_text().contains(LINK));
}

#[test]
fn test05_a_link_the_fix_cannot_move_is_refused_by_what_is_wrong_with_it() {
    let w = World::new("refused");
    let refuse = |link: &str| {
        api::fix(&w.args(link, true), "move-link")
            .unwrap_err()
            .message
    };
    assert!(refuse("references/x.md").contains("is not a link of SKILL.md that leaves"));
    fs::create_dir_all(w.repo().join("shared/folder")).unwrap();
    w.put(
        "skills/nacht/SKILL.md",
        &w.skill_text()
            .replace("Also <", "See [dir](../../shared/folder) and <"),
    );
    assert!(refuse("../../shared/folder").contains("folder"));
    let error = api::fix(
        &Args {
            dest: Some("../escape".into()),
            ..w.args(LINK, true)
        },
        "move-link",
    )
    .unwrap_err();
    assert!(
        error.message.contains("dest must be a folder"),
        "{}",
        error.message
    );
    let skill = find_skill(&w.repo(), "nacht").unwrap();
    assert!(move_link(&skill, LINK, ".git", true)
        .unwrap_err()
        .message
        .contains("deny-list"));
    assert_eq!(DEFAULT_DEST, "references");
}

#[test]
fn test06_a_missing_target_and_a_link_to_a_file_outside_the_library_are_not_moved() {
    let w = World::new("outside");
    w.put(
        "skills/nacht/SKILL.md",
        &w.skill_text().replace(LINK, "../../../shared-by-hand.md"),
    );
    fs::write(w.base.join("shared-by-hand.md"), "x").unwrap();
    let error = api::fix(&w.args("../../../shared-by-hand.md", true), "move-link").unwrap_err();
    assert!(
        error.message.contains("outside the library"),
        "{}",
        error.message
    );
    let error = api::fix(&w.args("../../../nothing.md", true), "move-link").unwrap_err();
    assert!(error.message.contains("not a link"), "{}", error.message);
}
