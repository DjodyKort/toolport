use super::claude_json::Known;
use super::clear::{self, Args as ClearArgs};
use super::overview::{self, Options};
use super::resolve::{project_slug, Repo, SetBy};
use super::session_ops::{self, ClearArgs as SessionsClear};
use super::store::{self, Dest};
use super::switch::{self, Position};
use super::{instructions, World};
use crate::context::fsx;
use crate::context::leave_out;
use crate::op::ErrorKind;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use toolport_plus_api::MemoryHost;

const DAY: i64 = 86_400;
const NOW: i64 = 1_800_000_000;

fn count(text: &str) -> u64 {
    text.len() as u64
}

struct Fx {
    _dir: tempfile::TempDir,
    home: PathBuf,
    data: PathBuf,
    managed: Option<PathBuf>,
    env: Option<String>,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = fsx::canonical(dir.path());
        let home = base.join("home");
        let data = base.join("data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        Self {
            _dir: dir,
            home,
            data,
            managed: None,
            env: None,
        }
    }

    fn world(&self) -> World<'_> {
        World {
            home: &self.home,
            data_dir: &self.data,
            now: NOW,
            count_tokens: &count,
            managed_settings: self.managed.as_deref(),
            policy_claude_md: None,
            env_disable: self.env.clone(),
        }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    fn put(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.at(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn repo(&self, rel: &str) -> PathBuf {
        let path = self.at(rel);
        std::fs::create_dir_all(path.join(".git/info")).unwrap();
        path
    }

    fn worktree(&self, main: &str, rel: &str) -> PathBuf {
        let name = Path::new(rel).file_name().unwrap();
        let registered = self.at(main).join(".git/worktrees").join(name);
        std::fs::create_dir_all(&registered).unwrap();
        let wt = self.at(rel);
        self.put(
            &format!("{rel}/.git"),
            &format!("gitdir: {}\n", registered.display()),
        );
        std::fs::write(registered.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            registered.join("gitdir"),
            format!("{}\n", wt.join(".git").display()),
        )
        .unwrap();
        wt
    }

    fn memory(&self, repo: &str, files: &[(&str, &str)]) -> PathBuf {
        let slug = project_slug(&self.at(repo));
        let dir = self.at(&format!(".claude/projects/{slug}/memory"));
        for (name, text) in files {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    fn session(&self, folder: &str, id: &str, text: &str, age_days: i64) -> PathBuf {
        let slug = project_slug(&self.at(folder));
        let path = self.at(&format!(".claude/projects/{slug}/{id}.jsonl"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs((NOW - age_days * DAY) as u64);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
        path
    }

    fn known(&self, folders: &[&str]) {
        let projects: serde_json::Map<String, Value> = folders
            .iter()
            .map(|f| (fsx::display(&self.at(f)), serde_json::json!({})))
            .collect();
        self.put(
            ".claude.json",
            &serde_json::json!({"projects": projects, "oauthAccount": {"secret": "never read"}})
                .to_string(),
        );
    }
}

fn ok(result: Result<Value, crate::op::OpError>) -> Value {
    result.unwrap_or_else(|e| panic!("{e:?}"))
}

fn err(result: Result<Value, crate::op::OpError>) -> crate::op::OpError {
    result.unwrap_err()
}

#[test]
fn the_winner_is_named_in_the_documented_order() {
    let fx = Fx::new();
    let repo_dir = fx.repo("work/app");
    fx.put(".claude/settings.json", "{\"autoMemoryEnabled\": false}");
    let repo = Repo::of(&repo_dir);
    let mut layers = fx.world().layers(&repo);
    assert_eq!(layers.effective(), (false, SetBy::Global));
    fx.put(
        "work/app/.claude/settings.json",
        "{\"autoMemoryEnabled\": true}",
    );
    layers = fx.world().layers(&repo);
    assert_eq!(layers.effective(), (true, SetBy::Project));
    fx.put(
        "work/app/.claude/settings.local.json",
        "{\"autoMemoryEnabled\": false}",
    );
    layers = fx.world().layers(&repo);
    assert_eq!(layers.effective(), (false, SetBy::Repository));
    let mut fx = fx;
    fx.env = Some("0".into());
    assert_eq!(
        fx.world().layers(&repo).effective(),
        (true, SetBy::Environment)
    );
    fx.env = Some("1".into());
    assert_eq!(
        fx.world().layers(&repo).effective(),
        (false, SetBy::Environment)
    );
    let managed = fx.put("managed.json", "{\"autoMemoryEnabled\": true}");
    fx.managed = Some(managed);
    assert_eq!(fx.world().layers(&repo).effective(), (true, SetBy::Managed));
}

#[test]
fn the_memory_folder_follows_auto_memory_directory_from_local_or_user_settings_only() {
    let fx = Fx::new();
    let repo = Repo::of(&fx.repo("work/app"));
    let default = fx.at(&format!(".claude/projects/{}/memory", repo.slug()));
    assert_eq!(
        fx.world().layers(&repo).directory(&fx.home, &repo),
        (default, false)
    );
    fx.put(
        "work/app/.claude/settings.json",
        &format!(
            "{{\"autoMemoryDirectory\": \"{}\"}}",
            fx.at("elsewhere").display()
        ),
    );
    assert!(!fx.world().layers(&repo).directory(&fx.home, &repo).1);
    fx.put(
        ".claude/settings.json",
        "{\"autoMemoryDirectory\": \"~/notes\"}",
    );
    assert_eq!(
        fx.world().layers(&repo).directory(&fx.home, &repo),
        (fx.at("notes"), true)
    );
}

#[test]
fn off_from_a_subfolder_is_written_at_the_root_and_a_preview_writes_nothing() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    fx.memory("work/app", &[("MEMORY.md", "- a note that costs tokens\n")]);
    let sub = root.join("src");
    std::fs::create_dir_all(&sub).unwrap();
    let preview = ok(switch::set_repository(
        &fx.world(),
        &sub,
        Position::Off,
        true,
    ));
    assert!(!root.join(".claude/settings.local.json").exists());
    assert_eq!(preview["previous"], "default");
    assert!(
        preview["plan"]["effects"]["tokens"]["after"].as_u64()
            < preview["plan"]["effects"]["tokens"]["before"].as_u64()
    );
    assert!(preview["plan"]["warnings"]
        .to_string()
        .contains("not verified"));

    let done = ok(switch::set_repository(
        &fx.world(),
        &sub,
        Position::Off,
        false,
    ));
    assert_eq!(done["state"]["enabled"], false);
    assert!(
        std::fs::read_to_string(root.join(".claude/settings.local.json"))
            .unwrap()
            .contains("\"autoMemoryEnabled\": false")
    );
    assert_eq!(
        done["result"]["undo"],
        format!("toolportctl memory set default --cwd {}", sub.display())
    );
    let back = ok(switch::set_repository(
        &fx.world(),
        &sub,
        Position::Default,
        false,
    ));
    assert_eq!(back["previous"], "off");
    assert!(!root.join(".claude/settings.local.json").exists());
}

#[test]
fn on_warns_when_the_global_default_is_already_on_and_keeps_a_file_owned_keys_alone() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    let done = ok(switch::set_repository(
        &fx.world(),
        &root,
        Position::On,
        true,
    ));
    assert!(done["plan"]["warnings"]
        .to_string()
        .contains("global default is on"));
    fx.put(".claude/settings.json", "{\"autoMemoryEnabled\": false}");
    let done = ok(switch::set_repository(
        &fx.world(),
        &root,
        Position::On,
        true,
    ));
    assert!(!done["plan"]["warnings"]
        .to_string()
        .contains("global default is on"));
}

#[test]
fn a_hand_written_switch_is_replaced_and_put_back_by_undo() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    let file = fx.put(
        "work/app/.claude/settings.local.json",
        "{\"autoMemoryEnabled\": true}\n",
    );
    let done = ok(switch::set_repository(
        &fx.world(),
        &root,
        Position::Off,
        false,
    ));
    assert!(done["plan"]["warnings"].to_string().contains("by hand"));
    assert!(std::fs::read_to_string(&file).unwrap().contains("false"));
    ok(switch::set_repository(
        &fx.world(),
        &root,
        Position::Default,
        false,
    ));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{\"autoMemoryEnabled\": true}\n"
    );
}

#[test]
fn managed_settings_refuse_a_repository_switch() {
    let mut fx = Fx::new();
    let root = fx.repo("work/app");
    fx.managed = Some(fx.put("managed.json", "{\"autoMemoryEnabled\": false}"));
    let refused = err(switch::set_repository(
        &fx.world(),
        &root,
        Position::On,
        false,
    ));
    assert_eq!(refused.kind, ErrorKind::Conflict);
    assert!(refused.message.contains("managed settings"));
}

#[test]
fn the_global_default_is_written_to_the_user_settings_and_default_removes_it() {
    let fx = Fx::new();
    fx.repo("work/app");
    fx.put(
        "work/app/.claude/settings.local.json",
        "{\"autoMemoryEnabled\": false}",
    );
    fx.known(&["work/app"]);
    let store = MemoryHost::default();
    let preview = ok(switch::set_global(&fx.world(), &store, Position::Off, true));
    assert!(!fx.at(".claude/settings.json").exists());
    assert_eq!(preview["overrides"], 1);
    assert!(preview["plan"]["warnings"]
        .to_string()
        .contains("1 repository has its own switch"));
    let done = ok(switch::set_global(
        &fx.world(),
        &store,
        Position::Off,
        false,
    ));
    assert_eq!(
        done["result"]["undo"],
        "toolportctl memory set default --global"
    );
    assert!(std::fs::read_to_string(fx.at(".claude/settings.json"))
        .unwrap()
        .contains("false"));
    ok(switch::set_global(
        &fx.world(),
        &store,
        Position::Default,
        false,
    ));
    assert!(!std::fs::read_to_string(fx.at(".claude/settings.json"))
        .unwrap()
        .contains("autoMemoryEnabled"));
}

fn clear_args(cwd: &Path) -> ClearArgs<'_> {
    ClearArgs {
        cwd,
        dest: Dest::Trash,
        confirm: None,
        also_off: false,
        dry_run: false,
    }
}

#[test]
fn clear_lists_the_files_first_then_moves_the_folder_to_the_trash_and_undo_puts_it_back() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    let wt = fx.worktree("work/app", "work/wt");
    let dir = fx.memory(
        "work/app",
        &[
            ("MEMORY.md", "- one\n"),
            ("a.md", "---\ntype: user\n---\nx\n"),
        ],
    );
    let mut args = clear_args(&wt);
    args.dry_run = true;
    let plan = ok(clear::clear(&fx.world(), &args));
    assert!(dir.exists());
    assert_eq!(plan["files"].as_array().unwrap().len(), 2);
    assert_eq!(plan["totals"]["count"], 2);
    assert_eq!(plan["sharedBy"].as_array().unwrap().len(), 1);

    let done = ok(clear::clear(&fx.world(), &clear_args(&root)));
    assert!(!dir.exists());
    let id = done["result"]["trashId"].as_str().unwrap().to_string();
    assert_eq!(
        done["result"]["undo"],
        format!("toolportctl memory undo {id}")
    );

    let preview = ok(clear::undo(&fx.world(), None, Some(&wt), true));
    assert!(!dir.exists());
    assert_eq!(preview["id"], id);
    ok(clear::undo(&fx.world(), None, Some(&wt), false));
    assert_eq!(
        std::fs::read_to_string(dir.join("MEMORY.md")).unwrap(),
        "- one\n"
    );
    assert_eq!(
        err(clear::undo(&fx.world(), None, Some(&root), false)).kind,
        ErrorKind::NotFound
    );
}

