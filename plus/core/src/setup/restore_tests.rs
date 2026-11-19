//! Restore and the reset check against two synthetic homes: one exports, the other restores.

use serde_json::{json, Value};

use super::check;
use super::export;
use super::restore::{self, RestoreOptions};
use super::tests::{options, World};

fn exported(from: &World) -> std::path::PathBuf {
    let data = export::run(&from.ctx(), &options(from), None).unwrap();
    std::path::PathBuf::from(data["file"].as_str().unwrap())
}

fn plan(to: &World, file: &std::path::Path, take: &[&str]) -> Value {
    let opts = RestoreOptions {
        file: file.into(),
        take: take.iter().map(|t| t.to_string()).collect(),
        ..Default::default()
    };
    restore::run(&to.ctx(), &opts).unwrap()
}

fn row<'a>(plan: &'a Value, id: &str) -> &'a Value {
    plan["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no row {id} in {plan}"))
}

fn apply(to: &World, file: &std::path::Path, take: &[&str], hash: &str) -> Value {
    let opts = RestoreOptions {
        file: file.into(),
        take: take.iter().map(|t| t.to_string()).collect(),
        apply: true,
        plan: Some(hash.into()),
        ..Default::default()
    };
    restore::run(&to.ctx(), &opts).unwrap()
}

#[test]
fn test01_a_restore_starts_as_a_dry_run_that_writes_nothing() {
    let from = World::new();
    from.repo("web");
    from.claude("CLAUDE.md", "global rules\n");
    let file = exported(&from);
    let to = World::new();
    let before = to.snapshot();
    let data = plan(&to, &file, &[]);
    to.assert_unchanged(&before);
    assert_eq!(data["dryRun"], true);
    assert_eq!(row(&data, "repo:~/code/web")["state"], "add");
    assert_eq!(row(&data, "claude:CLAUDE.md")["state"], "add");
    assert_eq!(row(&data, "server:alpha")["state"], "here");
}

#[test]
fn test02_apply_needs_the_plan_hash_and_refuses_when_this_mac_changed() {
    let from = World::new();
    from.repo("web");
    from.claude("CLAUDE.md", "from the file\n");
    let file = exported(&from);
    let to = World::new();
    let data = plan(&to, &file, &[]);
    let hash = data["plan"].as_str().unwrap().to_string();
    let missing = RestoreOptions {
        file: file.clone(),
        apply: true,
        ..Default::default()
    };
    assert!(restore::run(&to.ctx(), &missing).is_err());
    to.put(
        &to.home.join(".claude/CLAUDE.md"),
        "written after the plan\n",
    );
    let opts = RestoreOptions {
        file,
        apply: true,
        plan: Some(hash),
        ..Default::default()
    };
    let stale = restore::run(&to.ctx(), &opts).unwrap_err();
    assert!(
        stale.message.contains("changed since the plan"),
        "{}",
        stale.message
    );
}

#[test]
fn test03_apply_clones_missing_repositories_and_undo_removes_them_again() {
    let from = World::new();
    from.repo("web");
    from.claude("CLAUDE.md", "global rules\n");
    let file = exported(&from);
    let to = World::new();
    let hash = plan(&to, &file, &[])["plan"].as_str().unwrap().to_string();
    let done = apply(&to, &file, &[], &hash);
    assert_eq!(done["failed"], 0, "{done}");
    assert!(to.home.join("code/web/README.md").exists());
    assert_eq!(
        std::fs::read_to_string(to.home.join(".claude/CLAUDE.md")).unwrap(),
        "global rules\n"
    );
    let undone = restore::undo(&to.ctx(), None, false).unwrap();
    assert!(!to.home.join("code/web").exists(), "{undone}");
    assert!(!to.home.join(".claude/CLAUDE.md").exists());
    assert!(restore::undo(&to.ctx(), None, false).is_err());
}

#[test]
fn test04_a_differing_file_is_kept_unless_taken_and_undo_puts_it_back() {
    let from = World::new();
    from.claude("CLAUDE.md", "from the file\n");
    let file = exported(&from);
    let to = World::new();
    to.claude("CLAUDE.md", "mine\n");
    let kept = plan(&to, &file, &[]);
    assert_eq!(row(&kept, "claude:CLAUDE.md")["state"], "conflict");
    let hash = kept["plan"].as_str().unwrap().to_string();
    apply(&to, &file, &[], &hash);
    let path = to.home.join(".claude/CLAUDE.md");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine\n");

    let taken = plan(&to, &file, &["claude:CLAUDE.md"]);
    let hash = taken["plan"].as_str().unwrap().to_string();
    apply(&to, &file, &["claude:CLAUDE.md"], &hash);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "from the file\n");
    restore::undo(&to.ctx(), None, false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine\n");
}

