use super::run::{run, Action, RunOptions};
use super::run_tests::{read_json, with_world, World};
use crate::plus::skills::ops::find_skills_repo;
use crate::plus::skills::parser::discover_skills;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn clone_with_a_skill(at: &Path) {
    write(
        &at.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo skill\n---\nBody\n",
    );
}

fn sync_config(w: &World, local: &str) {
    let doc = json!({
        "repo": "git@example.com:acme/skills.git",
        "branch": "trunk",
        "auto_sync": true,
        "local_path": local,
    });
    write(&w.root.join("skills_sync.json"), &doc.to_string());
}

fn opts(w: &World, dry_run: bool) -> RunOptions {
    RunOptions {
        home: Some(w.home.to_string_lossy().into_owned()),
        write_clients: false,
        ..w.opts(dry_run)
    }
}

fn target(w: &World) -> PathBuf {
    w.data().join("skills_sync.json")
}

fn found_from_elsewhere(w: &World) -> Option<PathBuf> {
    let cwd = w.base.join("elsewhere");
    std::fs::create_dir_all(&cwd).unwrap();
    find_skills_repo(None, &cwd, &w.data())
}

#[test]
fn a_clone_where_mcpm_left_it_is_found_from_outside_the_repo() {
    with_world("ss-keep", |w| {
        let clone = w.home.join(".config/mcpm/skills_repo");
        clone_with_a_skill(&clone);
        sync_config(w, &clone.to_string_lossy());
        assert_eq!(found_from_elsewhere(w), None);

        let plan = run(&opts(w, false)).unwrap();

        assert_eq!(plan.skills_sync.len(), 1);
        assert_eq!(plan.skills_sync[0].id, "skills_sync.json");
        assert_eq!(plan.skills_sync[0].action, Action::Created);
        let copy = read_json(&target(w));
        assert_eq!(copy["local_path"], clone.to_string_lossy().as_ref());
        assert_eq!(copy["repo"], "git@example.com:acme/skills.git");
        assert_eq!(copy["branch"], "trunk");
        assert_eq!(copy["auto_sync"], true);
        let found = found_from_elsewhere(w).expect("skills repository");
        assert_eq!(found, clone);
        let names: Vec<String> = discover_skills(&found)
            .iter()
            .map(|s| s.name().to_string())
            .collect();
        assert_eq!(names, ["demo"]);
        assert!(plan
            .summary()
            .contains("skills-sync skills_sync.json created"));
        let plan_json = plan.to_value();
        assert_eq!(
            plan_json["skillsSync"],
            json!([{"id": "skills_sync.json", "action": "created"}])
        );
    });
}

#[test]
fn a_clone_that_moved_into_the_data_dir_is_followed() {
    with_world("ss-moved", |w| {
        let old = w.root.join("skills_repo");
        sync_config(w, &old.to_string_lossy());
        let moved = w.data().join("skills_repo");
        clone_with_a_skill(&moved);
        assert!(!old.exists());

        let plan = run(&opts(w, false)).unwrap();

        let copy = read_json(&target(w));
        assert_eq!(copy["local_path"], moved.to_string_lossy().as_ref());
        assert_eq!(found_from_elsewhere(w), Some(moved.clone()));
        let kinds: Vec<&str> = plan
            .warnings
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|x| x["kind"].as_str())
            .collect();
        assert!(kinds.contains(&"skills-repo-moved"), "{kinds:?}");
        assert!(!kinds.contains(&"skills-repo-missing"));
        assert_eq!(
            std::fs::read_to_string(w.root.join("skills_sync.json")).unwrap(),
            json!({"repo": "git@example.com:acme/skills.git", "branch": "trunk",
                   "auto_sync": true, "local_path": old.to_string_lossy()})
            .to_string()
        );
    });
}

#[test]
fn a_clone_keeps_its_place_below_the_root_when_it_moves() {
    with_world("ss-nested", |w| {
        sync_config(w, &w.root.join("clones/team").to_string_lossy());
        let moved = w.data().join("clones/team");
        clone_with_a_skill(&moved);

        run(&opts(w, false)).unwrap();

        assert_eq!(
            read_json(&target(w))["local_path"],
            moved.to_string_lossy().as_ref()
        );
    });
}