#[test]
fn an_expired_trash_entry_cannot_be_undone() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    fx.memory("work/app", &[("MEMORY.md", "- one\n")]);
    ok(clear::clear(&fx.world(), &clear_args(&root)));
    let mut world = fx.world();
    world.now = NOW + 31 * DAY;
    assert_eq!(
        err(clear::undo(&world, None, None, false)).kind,
        ErrorKind::NotFound
    );
}

#[test]
fn deleting_for_good_needs_the_repository_name_and_cannot_be_undone() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    let dir = fx.memory("work/app", &[("MEMORY.md", "- one\n")]);
    let mut args = clear_args(&root);
    args.dest = Dest::Forever;
    let refused = err(clear::clear(&fx.world(), &args));
    assert_eq!(refused.kind, ErrorKind::Usage);
    assert!(refused.message.contains("--confirm app"));
    args.confirm = Some("wrong");
    assert_eq!(err(clear::clear(&fx.world(), &args)).kind, ErrorKind::Usage);
    assert!(dir.exists());
    args.confirm = Some("app");
    let done = ok(clear::clear(&fx.world(), &args));
    assert!(!dir.exists());
    assert_eq!(done["result"]["undo"], Value::Null);
    assert_eq!(
        err(clear::undo(&fx.world(), None, None, false)).kind,
        ErrorKind::NotFound
    );
}

