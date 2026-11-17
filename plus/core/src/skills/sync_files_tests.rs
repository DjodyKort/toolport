use super::clock::{FixedClock, Instant};
use super::lint::{lint_description_lengths, lint_files};
use super::lock::{load_lockfile, save_lockfile};
use super::parser::discover_skills;
use super::sync::{sync_skills, Op, SyncOptions, SyncResult};
use super::transpilers::registry_with_home;
use super::verify::verify_installed;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("skills-files-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn put(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// Written the way the claude-code transpiler renders it, so an installed copy is byte-identical.
const SKILL_MD: &str = "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\n# Nacht\n\nSee [setup](SETUP.md) and [the guide](references/guide.md).\n";

struct World {
    repo: Tmp,
    home: Tmp,
}

impl World {
    fn new(tag: &str) -> Self {
        let w = World {
            repo: Tmp::new(&format!("{tag}-repo")),
            home: Tmp::new(&format!("{tag}-home")),
        };
        put(&w.repo.0, "skills/nacht/SKILL.md", SKILL_MD);
        put(&w.repo.0, "skills/nacht/SETUP.md", "# setup\n");
        put(&w.repo.0, "skills/nacht/references/guide.md", "guide\n");
        put(&w.repo.0, "skills/nacht/templates/x/.gitignore", "*.tmp\n");
        put(&w.repo.0, "skills/nacht/modules/a.md", "a\n");
        w
    }

    fn sync(&self, apply_held: bool, dry_run: bool) -> SyncResult {
        let clock = FixedClock(Instant {
            unix_secs: 1_700_000_000,
            micros: 0,
        });
        let registry = registry_with_home(Some(self.home.0.clone()));
        let opts = SyncOptions {
            output_root: self.home.0.clone(),
            lock_dir: self.home.0.clone(),
            global_mode: true,
            dry_run,
            migrate: None,
            client_keys: Some(vec!["claude-code".into()]),
            apply_held,
            clock: &clock,
        };
        let result = sync_skills(&discover_skills(&self.repo.0), &registry, &opts).unwrap();
        if !dry_run {
            super::save_lockfile(&self.home.0, &result.lockfile).unwrap();
        }
        result
    }

    fn installed(&self) -> PathBuf {
        self.home.0.join(".claude/skills/nacht")
    }
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn ops(result: &SyncResult, op: Op) -> Vec<String> {
    result
        .plan
        .changes
        .iter()
        .filter(|c| c.op == op)
        .map(|c| c.path.clone())
        .collect()
}

#[test]
fn test01_the_whole_skill_folder_is_installed_and_equals_the_source() {
    let w = World::new("whole");
    let result = w.sync(false, false);
    assert_eq!(result.plan.count(Op::Add), 5);
    assert!(result.plan.skipped.is_empty());
    assert_eq!(tree(&w.repo.0.join("skills/nacht")), tree(&w.installed()));
    let outputs = &result.lockfile.skills[0].1.output_files[0].1;
    assert!(outputs.contains(&".claude/skills/nacht/templates/x/.gitignore".to_string()));
    assert!(outputs.contains(&".claude/skills/nacht/SETUP.md".to_string()));
    assert!(outputs.contains(&".claude/skills/nacht/references/guide.md".to_string()));
}

#[test]
fn test02_caches_and_vcs_folders_stay_out_and_are_reported() {
    let w = World::new("deny");
    put(&w.repo.0, "skills/nacht/.git/HEAD", "ref\n");
    put(&w.repo.0, "skills/nacht/.DS_Store", "x");
    put(&w.repo.0, "skills/nacht/scripts/__pycache__/m.pyc", "x");
    put(&w.repo.0, "skills/nacht/scripts/run.sh", "echo\n");
    put(&w.repo.0, "skills/nacht/run.log", "x");
    let result = w.sync(false, false);
    let skipped: Vec<&str> = result
        .plan
        .skipped
        .iter()
        .map(|s| s.path.as_str())
        .collect();
    assert_eq!(
        skipped,
        [".DS_Store", ".git/", "run.log", "scripts/__pycache__/"]
    );
    let installed = tree(&w.installed());
    assert!(installed.contains_key("scripts/run.sh"));
    assert!(!installed
        .keys()
        .any(|k| k.split('/').any(|c| c == ".git" || c == "__pycache__")));
}

#[test]
fn test03_a_second_sync_writes_nothing() {
    let w = World::new("again");
    w.sync(false, false);
    let again = w.sync(false, false);
    assert_eq!(again.plan.count(Op::Unchanged), 5);
    assert_eq!(again.plan.changes.len(), 5);
}

#[test]
fn test04_a_file_dropped_from_the_source_is_held_then_removed_on_confirmation() {
    let w = World::new("remove");
    w.sync(false, false);
    fs::remove_file(w.repo.0.join("skills/nacht/modules/a.md")).unwrap();
    let held = w.sync(false, false);
    assert_eq!(
        ops(&held, Op::Remove),
        [".claude/skills/nacht/modules/a.md"]
    );
    assert!(held.plan.held().count() == 1);
    assert!(w.installed().join("modules/a.md").exists());
    let lock = load_lockfile(&w.home.0).unwrap();
    assert!(lock.skills[0].1.output_files[0]
        .1
        .contains(&".claude/skills/nacht/modules/a.md".to_string()));

    let done = w.sync(true, false);
    assert!(!w.installed().join("modules").exists());
    assert_eq!(done.cleaned.len(), 1);
    assert_eq!(tree(&w.repo.0.join("skills/nacht")), tree(&w.installed()));
}

#[test]
fn test05_a_file_the_sync_never_installed_is_not_touched() {
    let w = World::new("foreign");
    w.sync(false, false);
    put(&w.installed(), "notes/mine.md", "mine\n");
    fs::remove_file(w.repo.0.join("skills/nacht/modules/a.md")).unwrap();
    w.sync(true, false);
    assert_eq!(
        fs::read_to_string(w.installed().join("notes/mine.md")).unwrap(),
        "mine\n"
    );
}

#[test]
fn test06_a_removed_skill_waits_for_confirmation_with_its_files() {
    let w = World::new("gone");
    w.sync(false, false);
    fs::remove_dir_all(w.repo.0.join("skills/nacht")).unwrap();
    put(&w.repo.0, "skills/other/SKILL.md", "---\nname: other\ndescription: \"Another skill for the test. Use when needed here.\"\n---\n\n# Other\n");
    let held = w.sync(false, false);
    assert_eq!(held.plan.count(Op::Remove), 5);
    assert!(w.installed().join("references/guide.md").exists());
    let lock = load_lockfile(&w.home.0).unwrap();
    assert!(lock.skills.iter().any(|(name, _)| name == "nacht"));

    w.sync(true, false);
    assert!(!w.installed().exists());
    assert!(w.home.0.join(".claude/skills/other/SKILL.md").exists());
    let lock = load_lockfile(&w.home.0).unwrap();
    assert!(!lock.skills.iter().any(|(name, _)| name == "nacht"));
}

#[test]
fn test07_an_identical_file_without_a_lock_record_is_adopted_without_a_write() {
    let w = World::new("adopt");
    put(&w.installed(), "SETUP.md", "# setup\n");
    let result = w.sync(false, false);
    assert_eq!(ops(&result, Op::Adopt), [".claude/skills/nacht/SETUP.md"]);
    assert!(result.lockfile.skills[0].1.output_files[0]
        .1
        .contains(&".claude/skills/nacht/SETUP.md".to_string()));
}

#[test]
fn test08_a_differing_untracked_file_waits_and_is_backed_up_when_replaced() {
    let w = World::new("differs");
    put(&w.installed(), "SETUP.md", "# hand copy\n");
    let held = w.sync(false, false);
    assert_eq!(ops(&held, Op::Differs), [".claude/skills/nacht/SETUP.md"]);
    assert_eq!(
        fs::read_to_string(w.installed().join("SETUP.md")).unwrap(),
        "# hand copy\n"
    );
    let lock = load_lockfile(&w.home.0).unwrap();
    assert!(!lock.skills[0].1.output_files[0]
        .1
        .contains(&".claude/skills/nacht/SETUP.md".to_string()));

    w.sync(true, false);
    assert_eq!(
        fs::read_to_string(w.installed().join("SETUP.md")).unwrap(),
        "# setup\n"
    );
    let backups = tree(&w.home.0.join(".mcpm-backups"));
    assert!(backups.keys().any(|k| k.contains("SETUP.md.")));
}

#[test]
fn test09_a_changed_tracked_file_is_updated_without_a_decision() {
    let w = World::new("update");
    w.sync(false, false);
    put(&w.repo.0, "skills/nacht/references/guide.md", "guide v2\n");
    let result = w.sync(false, false);
    assert_eq!(
        ops(&result, Op::Update),
        [".claude/skills/nacht/references/guide.md"]
    );
    assert_eq!(result.plan.held().count(), 0);
}

#[test]
fn test10_a_dry_run_changes_nothing() {
    let w = World::new("dry");
    let result = w.sync(false, true);
    assert_eq!(result.plan.count(Op::Add), 5);
    assert!(!w.installed().exists());
}

#[test]
fn test11_verification_is_clean_after_a_sync_and_names_a_stale_copy() {
    let w = World::new("verify");
    w.sync(false, false);
    let registry = registry_with_home(Some(w.home.0.clone()));
    let lock = load_lockfile(&w.home.0).unwrap();
    let skills = discover_skills(&w.repo.0);
    let report = verify_installed(&skills, &lock, &registry, &w.home.0);
    assert!(report.clean());
    assert_eq!(report.checked, 5);

    fs::write(w.installed().join("SETUP.md"), "stale").unwrap();
    fs::remove_file(w.installed().join("templates/x/.gitignore")).unwrap();
    let report = verify_installed(&skills, &lock, &registry, &w.home.0);
    assert_eq!(report.differing[0].path, ".claude/skills/nacht/SETUP.md");
    assert_eq!(
        report.missing[0].path,
        ".claude/skills/nacht/templates/x/.gitignore"
    );
}

#[cfg(unix)]
#[test]
fn test12_a_link_inside_the_folder_is_kept_and_one_that_leaves_is_reported() {
    let w = World::new("links");
    let skill = w.repo.0.join("skills/nacht");
    std::os::unix::fs::symlink("SETUP.md", skill.join("alias.md")).unwrap();
    std::os::unix::fs::symlink("../../../elsewhere", skill.join("out")).unwrap();
    let result = w.sync(false, false);
    assert_eq!(
        fs::read_link(w.installed().join("alias.md")).unwrap(),
        PathBuf::from("SETUP.md")
    );
    assert!(result
        .plan
        .skipped
        .iter()
        .any(|s| s.path == "out" && s.reason.contains("link")));
}

#[cfg(unix)]
#[test]
fn test13_the_executable_bit_survives_the_copy() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new("exec");
    put(&w.repo.0, "skills/nacht/scripts/run.sh", "#!/bin/sh\n");
    let path = w.repo.0.join("skills/nacht/scripts/run.sh");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    w.sync(false, false);
    let mode = fs::metadata(w.installed().join("scripts/run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111);
}

#[test]
fn test14_lint_reports_skipped_files_and_links_that_would_not_be_installed() {
    let w = World::new("lint");
    put(&w.repo.0, "skills/nacht/build/out.md", "x");
    put(
        &w.repo.0,
        "skills/nacht/SKILL.md",
        "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\nSee [a](references/guide.md), [b](build/out.md), [c](missing.md), [d](https://example.test/x), [e](#top) and ![i](references/guide.md).\n\n```\n[z](nowhere.md)\n```\n",
    );
    let result = lint_files(&discover_skills(&w.repo.0));
    let warnings: Vec<&str> = result.warnings().map(|m| m.message.as_str()).collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].contains("build/out.md") && warnings[0].contains("would not be installed"));
    assert!(warnings[1].contains("missing.md") && warnings[1].contains("does not exist"));
    let infos: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.level == super::lint::LintLevel::Info)
        .collect();
    assert_eq!(infos.len(), 1);
    assert!(infos[0].message.contains("build/"));
}

