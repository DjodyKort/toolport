//! `toolportctl skills sync` without `--client`: it writes to the clients the scope's lock holds,
//! else to claude-code, and the lock it writes keeps that choice. mcpm only synced the clients it
//! was pointed at, so a bare sync must never fan out to every client.

use super::agents_golden_tests::copy_dir;
use super::run_with;
use crate::plus::skills::api::{self, Args};
use crate::plus::testutil::DataDirFx;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

struct Fx {
    _base: DataDirFx,
    root: PathBuf,
    home: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = DataDirFx::with_data_subdir("ctl-skills-scope", tag, "data");
        let root = base.dir.canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let basic = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/skills-collisions/repos/basic");
        copy_dir(&basic, &root.join("repo"));
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        Self {
            _base: base,
            root,
            home,
        }
    }

    fn repo(&self) -> String {
        self.root.join("repo").to_string_lossy().into_owned()
    }

    fn lock_file(&self, project: bool) -> PathBuf {
        let dir = if project {
            self.root.join("repo")
        } else {
            self.root.join("data")
        };
        dir.join("mcpm-skills.lock")
    }

    fn sync(&self, extra: &[&str]) -> (i32, String) {
        let mut list: Vec<String> = ["skills", "sync", "--repo", &self.repo()]
            .iter()
            .map(|s| s.to_string())
            .collect();
        list.extend(extra.iter().map(|s| s.to_string()));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&list, &mut out, &mut err);
        let mut text = String::from_utf8(out).unwrap();
        text.push_str(&String::from_utf8(err).unwrap());
        (code, text)
    }

    fn json(&self, extra: &[&str]) -> Value {
        let mut list: Vec<String> = ["--json", "skills", "sync", "--repo", &self.repo()]
            .iter()
            .map(|s| s.to_string())
            .collect();
        list.extend(extra.iter().map(|s| s.to_string()));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&list, &mut out, &mut err);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{text}{}", String::from_utf8_lossy(&err));
        serde_json::from_str::<Value>(text.trim()).unwrap()["data"].clone()
    }

    fn home_dirs(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.home)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn locked_clients(&self, project: bool) -> BTreeSet<String> {
        let text = std::fs::read_to_string(self.lock_file(project)).unwrap();
        let lock: Value = serde_json::from_str(&text).unwrap();
        ["skills", "rules"]
            .iter()
            .flat_map(|bucket| lock[bucket].as_object().into_iter().flatten())
            .flat_map(|(_, entry)| entry["clients_synced"].as_array().into_iter().flatten())
            .map(|c| c.as_str().unwrap().to_string())
            .collect()
    }

    fn wipe_home(&self) {
        std::fs::remove_dir_all(&self.home).unwrap();
        std::fs::create_dir_all(&self.home).unwrap();
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn set(clients: &[&str]) -> BTreeSet<String> {
    clients.iter().map(|c| c.to_string()).collect()
}

fn targeted(data: &Value) -> Vec<String> {
    data["targetedClients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_first_sync_without_client_writes_to_claude_code_only() {
    let fx = Fx::new("first");

    let data = fx.json(&[]);

    assert_eq!(targeted(&data), ["claude-code"]);
    assert_eq!(data["clientSource"], "default");
    assert_eq!(data["clientCount"], 1);
    assert_eq!(fx.home_dirs(), [".claude"]);
    assert_eq!(fx.locked_clients(false), set(&["claude-code"]));
    assert!(fx
        .home
        .join(".claude/skills/code-review/SKILL.md")
        .is_file());
}

#[test]
fn the_text_says_which_clients_it_picked_and_why() {
    let fx = Fx::new("text");

    let (code, first) = fx.sync(&[]);
    assert_eq!(code, 0, "{first}");
    assert!(
        first
            .contains("Clients: claude-code (no lock lists clients yet; --client chooses others)."),
        "{first}"
    );

    let (_, second) = fx.sync(&[]);
    assert!(
        second.contains(
            "Clients: claude-code (the clients of the existing lock; --client chooses others)."
        ),
        "{second}"
    );

    let (_, asked) = fx.sync(&["--client", "claude-code"]);
    assert!(!asked.contains("Clients:"), "{asked}");
}

#[test]
fn the_clients_of_an_explicit_sync_become_the_default_of_the_next_one() {
    let fx = Fx::new("kept");
    let asked = fx.json(&["--client", "cursor"]);
    assert_eq!(asked["clientSource"], "requested");
    assert_eq!(targeted(&asked), ["cursor"]);
    assert_eq!(fx.locked_clients(false), set(&["cursor"]));
    fx.wipe_home();

    let again = fx.json(&[]);

    assert_eq!(targeted(&again), ["cursor"]);
    assert_eq!(again["clientSource"], "lock");
    assert_eq!(fx.locked_clients(false), set(&["cursor"]));
    assert!(
        !fx.home_dirs().contains(&".claude".to_string()),
        "{:?}",
        fx.home_dirs()
    );
    assert!(!fx.home_dirs().is_empty());
}

#[test]
fn several_clients_in_the_lock_all_stay() {
    let fx = Fx::new("several");
    fx.json(&["--client", "cursor", "--client", "claude-code"]);
    fx.wipe_home();

    let again = fx.json(&[]);

    assert_eq!(
        targeted(&again).into_iter().collect::<BTreeSet<_>>(),
        set(&["claude-code", "cursor"])
    );
    assert_eq!(again["clientSource"], "lock");
    assert_eq!(fx.locked_clients(false), set(&["claude-code", "cursor"]));
}

#[test]
fn a_dry_run_picks_the_default_but_keeps_nothing() {
    let fx = Fx::new("dry");

    let preview = fx.json(&["--dry-run"]);
    assert_eq!(targeted(&preview), ["claude-code"]);
    assert!(!fx.lock_file(false).exists());
    assert!(fx.home_dirs().is_empty());

    let asked = fx.json(&["--client", "cursor", "--dry-run"]);
    assert_eq!(asked["clientSource"], "requested");
    assert!(!fx.lock_file(false).exists());

    let real = fx.json(&[]);
    assert_eq!(targeted(&real), ["claude-code"]);
    assert_eq!(real["clientSource"], "default");
}

#[test]
fn the_project_scope_keeps_its_own_choice() {
    let fx = Fx::new("scopes");
    fx.json(&["--project", "--client", "cursor"]);
    assert!(fx.lock_file(true).is_file());
    assert!(!fx.lock_file(false).exists());

    let global = fx.json(&[]);
    assert_eq!(targeted(&global), ["claude-code"]);
    assert_eq!(global["clientSource"], "default");

    let project = fx.json(&["--project"]);
    assert_eq!(targeted(&project), ["cursor"]);
    assert_eq!(project["clientSource"], "lock");
    assert_eq!(fx.locked_clients(true), set(&["cursor"]));
    assert_eq!(fx.locked_clients(false), set(&["claude-code"]));
}

#[test]
fn a_lock_that_names_no_known_client_starts_over_with_claude_code() {
    let fx = Fx::new("unknown");
    fx.json(&[]);
    let text = std::fs::read_to_string(fx.lock_file(false)).unwrap();
    std::fs::write(
        fx.lock_file(false),
        text.replace("\"claude-code\"", "\"retired-client\""),
    )
    .unwrap();
    fx.wipe_home();

    let again = fx.json(&[]);

    assert_eq!(targeted(&again), ["claude-code"]);
    assert_eq!(again["clientSource"], "default");
    assert_eq!(fx.locked_clients(false), set(&["claude-code"]));
}

#[test]
fn the_self_mcp_tool_and_the_ipc_handler_share_the_default() {
    let fx = Fx::new("surfaces");
    let args = json!({"repo_path": fx.repo(), "client_keys": []});

    let data = api::sync(&Args::from_json(&args)).unwrap();

    assert_eq!(data["targetedClients"], json!(["claude-code"]));
    assert_eq!(data["clientSource"], "default");
    assert_eq!(fx.home_dirs(), [".claude"]);

    let named = api::sync(&Args::from_json(
        &json!({"repo_path": fx.repo(), "client_keys": ["cursor"]}),
    ))
    .unwrap();
    assert_eq!(named["clientSource"], "requested");
    assert_eq!(fx.locked_clients(false), set(&["cursor"]));
}