#[test]
fn a_clone_from_another_machine_is_found_by_its_name() {
    with_world("ss-name", |w| {
        sync_config(w, "/Users/someone/.config/mcpm/skills_repo");
        let moved = w.data().join("skills_repo");
        clone_with_a_skill(&moved);

        run(&opts(w, false)).unwrap();

        assert_eq!(found_from_elsewhere(w), Some(moved));
    });
}

#[test]
fn a_clone_that_is_nowhere_is_copied_as_written_and_warned_about() {
    with_world("ss-missing", |w| {
        sync_config(w, "/nonexistent/skills_repo");

        let plan = run(&opts(w, false)).unwrap();

        assert_eq!(
            read_json(&target(w))["local_path"],
            "/nonexistent/skills_repo"
        );
        let missing: Vec<&Value> = plan
            .warnings
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["kind"] == "skills-repo-missing")
            .collect();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0]["detail"], "/nonexistent/skills_repo");
        assert_eq!(found_from_elsewhere(w), None);
    });
}

#[test]
fn the_home_shorthand_becomes_an_absolute_path() {
    with_world("ss-tilde", |w| {
        let clone = w.home.join("clone");
        clone_with_a_skill(&clone);
        sync_config(w, "~/clone");

        run(&opts(w, false)).unwrap();

        assert_eq!(
            read_json(&target(w))["local_path"],
            clone.to_string_lossy().as_ref()
        );
        assert_eq!(found_from_elsewhere(w), Some(clone));
    });
}

#[test]
fn a_dry_run_plans_the_copy_and_writes_nothing() {
    with_world("ss-dry", |w| {
        let clone = w.home.join("clone");
        clone_with_a_skill(&clone);
        sync_config(w, &clone.to_string_lossy());

        let plan = run(&opts(w, true)).unwrap();

        assert_eq!(plan.skills_sync[0].action, Action::Created);
        assert!(!target(w).exists());
    });
}

#[test]
fn a_rerun_changes_nothing_and_follows_the_clone_once_it_moves() {
    with_world("ss-rerun", |w| {
        sync_config(w, &w.root.join("skills_repo").to_string_lossy());
        run(&opts(w, false)).unwrap();
        let first = std::fs::read_to_string(target(w)).unwrap();
        assert_eq!(
            read_json(&target(w))["local_path"],
            w.root.join("skills_repo").to_string_lossy().as_ref()
        );

        let same = run(&opts(w, false)).unwrap();
        assert_eq!(same.skills_sync[0].action, Action::Unchanged);
        assert_eq!(std::fs::read_to_string(target(w)).unwrap(), first);

        let moved = w.data().join("skills_repo");
        clone_with_a_skill(&moved);
        let follow = run(&opts(w, false)).unwrap();
        assert_eq!(follow.skills_sync[0].action, Action::Updated);
        assert_eq!(found_from_elsewhere(w), Some(moved));

        let settled = run(&opts(w, false)).unwrap();
        assert_eq!(settled.skills_sync[0].action, Action::Unchanged);
    });
}

#[test]
fn a_file_the_user_wrote_is_a_conflict_and_stays_as_it_is() {
    with_world("ss-conflict", |w| {
        sync_config(w, "/nonexistent/skills_repo");
        let own = json!({"repo": "git@example.com:me/skills.git", "local_path": "/elsewhere"});
        write(&target(w), &own.to_string());

        let plan = run(&opts(w, false)).unwrap();

        assert_eq!(plan.skills_sync[0].action, Action::Conflict);
        assert_eq!(read_json(&target(w)), own);
    });
}

#[test]
fn no_skills_sync_json_in_the_root_is_no_row_and_no_file() {
    with_world("ss-none", |w| {
        let plan = run(&opts(w, false)).unwrap();

        assert!(plan.skills_sync.is_empty());
        assert!(!target(w).exists());
        assert_eq!(plan.to_value()["skillsSync"], json!([]));
    });
}

#[test]
fn an_unreadable_file_is_a_warning_and_the_import_goes_on() {
    with_world("ss-broken", |w| {
        write(&w.root.join("skills_sync.json"), "{ not json");

        let plan = run(&opts(w, false)).unwrap();

        assert!(plan.skills_sync.is_empty());
        assert!(!target(w).exists());
        assert!(plan
            .warnings
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["kind"] == "skills-sync-unreadable"));
        assert!(w.registry_text().is_some());
    });
}