#[test]
fn test15_lint_flags_a_hook_script_that_is_not_executable() {
    let w = World::new("hooks");
    put(
        &w.repo.0,
        "skills/nacht/SKILL.md",
        "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\nhooks:\n  Stop:\n    command: scripts/stop.sh\n---\n\n# Nacht\n",
    );
    put(&w.repo.0, "skills/nacht/scripts/stop.sh", "#!/bin/sh\n");
    let result = lint_files(&discover_skills(&w.repo.0));
    let errors: Vec<&str> = result.errors().map(|m| m.message.as_str()).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("stop.sh") && errors[0].contains("not executable"));
}

#[test]
fn test16_a_description_over_the_spec_limit_warns_and_errors_once_another_client_is_on() {
    let w = World::new("desc");
    let long = "x".repeat(1100);
    put(
        &w.repo.0,
        "skills/nacht/SKILL.md",
        &format!("---\nname: nacht\ndescription: \"{long}\"\n---\n\n# Nacht\n"),
    );
    let skills = discover_skills(&w.repo.0);
    assert_eq!(skills.len(), 1);
    assert_eq!(
        lint_description_lengths(&skills, false).warnings().count(),
        1
    );
    assert_eq!(lint_description_lengths(&skills, true).errors().count(), 1);
    let too_long = "x".repeat(1600);
    put(
        &w.repo.0,
        "skills/nacht/SKILL.md",
        &format!("---\nname: nacht\ndescription: \"{too_long}\"\n---\n\n# Nacht\n"),
    );
    assert!(discover_skills(&w.repo.0).is_empty());
}