#[test]
fn clearing_with_the_off_tick_turns_memory_off_and_undo_turns_it_back() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    fx.memory("work/app", &[("MEMORY.md", "- one\n")]);
    let mut args = clear_args(&root);
    args.also_off = true;
    ok(clear::clear(&fx.world(), &args));
    let layers = fx.world().layers(&Repo::of(&root));
    assert_eq!(layers.effective(), (false, SetBy::Repository));
    ok(clear::undo(&fx.world(), None, Some(&root), false));
    assert_eq!(
        fx.world().layers(&Repo::of(&root)).effective(),
        (true, SetBy::Default)
    );
}

#[test]
fn a_clear_whose_move_fails_leaves_memory_switched_on() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    let memory = fx.memory("work/app", &[("MEMORY.md", "- one\n")]);
    std::fs::write(fx.data.join("plus"), "not a folder").unwrap();
    let mut args = clear_args(&root);
    args.also_off = true;
    let failed = err(clear::clear(&fx.world(), &args));
    assert!(
        failed.message.contains("could not move"),
        "{}",
        failed.message
    );
    assert!(memory.join("MEMORY.md").exists());
    assert_eq!(
        fx.world().layers(&Repo::of(&root)).effective(),
        (true, SetBy::Default)
    );
    assert!(!root.join(".claude/settings.local.json").exists());
}