#[test]
fn test04b_only_the_conflicts_a_take_can_change_are_marked_takeable() {
    let from = World::new();
    from.repo("web");
    from.claude("CLAUDE.md", "from the file\n");
    let file = exported(&from);
    let to = World::new();
    let web = to.repo("web");
    let remote = from.dir.path().join("remotes/web.git");
    to.git(
        &web,
        &["remote", "set-url", "origin", &remote.to_string_lossy()],
    );
    to.git(&web, &["checkout", "-q", "-b", "other"]);
    to.claude("CLAUDE.md", "mine\n");
    let data = plan(&to, &file, &[]);
    let takeable = |id: &str| row(&data, id)["takeable"].as_bool().unwrap();
    assert_eq!(row(&data, "claude:CLAUDE.md")["state"], "conflict");
    assert!(takeable("claude:CLAUDE.md"));
    assert_eq!(row(&data, "repo:~/code/web")["state"], "conflict");
    assert!(!takeable("repo:~/code/web"));
    assert!(!takeable("server:alpha"));
    let taken = plan(&to, &file, &["repo:~/code/web"]);
    assert_eq!(row(&taken, "repo:~/code/web")["state"], "conflict");
    assert_eq!(
        std::fs::read_to_string(to.home.join(".claude/CLAUDE.md")).unwrap(),
        "mine\n"
    );
}

#[test]
fn test05_undo_keeps_a_file_that_changed_after_the_restore() {
    let from = World::new();
    from.claude("CLAUDE.md", "from the file\n");
    let file = exported(&from);
    let to = World::new();
    let hash = plan(&to, &file, &[])["plan"].as_str().unwrap().to_string();
    apply(&to, &file, &[], &hash);
    let path = to.home.join(".claude/CLAUDE.md");
    std::fs::write(&path, "edited since\n").unwrap();
    restore::undo(&to.ctx(), None, false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "edited since\n");
}

#[test]
fn test06_the_reset_check_lists_work_that_exists_only_here_and_never_writes_to_a_repository() {
    let world = World::new();
    world.messy_repo();
    world.no_remote_repo("scratch");
    let before = world.snapshot();
    let opts = check::Options {
        roots: vec![world.home.join("code")],
        tick: vec![],
        untick: vec![],
        ack: vec![],
    };
    let data = check::run(&world.ctx(), &opts).unwrap();
    world.assert_unchanged(&before);
    let kinds: Vec<&str> = data["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"unpushed") && kinds.contains(&"no-remote"),
        "{kinds:?}"
    );
    assert_eq!(data["ready"], false);
}

#[test]
fn test07_ticks_and_acknowledgements_survive_and_a_clean_world_is_ready_once_exported() {
    let world = World::new();
    world.repo("web");
    export::run(&world.ctx(), &options(&world), None).unwrap();
    let opts = |tick: &[&str]| check::Options {
        roots: vec![world.home.join("code")],
        tick: tick.iter().map(|t| t.to_string()).collect(),
        untick: vec![],
        ack: vec![],
    };
    let first = check::run(&world.ctx(), &opts(&["browser", "keys"])).unwrap();
    assert_eq!(first["ready"], false);
    let last = check::run(&world.ctx(), &opts(&["licenses", "outside"])).unwrap();
    assert_eq!(last["ready"], true, "{last}");
    assert!(check::run(&world.ctx(), &opts(&["nonsense"])).is_err());
}

fn journal_dir(to: &World) -> std::path::PathBuf {
    to.data.join("plus/setup/runs")
}

fn step<'a>(undone: &'a Value, what_ends_with: &str) -> &'a Value {
    undone["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["what"].as_str().unwrap().ends_with(what_ends_with))
        .unwrap_or_else(|| panic!("no step {what_ends_with} in {undone}"))
}

