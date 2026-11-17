use super::api::{self, Args, TEST_REPO};
use super::auto;
use super::lock::{lockfile_path, LOCKFILE_NAME};
use super::snapshot::{self, KEEP};
use crate::fsx::{self, Paths, PathsGuard};
use crate::op::{ErrorKind, OpError};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use toolport_plus_api::memory::MemoryHost;
use toolport_plus_api::{FakeTaskHost, ProcessOutcome};

/// 2026-11-17T12:00:00Z
const AT: i64 = 1_794_916_800;
const SKILL_MD: &str = "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\n---\n\n# Nacht\n\nSee [setup](SETUP.md).\n";
const HOOKED_MD: &str = "---\nname: nacht\ndescription: \"Runs night jobs. Use when work should continue unattended.\"\nhooks:\n  Stop:\n    command: scripts/stop.sh\n---\n\n# Nacht\n";

struct Env {
    repo: TempDir,
    home: TempDir,
    data: TempDir,
    host: MemoryHost,
    _paths: PathsGuard,
}

impl Env {
    fn new() -> Self {
        let (repo, home, data) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let paths = fsx::enter(Paths {
            home: Some(home.path().to_path_buf()),
            data_dir: Some(data.path().to_path_buf()),
        });
        let root = repo.path().to_path_buf();
        TEST_REPO.with(|r| *r.borrow_mut() = Some(root));
        put(repo.path(), "skills/nacht/SKILL.md", SKILL_MD);
        put(repo.path(), "skills/nacht/SETUP.md", "# setup\n");
        put(repo.path(), "skills/nacht/references/guide.md", "guide\n");
        put(repo.path(), "skills/nacht/modules/a.md", "a\n");
        Env {
            repo,
            home,
            data,
            host: MemoryHost::default(),
            _paths: paths,
        }
    }

    fn lib(&self, rel: &str, body: &str) {
        put(self.repo.path(), &format!("skills/nacht/{rel}"), body);
    }

    fn installed(&self, rel: &str) -> PathBuf {
        self.home.path().join(".claude/skills/nacht").join(rel)
    }

    fn args(&self) -> Args {
        Args::default()
    }

    fn sync_with(&self, args: Args) -> Result<Value, OpError> {
        api::sync(&args, Some(&self.host))
    }

    fn sync(&self) -> Value {
        self.sync_with(self.args()).unwrap()
    }

    fn apply(&self) -> Value {
        self.sync_with(Args {
            apply_held: true,
            ..self.args()
        })
        .unwrap()
    }

    fn unattended(&self) -> Result<Value, OpError> {
        self.sync_with(Args {
            unattended: true,
            ..self.args()
        })
    }

    fn undo(&self, dry_run: bool) -> Result<Value, OpError> {
        api::undo(
            &Args {
                dry_run,
                ..self.args()
            },
            Some(&self.host),
        )
    }

    fn lock_bytes(&self) -> Option<Vec<u8>> {
        fs::read(lockfile_path(self.data.path())).ok()
    }

    fn settings(&self) -> PathBuf {
        self.home.path().join(".claude/settings.json")
    }

    fn snapshots(&self) -> usize {
        snapshot::count(self.data.path())
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        TEST_REPO.with(|r| *r.borrow_mut() = None);
    }
}