#[cfg(unix)]
#[test]
fn a_switch_that_cannot_be_written_puts_the_cleared_memory_back() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    let root = fx.repo("work/app");
    let memory = fx.memory("work/app", &[("MEMORY.md", "- one\n")]);
    let settings_dir = root.join(".claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    let set_mode = |mode| {
        std::fs::set_permissions(&settings_dir, std::fs::Permissions::from_mode(mode)).unwrap()
    };
    set_mode(0o555);
    // A superuser writes through the mode, so the test has nothing to show there.
    let writable = std::fs::write(settings_dir.join("probe"), "").is_ok();
    let mut args = clear_args(&root);
    args.also_off = true;
    let result = (!writable).then(|| clear::clear(&fx.world(), &args));
    set_mode(0o755);
    let Some(result) = result else {
        return;
    };
    let failed = err(result);
    assert!(
        failed.message.contains("put back, nothing changed"),
        "{}",
        failed.message
    );
    assert_eq!(
        std::fs::read_to_string(memory.join("MEMORY.md")).unwrap(),
        "- one\n"
    );
    assert!(store::list(&fx.data, store::Store::Trash, NOW).is_empty());
    assert_eq!(
        fx.world().layers(&Repo::of(&root)).effective(),
        (true, SetBy::Default)
    );
}

#[test]
fn nothing_to_clear_is_not_found_and_a_folder_that_holds_the_home_is_refused() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    assert_eq!(
        err(clear::clear(&fx.world(), &clear_args(&root))).kind,
        ErrorKind::NotFound
    );
    fx.put("notes/MEMORY.md", "x");
    fx.put(
        ".claude/settings.json",
        &format!("{{\"autoMemoryDirectory\": \"{}\"}}", fx.home.display()),
    );
    let refused = err(clear::clear(&fx.world(), &clear_args(&root)));
    assert_eq!(refused.kind, ErrorKind::Conflict);
}

fn sessions_fixture(fx: &Fx) -> (PathBuf, PathBuf) {
    let root = fx.repo("work/app");
    let wt = fx.worktree("work/app", "work/wt");
    std::fs::create_dir_all(root.join("src")).unwrap();
    fx.known(&["work/app/src", "elsewhere"]);
    fx.session("work/app", "s-recent", "aaaa", 1);
    fx.session("work/app/src", "s-sub", "bb", 27);
    fx.session("work/wt", "s-wt", "c", 10);
    fx.session("elsewhere", "s-other", "d", 5);
    fx.memory("work/app", &[("MEMORY.md", "m")]);
    (root, wt)
}

