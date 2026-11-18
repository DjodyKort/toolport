use crate::context::fsx;
use crate::context::launch_profile::{self as lp, AddArgs};
use crate::context::regenerate;
use crate::context::testkit::ScratchDir;
use crate::direct::{self, State};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use toolport_plus_api::{
    convert, AccessSetInfo, MemoryClients, MemoryHost, MemoryRegistry, MemorySecrets, ServerRecord,
};

fn server(id: &str, name: &str) -> ServerRecord {
    convert(&json!({"id": id, "name": name, "enabled": true,
        "transport": "stdio", "command": "run-it"}))
    .unwrap()
}

fn set(id: &str, members: &[&str]) -> AccessSetInfo {
    AccessSetInfo {
        id: id.into(),
        name: id.into(),
        server_ids: members.iter().map(|m| m.to_string()).collect(),
    }
}

struct Fx {
    _scratch: ScratchDir,
    host: MemoryHost,
    home: PathBuf,
}

fn fx(tag: &str) -> Fx {
    let scratch = ScratchDir::new(tag);
    let dir = fsx::canonical(scratch.path());
    let (home, data) = (dir.join("home"), dir.join("data"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&data).unwrap();
    let registry =
        MemoryRegistry::with_servers(vec![server("srv-a", "alpha"), server("srv-b", "beta")]);
    *registry.access_sets.borrow_mut() =
        vec![set("default", &["srv-a", "srv-b"]), set("bare", &["srv-a"])];
    *registry.default_access.borrow_mut() = Some("default".into());
    let host = MemoryHost {
        registry,
        clients: MemoryClients {
            gateway: Some("/opt/toolport/toolport-gateway".into()),
            launcher: Some(std::env::current_exe().unwrap().display().to_string()),
            ..MemoryClients::with_client("claude-code", "Claude Code")
        },
        secrets: MemorySecrets {
            data_dir: Some(data),
            ..Default::default()
        },
        home: Some(home.clone()),
        ..Default::default()
    };
    Fx {
        _scratch: scratch,
        host,
        home,
    }
}

impl Fx {
    fn profile(&self, name: &str, set: &str) {
        lp::add(
            &self.host,
            &AddArgs {
                name,
                set: Some(set),
                deny_direct: None,
                dry_run: false,
            },
        )
        .unwrap();
    }

    fn mcp_file(&self, name: &str) -> PathBuf {
        self.home
            .join(".config/toolport/profiles")
            .join(name)
            .join("mcp.json")
    }

    fn profile_state(&self, name: &str) -> Value {
        lp::ls(&self.host, Some(name), None).unwrap()["profiles"][0]["state"].clone()
    }

    fn entry(&self, server: &str) {
        direct::add(
            &self.host,
            &direct::Args {
                server,
                client: "claude-code",
                ..Default::default()
            },
        )
        .unwrap();
    }

    fn move_launcher(&mut self) {
        let old = self.home.join("bin/toolportctl");
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, "").unwrap();
        let current = self
            .host
            .clients
            .launcher
            .replace(old.display().to_string());
        self.entry("alpha");
        fs::remove_file(old).unwrap();
        self.host.clients.launcher = current;
    }

    fn state(&self) -> State {
        direct::list(&self.host, None).unwrap()[0].state
    }
}

fn rows(report: &Value, part: &str) -> Vec<Value> {
    report[part]["rows"].as_array().unwrap().clone()
}

#[test]
fn test01_a_lost_binding_and_a_stale_file_are_regenerated() {
    let fx = fx("rg01");
    fx.profile("bare", "bare");
    fx.profile("lost", "bare");
    fx.host.registry.client_scopes.borrow_mut().clear();
    fs::write(fx.mcp_file("bare"), "{}").unwrap();
    let report = regenerate::run(&fx.host, false);
    let done = rows(&report, "launchProfiles");
    assert_eq!(done.len(), 2);
    assert!(done.iter().all(|row| row["action"] == "regenerated"));
    assert_eq!(fx.profile_state("bare"), "ok");
    assert_eq!(fx.profile_state("lost"), "ok");
}

#[test]
fn test02_a_dry_run_names_the_rewrites_and_changes_nothing() {
    let fx = fx("rg02");
    fx.profile("bare", "bare");
    fs::write(fx.mcp_file("bare"), "{}").unwrap();
    let report = regenerate::run(&fx.host, true);
    assert_eq!(
        rows(&report, "launchProfiles")[0]["action"],
        "would regenerate"
    );
    assert_eq!(fs::read_to_string(fx.mcp_file("bare")).unwrap(), "{}");
}

#[test]
fn test03_a_profile_without_its_access_set_is_left_for_the_user() {
    let fx = fx("rg03");
    fx.profile("bare", "bare");
    fx.host
        .registry
        .access_sets
        .borrow_mut()
        .retain(|s| s.id != "bare");
    let report = regenerate::run(&fx.host, false);
    let row = &rows(&report, "launchProfiles")[0];
    assert_eq!(row["action"], "left");
    assert!(row["reason"].as_str().unwrap().contains("no longer exists"));
    assert_eq!(
        row["fix"],
        "toolportctl context launch-profile add bare --set <access set>"
    );
}

#[test]
fn test04_a_profile_that_is_up_to_date_is_only_counted() {
    let fx = fx("rg04");
    fx.profile("bare", "bare");
    let report = regenerate::run(&fx.host, false);
    assert!(rows(&report, "launchProfiles").is_empty());
    assert_eq!(report["launchProfiles"]["upToDate"], 1);
}

#[test]
fn test05_a_direct_entry_whose_launcher_moved_is_rewritten() {
    let mut fx = fx("rg05");
    fx.move_launcher();
    assert_eq!(fx.state(), State::Stale);
    let report = regenerate::run(&fx.host, false);
    assert_eq!(rows(&report, "directEntries")[0]["action"], "rewritten");
    assert_eq!(fx.state(), State::Ok);
}

#[test]
fn test06_a_stale_direct_entry_is_untouched_by_a_dry_run() {
    let mut fx = fx("rg06");
    fx.move_launcher();
    let report = regenerate::run(&fx.host, true);
    assert_eq!(rows(&report, "directEntries")[0]["action"], "would rewrite");
    assert_eq!(fx.state(), State::Stale);
}

#[test]
fn test07_an_entry_edited_by_hand_is_left_alone() {
    let fx = fx("rg07");
    fx.entry("alpha");
    fx.host.clients.clients.borrow_mut()[0].entries[0]
        .args
        .push("--mine".into());
    assert_eq!(fx.state(), State::Customized);
    let report = regenerate::run(&fx.host, false);
    let row = &rows(&report, "directEntries")[0];
    assert_eq!(row["action"], "left");
    assert_eq!(
        row["fix"],
        "toolportctl client direct check --client claude-code"
    );
    assert_eq!(fx.state(), State::Customized);
}

#[test]
fn test08_nothing_to_do_reports_empty_lists() {
    let fx = fx("rg08");
    let report = regenerate::run(&fx.host, false);
    assert!(rows(&report, "launchProfiles").is_empty());
    assert!(rows(&report, "directEntries").is_empty());
}