fn put(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn actions(data: &Value) -> Vec<(String, String)> {
    data["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["path"].as_str().unwrap().to_string(),
                r["action"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn held(data: &Value) -> Vec<(String, String)> {
    data["held"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["path"].as_str().unwrap().to_string(),
                r["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn test01_a_sync_keeps_a_snapshot_and_undo_removes_what_it_created() {
    let env = Env::new();
    let done = env.sync();
    assert_eq!(done["snapshot"]["files"], 4);
    assert_eq!(done["verify"]["matched"], 4);
    assert_eq!(done["verify"]["total"], 4);
    assert_eq!(env.snapshots(), 1);

    let preview = env.undo(true).unwrap();
    assert_eq!(preview["removed"], 4);
    assert_eq!(preview["lock"]["action"], "remove");
    assert!(env.installed("SKILL.md").is_file());
    assert!(!auto::read(env.data.path()).unwrap().paused);

    let undone = env.undo(false).unwrap();
    assert_eq!(undone["removed"], 4);
    assert_eq!(undone["conflicts"], 0);
    assert!(!env.installed("SKILL.md").exists());
    assert!(!env.home.path().join(".claude/skills/nacht").exists());
    assert!(!lockfile_path(env.data.path()).exists());
    let state = auto::read(env.data.path()).unwrap();
    assert!(state.paused);
    assert_eq!(state.reason.as_deref(), Some(auto::REASON_UNDO));

    let again = env.undo(false).unwrap_err();
    assert_eq!(again.kind, ErrorKind::NotFound);
    assert!(again.message.contains("no snapshot to undo"));
}

#[test]
fn test02_undo_restores_overwritten_and_removed_files_with_their_content_and_mode() {
    let env = Env::new();
    env.lib("scripts/run.sh", "#!/bin/sh\necho one\n");
    set_mode(&env.repo.path().join("skills/nacht/scripts/run.sh"), 0o755);
    env.sync();
    let lock_before = env.lock_bytes();
    let script_before = fs::read(env.installed("scripts/run.sh")).unwrap();

    env.lib("SETUP.md", "# setup, edited\n");
    env.lib("scripts/run.sh", "#!/bin/sh\necho two\n");
    set_mode(&env.repo.path().join("skills/nacht/scripts/run.sh"), 0o700);
    fs::remove_file(env.repo.path().join("skills/nacht/references/guide.md")).unwrap();
    let second = env.apply();
    assert_eq!(second["snapshot"]["files"], 3);
    assert!(!env.installed("references/guide.md").exists());
    assert_eq!(mode(&env.installed("scripts/run.sh")), 0o700);

    let undone = env.undo(false).unwrap();
    assert_eq!(undone["restored"], 3);
    assert_eq!(undone["conflicts"], 0);
    assert_eq!(
        fs::read_to_string(env.installed("SETUP.md")).unwrap(),
        "# setup\n"
    );
    assert_eq!(
        fs::read_to_string(env.installed("references/guide.md")).unwrap(),
        "guide\n"
    );
    assert_eq!(
        fs::read(env.installed("scripts/run.sh")).unwrap(),
        script_before
    );
    assert_eq!(mode(&env.installed("scripts/run.sh")), 0o755);
    assert_eq!(env.lock_bytes(), lock_before);
}

#[test]
fn test03_a_file_changed_after_the_sync_is_skipped_and_listed_as_a_conflict() {
    let env = Env::new();
    env.sync();
    fs::write(env.installed("SETUP.md"), "mine\n").unwrap();

    let preview = env.undo(true).unwrap();
    assert_eq!(preview["conflicts"], 1);
    let undone = env.undo(false).unwrap();
    assert_eq!(undone["conflicts"], 1);
    assert_eq!(undone["removed"], 3);
    assert_eq!(
        fs::read_to_string(env.installed("SETUP.md")).unwrap(),
        "mine\n"
    );
    assert!(!env.installed("SKILL.md").exists());
    assert!(actions(&undone)
        .iter()
        .any(|(path, action)| path.ends_with("SETUP.md") && action == "conflict"));
}

#[test]
fn test04_undo_puts_the_hooks_value_back_and_leaves_other_settings_alone() {
    let env = Env::new();
    put(
        env.home.path(),
        ".claude/settings.json",
        "{\"theme\": \"dark\"}",
    );
    env.lib("SKILL.md", HOOKED_MD);
    env.lib("scripts/stop.sh", "#!/bin/sh\n");
    set_mode(&env.repo.path().join("skills/nacht/scripts/stop.sh"), 0o755);
    let applied = env.apply();
    assert_eq!(applied["snapshot"]["hooks"], 1);
    let after: Value = serde_json::from_str(&fs::read_to_string(env.settings()).unwrap()).unwrap();
    assert!(after.get("hooks").is_some());

    env.undo(false).unwrap();
    let restored: Value =
        serde_json::from_str(&fs::read_to_string(env.settings()).unwrap()).unwrap();
    assert_eq!(restored, json!({"theme": "dark"}));
}

#[test]
fn test05_hooks_changed_since_the_sync_refuse_the_whole_undo() {
    let env = Env::new();
    env.lib("SKILL.md", HOOKED_MD);
    env.lib("scripts/stop.sh", "#!/bin/sh\n");
    set_mode(&env.repo.path().join("skills/nacht/scripts/stop.sh"), 0o755);
    env.apply();
    let mut settings: Value =
        serde_json::from_str(&fs::read_to_string(env.settings()).unwrap()).unwrap();
    settings["hooks"]["Stop"]
        .as_array_mut()
        .unwrap()
        .push(json!({"matcher": "", "hooks": [{"type": "command", "command": "/mine.sh"}]}));
    fs::write(env.settings(), serde_json::to_string(&settings).unwrap()).unwrap();

    let preview = env.undo(true).unwrap();
    assert!(preview["refused"]
        .as_str()
        .unwrap()
        .contains("changed since the sync"));
    let error = env.undo(false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert!(env.installed("SKILL.md").is_file());
    assert!(env.installed("scripts/stop.sh").is_file());
    assert!(!auto::read(env.data.path()).unwrap().paused);
}

#[test]
fn test06_only_the_newest_snapshots_stay() {
    let env = Env::new();
    for n in 0..KEEP + 2 {
        env.lib("SETUP.md", &format!("# setup {n}\n"));
        env.sync();
    }
    assert_eq!(env.snapshots(), KEEP);
    let newest = snapshot::newest(env.data.path()).unwrap();
    let preview = env.undo(true).unwrap();
    assert_eq!(preview["snapshot"]["id"], newest);
}

#[test]
fn test07_a_sync_that_changes_nothing_keeps_no_snapshot() {
    let env = Env::new();
    env.sync();
    let again = env.sync();
    assert_eq!(again["snapshot"], Value::Null);
    assert_eq!(env.snapshots(), 1);
    assert_eq!(again["verify"]["matched"], 4);
}

#[test]
fn test08_a_dry_run_keeps_no_snapshot_and_reports_no_verification() {
    let env = Env::new();
    let preview = env
        .sync_with(Args {
            dry_run: true,
            ..env.args()
        })
        .unwrap();
    assert_eq!(preview["snapshot"], Value::Null);
    assert_eq!(preview["verify"], Value::Null);
    assert_eq!(env.snapshots(), 0);
    assert!(!env.installed("SKILL.md").exists());
}

#[test]
fn test09_an_unattended_run_applies_new_and_tracked_changes_and_holds_the_rest() {
    let env = Env::new();
    put(env.home.path(), ".claude/skills/nacht/SETUP.md", "mine\n");
    let first = env.unattended().unwrap();
    assert_eq!(first["unattended"], true);
    assert_eq!(
        held(&first),
        [(
            ".claude/skills/nacht/SETUP.md".to_string(),
            "differs".to_string()
        )]
    );
    assert_eq!(
        fs::read_to_string(env.installed("SETUP.md")).unwrap(),
        "mine\n"
    );
    assert!(env.installed("modules/a.md").is_file());

    env.lib("modules/a.md", "a, edited\n");
    fs::remove_file(env.repo.path().join("skills/nacht/references/guide.md")).unwrap();
    env.lib("SKILL.md", HOOKED_MD);
    env.lib("scripts/stop.sh", "#!/bin/sh\n");
    set_mode(&env.repo.path().join("skills/nacht/scripts/stop.sh"), 0o755);
    let second = env.unattended().unwrap();
    assert_eq!(
        fs::read_to_string(env.installed("modules/a.md")).unwrap(),
        "a, edited\n"
    );
    assert!(env.installed("references/guide.md").is_file());
    let mut reasons: Vec<String> = held(&second)
        .into_iter()
        .map(|(_, reason)| reason)
        .collect();
    reasons.sort();
    assert_eq!(reasons, ["differs", "new-hook", "removal"]);
    assert!(!env.settings().exists());

    let manual = env.apply();
    assert!(held(&manual).is_empty());
    assert!(!env.installed("references/guide.md").exists());
    assert_eq!(
        fs::read_to_string(env.installed("SETUP.md")).unwrap(),
        "# setup\n"
    );
}

#[test]
fn test10_a_paused_unattended_run_does_nothing_and_says_so() {
    let env = Env::new();
    auto::pause_op(&super::SystemClock).unwrap();
    let skipped = env.unattended().unwrap();
    assert_eq!(skipped["paused"], true);
    assert_eq!(skipped["skipped"], true);
    assert_eq!(skipped["reason"], auto::REASON_MANUAL);
    assert!(skipped["message"]
        .as_str()
        .unwrap()
        .contains("paused since"));
    assert!(!env.installed("SKILL.md").exists());
    assert!(!lockfile_path(env.data.path()).exists());

    let manual = env.sync();
    assert_eq!(manual["snapshot"]["files"], 4);

    auto::resume_op().unwrap();
    assert_eq!(env.unattended().unwrap()["paused"], false);
}

#[test]
fn test11_an_unattended_run_refuses_the_flags_that_apply_held_changes() {
    let env = Env::new();
    for args in [
        Args {
            unattended: true,
            apply_held: true,
            ..env.args()
        },
        Args {
            unattended: true,
            migrate: Some(true),
            ..env.args()
        },
    ] {
        let error = env.sync_with(args).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Usage);
    }
    assert!(!env.installed("SKILL.md").exists());
}

#[test]
fn test12_pause_and_resume_report_whether_they_changed_anything() {
    let env = Env::new();
    assert_eq!(auto::status_op().unwrap()["paused"], false);
    let first = auto::pause_op(&super::SystemClock).unwrap();
    assert_eq!(
        (first["paused"].clone(), first["changed"].clone()),
        (json!(true), json!(true))
    );
    assert_eq!(
        auto::pause_op(&super::SystemClock).unwrap()["changed"],
        false
    );
    assert_eq!(auto::resume_op().unwrap()["changed"], true);
    assert_eq!(auto::resume_op().unwrap()["changed"], false);

    put(env.data.path(), "plus/skills-auto.json", "{ nope");
    let error = auto::status_op().unwrap_err();
    assert!(error.message.contains("is not valid"));
    assert!(env.unattended().is_err());
}

#[test]
fn test13_verify_names_a_missing_file_a_differing_file_and_a_changed_mode() {
    let env = Env::new();
    env.lib("scripts/run.sh", "#!/bin/sh\n");
    set_mode(&env.repo.path().join("skills/nacht/scripts/run.sh"), 0o755);
    env.sync();
    let clean = api::verify(&env.args()).unwrap();
    assert_eq!(
        (clean["matched"].clone(), clean["total"].clone()),
        (json!(5), json!(5))
    );
    assert_eq!(clean["clean"], true);

    fs::remove_file(env.installed("references/guide.md")).unwrap();
    fs::write(env.installed("SETUP.md"), "mine\n").unwrap();
    set_mode(&env.installed("scripts/run.sh"), 0o644);
    let dirty = api::verify(&env.args()).unwrap();
    assert_eq!(dirty["matched"], 2);
    assert_eq!(dirty["clean"], false);
    let mut rows: Vec<(String, String)> = dirty["mismatched"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["reason"].as_str().unwrap().to_string(),
                r["path"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        [
            (
                "differs".to_string(),
                ".claude/skills/nacht/SETUP.md".to_string()
            ),
            (
                "missing".to_string(),
                ".claude/skills/nacht/references/guide.md".to_string()
            ),
            (
                "mode".to_string(),
                ".claude/skills/nacht/scripts/run.sh".to_string()
            ),
        ]
    );
}

#[test]
fn test14_a_changed_executable_bit_is_repaired_by_the_next_sync() {
    let env = Env::new();
    env.lib("scripts/run.sh", "#!/bin/sh\n");
    set_mode(&env.repo.path().join("skills/nacht/scripts/run.sh"), 0o755);
    env.sync();
    set_mode(&env.installed("scripts/run.sh"), 0o644);
    let repaired = env.sync();
    assert_eq!(repaired["plan"]["counts"]["update"], 1);
    assert_eq!(mode(&env.installed("scripts/run.sh")), 0o755);
    assert_eq!(repaired["verify"]["clean"], true);
}

#[test]
fn test15_verify_without_a_lock_checks_nothing_and_is_not_clean() {
    let env = Env::new();
    let none = api::verify(&env.args()).unwrap();
    assert_eq!(none["lockfilePresent"], false);
    assert_eq!(none["total"], 0);
    assert_eq!(none["clean"], false);
    assert!(!env.data.path().join(LOCKFILE_NAME).exists());
}

#[test]
fn test16_undo_after_a_sync_that_changed_one_file_restores_only_that_file() {
    let env = Env::new();
    env.sync();
    env.lib("SETUP.md", "# setup, edited\n");
    let second = env.sync();
    assert_eq!(second["snapshot"]["files"], 1);
    let undone = env.undo(false).unwrap();
    assert_eq!(undone["restored"], 1);
    assert_eq!(undone["removed"], 0);
    assert_eq!(
        fs::read_to_string(env.installed("SETUP.md")).unwrap(),
        "# setup\n"
    );
    assert!(env.installed("SKILL.md").is_file());
}

#[test]
fn test17_undo_puts_back_a_file_the_sync_replaced_as_a_collision() {
    let env = Env::new();
    put(env.home.path(), ".claude/commands/nacht.md", "my command\n");
    let done = env
        .sync_with(Args {
            migrate: Some(true),
            ..env.args()
        })
        .unwrap();
    assert_eq!(done["replaced"], 1);
    assert!(!env.home.path().join(".claude/commands/nacht.md").exists());

    env.undo(false).unwrap();
    assert_eq!(
        fs::read_to_string(env.home.path().join(".claude/commands/nacht.md")).unwrap(),
        "my command\n"
    );
}

#[test]
fn test18_undo_restores_a_binary_file_byte_for_byte() {
    let env = Env::new();
    let bytes = [0u8, 159, 146, 150, 255, 10];
    let path = env.repo.path().join("skills/nacht/assets/blob.bin");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    env.sync();
    fs::write(&path, [1u8, 2, 3]).unwrap();
    env.sync();
    env.undo(false).unwrap();
    assert_eq!(fs::read(env.installed("assets/blob.bin")).unwrap(), bytes);
}

#[test]
fn test19_an_unattended_run_holds_a_tracked_file_that_differs_when_the_lock_has_no_hash_for_it() {
    let env = Env::new();
    env.sync();
    let mut lock = super::lock::load_lockfile(env.data.path()).unwrap();
    for (_, entry) in &mut lock.skills {
        entry.file_hashes.clear();
    }
    super::lock::save_lockfile(env.data.path(), &lock).unwrap();
    fs::write(env.installed("SETUP.md"), "mine\n").unwrap();
    env.lib("SETUP.md", "# setup, edited\n");

    let unattended = env.unattended().unwrap();
    assert_eq!(
        held(&unattended),
        [(
            ".claude/skills/nacht/SETUP.md".to_string(),
            "edited".to_string()
        )]
    );
    assert_eq!(
        fs::read_to_string(env.installed("SETUP.md")).unwrap(),
        "mine\n"
    );

    env.sync();
    assert_eq!(
        fs::read_to_string(env.installed("SETUP.md")).unwrap(),
        "# setup, edited\n"
    );
}

fn last_run(_env: &Env) -> Value {
    auto::status_op().unwrap()["lastRun"].clone()
}

fn reasons(run: &Value) -> Vec<String> {
    let mut reasons: Vec<String> = run["held"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["reason"].as_str().unwrap().to_string())
        .collect();
    reasons.sort();
    reasons
}

#[test]
fn test20_an_unattended_run_records_what_it_applied_and_held() {
    let env = Env::new();
    assert_eq!(last_run(&env), Value::Null);
    put(env.home.path(), ".claude/skills/nacht/SETUP.md", "mine\n");
    env.unattended().unwrap();
    let run = last_run(&env);
    assert_eq!(run["applied"], 3);
    assert_eq!(run["skipped"], 0);
    assert_eq!(reasons(&run), ["differs"]);
    assert!(run["at"].as_str().unwrap().ends_with('Z'));
    assert!(run.get("error").is_none());
}

#[test]
fn test21_a_paused_or_dry_unattended_run_records_nothing() {
    let env = Env::new();
    env.sync_with(Args {
        unattended: true,
        dry_run: true,
        ..env.args()
    })
    .unwrap();
    assert_eq!(last_run(&env), Value::Null);
    auto::pause_op(&super::SystemClock).unwrap();
    env.unattended().unwrap();
    assert_eq!(last_run(&env), Value::Null);
}

#[test]
fn test22_a_manual_apply_clears_what_it_applied_from_the_held_list_and_keeps_the_rest() {
    let env = Env::new();
    put(env.home.path(), ".claude/skills/nacht/SETUP.md", "mine\n");
    env.unattended().unwrap();
    env.lib("modules/a.md", "a, edited\n");
    fs::remove_file(env.repo.path().join("skills/nacht/references/guide.md")).unwrap();
    env.unattended().unwrap();
    assert_eq!(reasons(&last_run(&env)), ["differs", "removal"]);

    env.sync();
    assert_eq!(reasons(&last_run(&env)), ["differs", "removal"]);

    env.apply();
    assert_eq!(last_run(&env)["held"], json!([]));
    assert_eq!(last_run(&env)["applied"], 1);
}

#[test]
fn test23_a_manual_sync_for_some_clients_leaves_the_held_list_alone() {
    let env = Env::new();
    put(env.home.path(), ".claude/skills/nacht/SETUP.md", "mine\n");
    env.unattended().unwrap();
    env.sync_with(Args {
        apply_held: true,
        clients: Some(vec!["claude-code".into()]),
        ..env.args()
    })
    .unwrap();
    assert_eq!(reasons(&last_run(&env)), ["differs"]);
}

#[test]
fn test24_a_failed_unattended_run_records_the_error_and_keeps_what_was_held() {
    let env = Env::new();
    put(env.home.path(), ".claude/skills/nacht/SETUP.md", "mine\n");
    env.unattended().unwrap();
    let error = env
        .sync_with(Args {
            unattended: true,
            clients: Some(vec!["cursor".into()]),
            ..env.args()
        })
        .unwrap_err();
    let run = last_run(&env);
    assert_eq!(run["error"], json!(error.message));
    assert_eq!(run["applied"], 0);
    assert_eq!(reasons(&run), ["differs"]);
}

#[test]
fn test25_pause_and_resume_keep_the_last_run() {
    let env = Env::new();
    put(env.home.path(), ".claude/skills/nacht/SETUP.md", "mine\n");
    env.unattended().unwrap();
    auto::pause_op(&super::SystemClock).unwrap();
    assert_eq!(reasons(&last_run(&env)), ["differs"]);
    auto::resume_op().unwrap();
    assert_eq!(reasons(&last_run(&env)), ["differs"]);
}

#[test]
fn test26_the_schedule_is_the_shipped_task_off_until_it_is_turned_on() {
    let env = Env::new();
    let host = FakeTaskHost::at(env.data.path().to_path_buf(), AT);
    let off = auto::schedule_json(&host);
    assert_eq!(off["task"], "skills-auto-sync");
    assert_eq!(off["enabled"], false);
    assert_eq!(off["cron"], "30 6 * * *");
    assert_eq!(off["when"], "daily 06:30 UTC");
    assert_eq!(off["autoRun"], true);
    assert_eq!(off["nextRun"], Value::Null);

    let on = auto::switch_op(&host, true, false).unwrap();
    assert_eq!(on["schedule"]["enabled"], true);
    assert_eq!(on["schedule"]["nextRun"], "2026-11-18T06:30:00Z");
    assert_eq!(on["result"]["undo"], "toolportctl skills auto off");
    assert_eq!(
        auto::full_status_op(&host).unwrap()["schedule"]["enabled"],
        true
    );

    let off_again = auto::switch_op(&host, false, false).unwrap();
    assert_eq!(off_again["schedule"]["enabled"], false);
    assert_eq!(off_again["result"]["undo"], "toolportctl skills auto on");
}

#[test]
fn test27_a_dry_run_switch_changes_nothing_and_a_removed_task_is_reported() {
    let env = Env::new();
    let host = FakeTaskHost::at(env.data.path().to_path_buf(), AT);
    let preview = auto::switch_op(&host, true, true).unwrap();
    assert_eq!(preview["dryRun"], true);
    assert_eq!(preview["schedule"]["enabled"], false);
    assert_eq!(preview["plan"]["undo"], "toolportctl skills auto off");

    auto::switch_op(&host, true, false).unwrap();
    crate::tasks::store::remove_task(env.data.path(), auto::TASK_ID).unwrap();
    let gone = auto::schedule_json(&host);
    assert_eq!(gone["present"], false);
    assert_eq!(gone["enabled"], false);
}

#[test]
fn test28_the_shipped_task_runs_the_unattended_sync_through_the_bundled_ctl() {
    let env = Env::new();
    let host = FakeTaskHost::at(env.data.path().to_path_buf(), AT);
    auto::switch_op(&host, true, false).unwrap();
    host.process_results
        .borrow_mut()
        .push_back(ProcessOutcome::Exited {
            code: Some(0),
            output: String::new(),
        });
    let started = crate::tasks::api::run(auto::TASK_ID, "manual", false, &host).unwrap();
    let run_id = started["run"]["id"].as_str().unwrap();
    let run = crate::tasks::runner::execute(&host, env.data.path(), run_id).unwrap();
    assert_eq!(run.status, crate::tasks::store::Status::Ok);
    let spec = host.processes.borrow()[0].clone();
    assert_eq!(spec.program, crate::tasks::proc::CTL_NAME);
    assert_eq!(spec.args, ["skills", "sync", "--unattended"]);
}

#[test]
fn test29_the_shipped_task_is_started_by_its_schedule_and_not_by_a_client() {
    let env = Env::new();
    let task = crate::tasks::store::load_task(env.data.path(), auto::TASK_ID).unwrap();
    assert!(crate::tasks::triggers::auto_runs(&task));
    assert!(!task.triggers.self_mcp.enabled);
    assert_eq!(crate::tasks::model::validate(&task), Ok(()));
}