#[test]
fn sessions_join_the_worktrees_and_the_subfolders_the_repository_has_run_in() {
    let fx = Fx::new();
    let (_, wt) = sessions_fixture(&fx);
    let list = ok(session_ops::ls(&fx.world(), Some(&wt), false));
    let ids: Vec<&str> = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["s-recent", "s-wt", "s-sub"]);
    assert_eq!(list["totals"]["count"], 3);
    assert_eq!(list["totals"]["bytes"], 7);
    assert_eq!(list["totals"]["expiringThisWeek"], 1);
    assert_eq!(list["sessions"][2]["daysLeft"], 3);
    assert_eq!(list["sessions"][2]["ageDays"], 27);
    assert_eq!(list["retention"]["days"], 30);
}

#[test]
fn clearing_sessions_moves_them_and_undo_and_restore_bring_them_back() {
    let fx = Fx::new();
    let (root, _) = sessions_fixture(&fx);
    let sub_session = fx.at(&format!(
        ".claude/projects/{}/s-sub.jsonl",
        project_slug(&fx.at("work/app/src"))
    ));
    let args = |dest, dry_run| SessionsClear {
        cwd: &root,
        ids: vec!["s-sub".into()],
        older_than: None,
        dest,
        confirm: None,
        dry_run,
    };
    let plan = ok(session_ops::clear(&fx.world(), &args(Dest::Trash, true)));
    assert!(sub_session.exists());
    assert_eq!(plan["totalBytes"], 2);
    let done = ok(session_ops::clear(&fx.world(), &args(Dest::Trash, false)));
    assert!(!sub_session.exists());
    let id = done["result"]["id"].as_str().unwrap();
    assert_eq!(
        done["result"]["undo"],
        format!("toolportctl sessions undo {id}")
    );
    ok(session_ops::undo(&fx.world(), None, Some(&root), false));
    assert!(sub_session.exists());

    let archived = ok(session_ops::clear(&fx.world(), &args(Dest::Archive, false)));
    let archive_id = archived["result"]["id"].as_str().unwrap().to_string();
    assert!(!sub_session.exists());
    let list = ok(session_ops::ls(&fx.world(), Some(&root), false));
    assert_eq!(list["archive"][0]["id"], archive_id);
    assert!(list["archive"][0]["expiresAt"].is_null());
    assert_eq!(
        err(session_ops::undo(
            &fx.world(),
            Some(&archive_id),
            None,
            false
        ))
        .kind,
        ErrorKind::NotFound
    );
    ok(session_ops::restore(&fx.world(), &archive_id, false));
    assert!(sub_session.exists());
}