const GUIDE: &str = ".claude/skills/nacht/references/guide.md";

fn recorded_hash(w: &World, rel: &str) -> Option<String> {
    let lock = load_lockfile(&w.home.0).unwrap();
    lock.skills[0]
        .1
        .file_hash("claude-code", rel)
        .map(str::to_string)
}

#[test]
fn test17_the_lock_records_a_hash_for_every_installed_file() {
    let w = World::new("hashes");
    w.sync(false, false);
    let lock = load_lockfile(&w.home.0).unwrap();
    let hashes = &lock.skills[0].1.file_hashes;
    assert_eq!(hashes.len(), 1);
    assert_eq!(hashes[0].0, "claude-code");
    assert_eq!(hashes[0].1.len(), 5);
    assert!(hashes[0].1.iter().all(|(_, h)| h.starts_with("sha256:")));
    assert_eq!(
        lock,
        super::lock::LockFile::parse(&lock.serialize()).unwrap()
    );
}

#[test]
fn test18_a_hand_edit_of_an_installed_file_is_held_and_left_in_place() {
    let w = World::new("edited");
    w.sync(false, false);
    let before = recorded_hash(&w, GUIDE).unwrap();
    put(&w.installed(), "references/guide.md", "my notes\n");

    let held = w.sync(false, false);
    assert_eq!(ops(&held, Op::Edited), [GUIDE]);
    assert_eq!(held.plan.count(Op::Update), 0);
    assert_eq!(held.plan.held().count(), 1);
    assert_eq!(
        fs::read_to_string(w.installed().join("references/guide.md")).unwrap(),
        "my notes\n"
    );
    assert!(!w.home.0.join(".mcpm-backups").exists());
    assert_eq!(recorded_hash(&w, GUIDE).unwrap(), before);

    let again = w.sync(false, false);
    assert_eq!(ops(&again, Op::Edited), [GUIDE]);
}