#[test]
fn test08_an_undo_step_that_fails_is_named_and_keeps_the_run_open_for_a_second_try() {
    let from = World::new();
    from.claude("CLAUDE.md", "from the file\n");
    from.claude("agents/a.md", "agent from the file\n");
    let file = exported(&from);
    let to = World::new();
    to.claude("CLAUDE.md", "mine\n");
    let take = ["claude:CLAUDE.md"];
    let hash = plan(&to, &file, &take)["plan"]
        .as_str()
        .unwrap()
        .to_string();
    let done = apply(&to, &file, &take, &hash);
    let run = done["run"].as_str().unwrap().to_string();
    let backup = journal_dir(&to).join(&run).join("backup/0");
    let saved = std::fs::read(&backup).unwrap();
    std::fs::remove_file(&backup).unwrap();

    let first = restore::undo(&to.ctx(), None, false).unwrap();
    assert_eq!(first["undone"], false, "{first}");
    assert_eq!(first["failed"], json!(["~/.claude/CLAUDE.md"]));
    let failed = step(&first, "CLAUDE.md");
    assert_eq!(
        (failed["ok"].clone(), failed["result"].clone()),
        (json!(false), json!("failed"))
    );
    assert!(
        failed["note"].as_str().unwrap().contains("cannot be read"),
        "{first}"
    );
    assert!(!to.home.join(".claude/agents/a.md").exists());
    assert_eq!(step(&first, "a.md")["result"], "done");
    assert_eq!(
        std::fs::read_to_string(to.home.join(".claude/CLAUDE.md")).unwrap(),
        "from the file\n"
    );

    std::fs::write(&backup, saved).unwrap();
    let second = restore::undo(&to.ctx(), None, false).unwrap();
    assert_eq!(second["undone"], true, "{second}");
    assert_eq!(second["failed"], json!([]));
    assert_eq!(step(&second, "a.md")["note"], "undone by an earlier try");
    assert_eq!(
        std::fs::read_to_string(to.home.join(".claude/CLAUDE.md")).unwrap(),
        "mine\n"
    );
    assert!(restore::undo(&to.ctx(), None, false).is_err());
}