#[test]
fn older_than_picks_by_age_and_a_missing_selection_or_id_is_refused() {
    let fx = Fx::new();
    let (root, _) = sessions_fixture(&fx);
    let mut args = SessionsClear {
        cwd: &root,
        ids: vec![],
        older_than: Some(7),
        dest: Dest::Trash,
        confirm: None,
        dry_run: true,
    };
    let plan = ok(session_ops::clear(&fx.world(), &args));
    let ids: Vec<&str> = plan["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["s-wt", "s-sub"]);
    args.older_than = None;
    assert_eq!(
        err(session_ops::clear(&fx.world(), &args)).kind,
        ErrorKind::Usage
    );
    args.ids = vec!["nope".into()];
    assert_eq!(
        err(session_ops::clear(&fx.world(), &args)).kind,
        ErrorKind::NotFound
    );
    args.ids = vec!["s-other".into()];
    assert_eq!(
        err(session_ops::clear(&fx.world(), &args)).kind,
        ErrorKind::NotFound
    );
}

#[test]
fn deleting_sessions_for_good_needs_the_name_and_removes_the_files() {
    let fx = Fx::new();
    let (root, _) = sessions_fixture(&fx);
    let file = fx.at(&format!(
        ".claude/projects/{}/s-recent.jsonl",
        project_slug(&fx.at("work/app"))
    ));
    let mut args = SessionsClear {
        cwd: &root,
        ids: vec!["s-recent".into()],
        older_than: None,
        dest: Dest::Forever,
        confirm: None,
        dry_run: false,
    };
    assert_eq!(
        err(session_ops::clear(&fx.world(), &args)).kind,
        ErrorKind::Usage
    );
    assert!(file.exists());
    args.confirm = Some("app");
    ok(session_ops::clear(&fx.world(), &args));
    assert!(!file.exists());
}

#[test]
fn retention_is_read_set_at_least_one_and_unchanged_values_write_nothing() {
    let fx = Fx::new();
    let store = MemoryHost::default();
    let read = ok(session_ops::retention(&fx.world(), &store, None, false));
    assert_eq!(read["retention"]["days"], 30);
    assert_eq!(read["retention"]["source"], "default");
    assert_eq!(
        err(session_ops::retention(&fx.world(), &store, Some(0), false)).kind,
        ErrorKind::Usage
    );
    let preview = ok(session_ops::retention(&fx.world(), &store, Some(90), true));
    assert!(!fx.at(".claude/settings.json").exists());
    assert_eq!(
        preview["plan"]["undo"],
        "toolportctl sessions retention --set 30"
    );
    ok(session_ops::retention(&fx.world(), &store, Some(90), false));
    let read = ok(session_ops::retention(&fx.world(), &store, None, false));
    assert_eq!(
        (
            read["retention"]["days"].as_u64(),
            read["retention"]["source"].as_str()
        ),
        (Some(90), Some("user settings"))
    );
    let again = ok(session_ops::retention(&fx.world(), &store, Some(90), false));
    assert_eq!(again["result"]["changed"].as_array().unwrap().len(), 0);
}

#[test]
fn the_overview_has_a_row_per_repository_and_an_other_row_for_folders_that_match_none() {
    let fx = Fx::new();
    let (_, _) = sessions_fixture(&fx);
    fx.repo("work/quiet");
    fx.put(
        "work/quiet/.claude/settings.local.json",
        "{\"autoMemoryEnabled\": false}",
    );
    fx.put("work/quiet/CLAUDE.local.md", "mine");
    fx.known(&["work/app/src", "work/quiet"]);
    let slug = "-tmp-gone";
    fx.put(&format!(".claude/projects/{slug}/x.jsonl"), "12345");
    let all = |sort: &'static str, filter: Option<&'static str>| {
        ok(overview::overview(
            &fx.world(),
            &Options {
                all: true,
                cwd: None,
                sort,
                filter,
            },
        ))
    };
    let data = all("name", None);
    let names: Vec<&str> = data["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["app", "quiet"]);
    let app = &data["rows"][0];
    assert_eq!(app["sessions"]["count"], 3);
    assert_eq!(app["memory"]["files"], 1);
    assert_eq!(app["worktrees"], 1);
    let quiet = &data["rows"][1];
    assert_eq!(quiet["memory"]["enabled"], false);
    assert_eq!(quiet["memory"]["setBy"], "repository");
    assert_eq!(quiet["instructions"]["flags"][0], "local file");
    assert_eq!(data["other"]["sessions"], 2);
    assert_eq!(data["other"]["folders"], 2);
    assert_eq!(data["totals"]["repositories"], 2);

    let off = all("name", Some("memory-off"));
    assert_eq!(off["rows"].as_array().unwrap().len(), 1);
    assert_eq!(off["rows"][0]["name"], "quiet");
    assert_eq!(all("name", Some("expiring"))["rows"][0]["name"], "app");
    assert_eq!(all("sessions", None)["rows"][0]["name"], "app");
    assert_eq!(
        err(overview::overview(
            &fx.world(),
            &Options {
                all: true,
                cwd: None,
                sort: "name",
                filter: Some("bogus")
            }
        ))
        .kind,
        ErrorKind::Usage
    );
}

#[test]
fn only_the_two_known_facts_are_taken_from_the_claude_json() {
    let fx = Fx::new();
    fx.repo("work/app");
    fx.put(
        ".claude.json",
        &format!(
            "{{\"projects\": {{\"{}\": {{\"hasClaudeMdExternalIncludesApproved\": true, \"mcpServers\": {{\"x\": {{\"env\": {{\"TOKEN\": \"s3cret\"}}}}}}}}}}}}",
            fx.at("work/app").display()
        ),
    );
    let known = Known::read(&fx.home);
    assert_eq!(known.folders, [fx.at("work/app")]);
    assert_eq!(
        known.external_imports_approved(&fx.at("work/app")),
        Some(true)
    );
    assert_eq!(known.external_imports_approved(&fx.at("work")), None);
}

#[test]
fn the_instruction_files_come_in_the_documented_order_with_their_states() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    fx.put(".claude/CLAUDE.md", "user rules");
    fx.put("work/CLAUDE.md", "parent rules");
    fx.put("work/app/CLAUDE.md", "project @docs/a.md");
    fx.put("work/app/docs/a.md", "imported");
    fx.put("work/app/CLAUDE.local.md", "local");
    fx.put(
        "work/app/.claude/rules/r.md",
        "---\npaths: src/**\n---\nlazy",
    );
    fx.put("work/app/AGENTS.md", "agents");
    fx.put("work/app/packages/x/CLAUDE.md", "nested");
    fx.put("work/app/node_modules/dep/CLAUDE.md", "skipped");
    fx.put(
        "work/app/.claude/settings.local.json",
        &format!(
            "{{\"claudeMdExcludes\": [\"{}\"]}}",
            fx.at("work/CLAUDE.md").display()
        ),
    );
    let report = instructions::report(&fx.world(), &Repo::of(&root), &Known::default()).unwrap();
    let rows: Vec<(&str, &str, &str)> = report
        .rows
        .iter()
        .map(|r| {
            (
                r["scope"].as_str().unwrap(),
                r["name"].as_str().unwrap(),
                r["state"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("managed", "CLAUDE.md", "none"),
            ("user", "CLAUDE.md", "loads"),
            ("parent", "CLAUDE.md", "excluded"),
            ("project", "CLAUDE.md", "loads"),
            ("import", "a.md", "loads"),
            ("local", "CLAUDE.local.md", "loads"),
            ("rules", "r.md", "on demand"),
            ("on-demand", "CLAUDE.md", "on demand"),
            ("agents", "AGENTS.md", "not read"),
        ]
    );
    let import = &report.rows[4];
    assert_eq!(import["parent"], fsx::display(&fx.at("work/app/CLAUDE.md")));
    assert_eq!(import["canExclude"], false);
    assert_eq!(report.rows[2]["leftOutBy"], "settings");
    assert_eq!(report.rows[3]["canExclude"], true);
    assert_eq!(report.excluded, 1);
    assert_eq!(report.memory_index_tokens, 0);
    assert_eq!(report.local_files, 1);
    assert_eq!(report.left_out_tokens, "parent rules".len() as u64);
    assert_eq!(
        report.total_tokens,
        ("user rules".len() + "project @docs/a.md".len() + "imported".len() + "local".len()) as u64
    );
}

#[test]
fn a_toolport_exclusion_is_shown_as_toolports_and_agents_md_loads_without_a_claude_md() {
    let fx = Fx::new();
    let root = fx.repo("work/app");
    fx.put("work/app/AGENTS.md", "agents");
    let rows = instructions::report(&fx.world(), &Repo::of(&root), &Known::default())
        .unwrap()
        .rows;
    assert_eq!(rows.last().unwrap()["state"], "loads");
    let file = fx.put("work/app/CLAUDE.md", "p");
    let args = leave_out::Args {
        op: leave_out::Op::Add,
        target: leave_out::Target::File(fsx::display(&file)),
        cwd: &root,
        dry_run: false,
    };
    let count = |t: &str| t.len() as u64;
    let loads = crate::context::loads::InstructionLoads {
        home: &fx.home,
        policy_file: None,
        count_tokens: &count,
    };
    let roots = crate::context::Roots::from_home(&fx.home);
    leave_out::run(
        &leave_out::World {
            roots: &roots,
            loads: &loads,
            data_dir: &fx.data,
        },
        &args,
    )
    .unwrap();
    let rows = instructions::report(&fx.world(), &Repo::of(&root), &Known::default())
        .unwrap()
        .rows;
    let claude = rows
        .iter()
        .find(|r| r["name"] == "CLAUDE.md" && r["source"] == "project")
        .unwrap();
    assert_eq!(
        (claude["state"].as_str(), claude["leftOutBy"].as_str()),
        (Some("excluded"), Some("toolport"))
    );
}