#[test]
fn test19_a_held_edit_is_backed_up_before_the_confirmed_sync_replaces_it() {
    let w = World::new("edited-apply");
    w.sync(false, false);
    put(&w.installed(), "references/guide.md", "my notes\n");
    w.sync(false, false);

    let applied = w.sync(true, false);
    assert_eq!(ops(&applied, Op::Edited), [GUIDE]);
    assert_eq!(applied.plan.held().count(), 0);
    assert_eq!(
        fs::read_to_string(w.installed().join("references/guide.md")).unwrap(),
        "guide\n"
    );
    let backups = tree(&w.home.0.join(".mcpm-backups"));
    let (_, saved) = backups
        .iter()
        .find(|(k, _)| k.contains("guide.md."))
        .expect("the edit is in the backup folder");
    assert_eq!(saved, b"my notes\n");
    assert_eq!(w.sync(false, false).plan.held().count(), 0);
}

#[test]
fn test20_an_edit_that_matches_the_library_copy_is_not_held() {
    let w = World::new("edited-same");
    w.sync(false, false);
    put(&w.installed(), "references/guide.md", "guide v2\n");
    put(&w.repo.0, "skills/nacht/references/guide.md", "guide v2\n");
    let result = w.sync(false, false);
    assert_eq!(result.plan.count(Op::Edited), 0);
    assert_eq!(result.plan.held().count(), 0);
    assert_ne!(recorded_hash(&w, GUIDE).unwrap(), "");
}