#[cfg(unix)]
#[test]
fn test09_a_file_that_cannot_be_removed_is_a_failure_not_a_success() {
    use std::os::unix::fs::PermissionsExt;

    let from = World::new();
    from.claude("CLAUDE.md", "from the file\n");
    let file = exported(&from);
    let to = World::new();
    let hash = plan(&to, &file, &[])["plan"].as_str().unwrap().to_string();
    apply(&to, &file, &[], &hash);
    let dir = to.home.join(".claude");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let probe = dir.join("probe");
    if std::fs::write(&probe, "x").is_ok() {
        std::fs::remove_file(&probe).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let blocked = restore::undo(&to.ctx(), None, false).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(blocked["undone"], false, "{blocked}");
    assert_eq!(step(&blocked, "CLAUDE.md")["result"], "failed");
    assert!(dir.join("CLAUDE.md").exists());
    let again = restore::undo(&to.ctx(), None, false).unwrap();
    assert_eq!(again["undone"], true, "{again}");
    assert!(!dir.join("CLAUDE.md").exists());
}

#[test]
fn test10_what_was_kept_on_purpose_is_not_a_failure() {
    let from = World::new();
    from.claude("CLAUDE.md", "from the file\n");
    let file = exported(&from);
    let to = World::new();
    let hash = plan(&to, &file, &[])["plan"].as_str().unwrap().to_string();
    apply(&to, &file, &[], &hash);
    std::fs::write(to.home.join(".claude/CLAUDE.md"), "edited since\n").unwrap();
    let undone = restore::undo(&to.ctx(), None, false).unwrap();
    assert_eq!(undone["undone"], true, "{undone}");
    assert_eq!(undone["failed"], json!([]));
    assert_eq!(step(&undone, "CLAUDE.md")["result"], "kept");
}

#[test]
fn test11_a_dry_run_of_undo_changes_nothing_and_never_says_undone() {
    let from = World::new();
    from.claude("CLAUDE.md", "from the file\n");
    let file = exported(&from);
    let to = World::new();
    let hash = plan(&to, &file, &[])["plan"].as_str().unwrap().to_string();
    apply(&to, &file, &[], &hash);
    let before = to.snapshot();
    let preview = restore::undo(&to.ctx(), None, true).unwrap();
    to.assert_unchanged(&before);
    assert_eq!(preview["undone"], false);
    assert_eq!(step(&preview, "CLAUDE.md")["result"], "done");
}

#[test]
fn test12_a_run_killed_after_any_step_can_still_be_undone() {
    let from = World::new();
    from.repo("web");
    from.claude("CLAUDE.md", "global rules\n");
    from.claude("agents/a.md", "an agent\n");
    let file = exported(&from);
    for stop in 0..3 {
        let to = World::new();
        let hash = plan(&to, &file, &[])["plan"].as_str().unwrap().to_string();
        let opts = RestoreOptions {
            file: file.clone(),
            apply: true,
            plan: Some(hash),
            ..Default::default()
        };
        let killed = restore::run_stopping_after(&to.ctx(), &opts, Some(stop)).unwrap();
        assert_eq!(killed["stopped"], stop, "{killed}");
        let run = killed["run"].as_str().unwrap();
        assert!(journal_dir(&to).join(format!("{run}.json")).exists());
        assert_eq!(
            to.home.join("code/web").exists(),
            stop >= 1,
            "the clone is the first step"
        );
        assert_eq!(to.home.join(".claude/CLAUDE.md").exists(), stop >= 2);
        assert!(!to.home.join(".claude/agents/a.md").exists());

        let undone = restore::undo(&to.ctx(), None, false).unwrap();
        assert_eq!(undone["undone"], true, "stop {stop}: {undone}");
        assert!(!to.home.join("code/web").exists(), "stop {stop}");
        assert!(!to.home.join(".claude/CLAUDE.md").exists(), "stop {stop}");
        assert!(!to.home.join(".claude/agents/a.md").exists(), "stop {stop}");
    }
}

#[test]
fn test13_the_journal_grows_with_every_step_not_at_the_end() {
    let from = World::new();
    from.claude("CLAUDE.md", "global rules\n");
    from.claude("agents/a.md", "an agent\n");
    let file = exported(&from);
    let to = World::new();
    let hash = plan(&to, &file, &[])["plan"].as_str().unwrap().to_string();
    let opts = RestoreOptions {
        file,
        apply: true,
        plan: Some(hash),
        ..Default::default()
    };
    let mut touched = Vec::new();
    for stop in 0..=2 {
        let killed = restore::run_stopping_after(&to.ctx(), &opts, Some(stop)).unwrap();
        let run = killed["run"].as_str().unwrap();
        let journal: Value = serde_json::from_slice(
            &std::fs::read(journal_dir(&to).join(format!("{run}.json"))).unwrap(),
        )
        .unwrap();
        touched.push(journal["files"].as_array().unwrap().len());
        restore::undo(&to.ctx(), None, false).unwrap();
    }
    assert_eq!(touched, [0, 1, 2]);
}

#[cfg(feature = "cipher")]
mod sealed {
    use super::*;
    use crate::setup::tests::{PASS, SECRET_VALUE};
    use crate::setup::PartId;

    fn sealed_file(from: &World) -> std::path::PathBuf {
        let mut opts = options(from);
        opts.parts.insert(PartId::Secrets);
        let data = export::run(&from.ctx(), &opts, Some(PASS)).unwrap();
        std::path::PathBuf::from(data["file"].as_str().unwrap())
    }

    fn empty_target() -> World {
        let to = World::new();
        to.host.registry.servers.borrow_mut().clear();
        to.host.secrets.vault.borrow_mut().clear();
        to
    }

    fn opts(file: &std::path::Path, passphrase: Option<&str>) -> RestoreOptions {
        RestoreOptions {
            file: file.into(),
            passphrase: passphrase.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn test14_without_the_passphrase_the_secrets_stay_locked_and_nothing_is_stored() {
        let from = World::new();
        let file = sealed_file(&from);
        let to = empty_target();
        let data = restore::run(&to.ctx(), &opts(&file, None)).unwrap();
        assert_eq!(row(&data, "secrets")["state"], "needs");
        assert_eq!(data["locked"], true);
        assert!(!format!("{data}").contains(SECRET_VALUE));
    }

    #[test]
    fn test15_a_wrong_passphrase_is_refused_before_anything_changes() {
        let from = World::new();
        let file = sealed_file(&from);
        let to = empty_target();
        let err = restore::run(&to.ctx(), &opts(&file, Some("not the passphrase"))).unwrap_err();
        assert!(err.message.contains("does not open"), "{}", err.message);
        assert!(to.host.secrets.vault.borrow().is_empty());
    }

    #[test]
    fn test16_the_right_passphrase_stores_secrets_only_for_servers_added_in_this_run() {
        let from = World::new();
        let file = sealed_file(&from);
        let to = empty_target();
        let dry = restore::run(&to.ctx(), &opts(&file, Some(PASS))).unwrap();
        assert_eq!(row(&dry, "secrets")["state"], "add");
        assert!(!format!("{dry}").contains(SECRET_VALUE));
        assert!(to.host.secrets.vault.borrow().is_empty());
        let mut apply = opts(&file, Some(PASS));
        apply.apply = true;
        apply.plan = Some(dry["plan"].as_str().unwrap().into());
        restore::run(&to.ctx(), &apply).unwrap();
        let vault = to.host.secrets.vault.borrow();
        assert_eq!(vault.values().collect::<Vec<_>>(), vec![SECRET_VALUE]);
    }

    #[test]
    fn test17_a_server_that_was_already_here_keeps_its_own_secret() {
        let from = World::new();
        let file = sealed_file(&from);
        let to = World::new();
        to.host
            .secrets
            .vault
            .borrow_mut()
            .insert(("srv-alpha".into(), "API_KEY".into()), "mine".into());
        let dry = restore::run(&to.ctx(), &opts(&file, Some(PASS))).unwrap();
        let mut apply = opts(&file, Some(PASS));
        apply.apply = true;
        apply.plan = Some(dry["plan"].as_str().unwrap().into());
        restore::run(&to.ctx(), &apply).unwrap();
        let vault = to.host.secrets.vault.borrow();
        assert_eq!(vault.values().collect::<Vec<_>>(), vec!["mine"]);
    }
}