#[test]
fn test21_a_lock_without_hashes_updates_a_changed_file_and_adopts_hashes() {
    let w = World::new("old-lock");
    w.sync(false, false);
    let mut lock = load_lockfile(&w.home.0).unwrap();
    lock.skills[0].1.file_hashes.clear();
    save_lockfile(&w.home.0, &lock).unwrap();
    assert!(!fs::read_to_string(w.home.0.join(super::LOCKFILE_NAME))
        .unwrap()
        .contains("file_hashes"));
    put(&w.installed(), "references/guide.md", "my notes\n");

    let result = w.sync(false, false);
    assert_eq!(ops(&result, Op::Update), [GUIDE]);
    assert_eq!(result.plan.count(Op::Edited), 0);
    assert_eq!(result.plan.held().count(), 0);
    assert_eq!(
        load_lockfile(&w.home.0).unwrap().skills[0].1.file_hashes[0]
            .1
            .len(),
        5
    );
}

#[test]
fn test22_a_removal_of_an_edited_file_keeps_a_copy_and_a_plain_one_does_not() {
    let w = World::new("edited-gone");
    w.sync(false, false);
    put(&w.installed(), "references/guide.md", "my notes\n");
    fs::remove_file(w.repo.0.join("skills/nacht/references/guide.md")).unwrap();
    fs::remove_file(w.repo.0.join("skills/nacht/modules/a.md")).unwrap();

    let held = w.sync(false, false);
    assert_eq!(ops(&held, Op::Remove).len(), 2);
    assert_eq!(held.plan.held().count(), 2);
    assert!(w.installed().join("references/guide.md").exists());
    assert!(recorded_hash(&w, GUIDE).is_some());

    w.sync(true, false);
    assert!(!w.installed().join("references").exists());
    assert!(!w.installed().join("modules").exists());
    let backups = tree(&w.home.0.join(".mcpm-backups"));
    let names: Vec<&String> = backups.keys().filter(|k| k.contains("guide.md.")).collect();
    assert_eq!(names.len(), 1);
    assert!(!backups.keys().any(|k| k.contains("a.md.")));
}

#[test]
fn test23_verification_names_a_file_changed_by_hand_since_the_sync() {
    let w = World::new("verify-edited");
    w.sync(false, false);
    let registry = registry_with_home(Some(w.home.0.clone()));
    let skills = discover_skills(&w.repo.0);
    fs::write(w.installed().join("SETUP.md"), "stale").unwrap();
    let lock = load_lockfile(&w.home.0).unwrap();
    let report = verify_installed(&skills, &lock, &registry, &w.home.0);
    assert_eq!(report.edited.len(), 1);
    assert_eq!(report.edited[0].path, ".claude/skills/nacht/SETUP.md");

    put(&w.repo.0, "skills/nacht/SETUP.md", "# setup v2\n");
    let skills = discover_skills(&w.repo.0);
    fs::write(w.installed().join("SETUP.md"), "# setup\n").unwrap();
    let report = verify_installed(&skills, &lock, &registry, &w.home.0);
    assert_eq!(report.differing.len(), 1);
    assert!(report.edited.is_empty());
}

#[test]
fn test24_lint_names_a_skill_that_cannot_be_read_and_why() {
    let w = World::new("unparsable");
    put(
        &w.repo.0,
        "skills/broken/SKILL.md",
        "no frontmatter at all\n",
    );
    put(
        &w.repo.0,
        "skills/hooked/SKILL.md",
        "---\nname: hooked\ndescription: \"Has a hook that points nowhere. Use when testing lint.\"\nhooks:\n  Stop:\n    command: scripts/gone.sh\n---\n\n# Hooked\n",
    );
    put(&w.repo.0, "skills/empty/readme.md", "x");
    let found = super::parser::discover_skills_report(&w.repo.0);
    assert_eq!(found.skills.len(), 1);
    let result = super::lint::lint_parse_failures(&found.failures);
    let errors: Vec<_> = result.errors().collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(errors[0].name, "broken");
    assert!(errors[0].message.contains("skills/broken/SKILL.md"));
    assert!(errors[0].message.contains("No YAML frontmatter"));
    assert_eq!(errors[1].name, "hooked");
    assert!(errors[1].message.contains("scripts/gone.sh"));
}
