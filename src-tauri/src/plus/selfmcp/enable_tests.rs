//! The self server is switched on at import and install (D-052) and stays where the user left it.

use super::register::{self, Ensured, Intent, Standing};
use crate::plus::ctl::run_with;
use crate::plus::import_mcpm::{run, Action, Plan, RunOptions};
use crate::plus::registry_ro;
use crate::plus::testutil::DataDirFx;
use crate::registry::{self, Registry};
use serde_json::{json, Value};
use std::path::PathBuf;

const ACTIVE: &str = "default";

struct World {
    fx: DataDirFx,
    root: PathBuf,
    home: PathBuf,
}

impl World {
    fn new(tag: &str) -> Self {
        let fx = DataDirFx::with_data_subdir("selfmcp-enable", tag, "data");
        let root = fx.dir.join("mcpm");
        let home = fx.dir.join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            root.join("servers.json"),
            json!({
                "alpha": {"name": "alpha", "profile_tags": ["work"],
                          "command": "alpha-server", "args": []},
                "beta": {"name": "beta", "command": "beta-server", "args": []}
            })
            .to_string(),
        )
        .unwrap();
        let world = Self { fx, root, home };
        world.client("claude-code", "alpha");
        world.client("cursor", "beta");
        world
    }

    fn client(&self, file: &str, server: &str) {
        std::fs::write(
            self.root.join(format!("{file}.json")),
            json!({"mcpServers": {
                format!("mcpm_{server}"): {"command": "mcpm", "args": ["run", server]}
            }})
            .to_string(),
        )
        .unwrap();
    }

    fn opts(&self, dry_run: bool) -> RunOptions {
        RunOptions {
            root: self.root.clone(),
            home: Some(self.home.to_string_lossy().into_owned()),
            dry_run,
            ..RunOptions::default()
        }
    }

    fn import(&self) -> Plan {
        run(&self.opts(false)).unwrap()
    }

    fn registry_text(&self) -> String {
        std::fs::read_to_string(self.fx.dir.join("data/registry.json")).unwrap_or_default()
    }

    fn reg(&self) -> Registry {
        registry_ro::read().unwrap()
    }

    fn self_id(&self) -> String {
        register::find_self(&self.reg())
            .expect("self server")
            .id
            .clone()
    }

    fn enabled(&self, profile: &str) -> bool {
        let reg = self.reg();
        register::find_self(&reg).is_some_and(|s| reg.is_enabled(profile, &s.id))
    }

    fn switch(&self, profile: &str, on: bool) {
        let id = self.self_id();
        registry::update(|reg| reg.set_server_enabled(profile, &id, on)).unwrap();
    }

    fn ctl(&self, argv: &[&str]) -> (i32, Value) {
        let mut args: Vec<String> = vec!["--json".into()];
        args.extend(argv.iter().map(|s| s.to_string()));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&args, &mut out, &mut err);
        let out = String::from_utf8(out).unwrap();
        (code, serde_json::from_str(out.trim()).expect(&out))
    }

    fn give_binary(&self) {
        let bin = self.fx.dir.join("data/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let file = format!("toolport-selfmcp{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(bin.join(file), "").unwrap();
    }
}

fn self_entries(reg: &Registry) -> usize {
    reg.servers
        .iter()
        .filter(|s| s.source.as_deref() == Some(register::SELF_SOURCE))
        .count()
}

fn check<'a>(doctor: &'a Value, name: &str) -> &'a Value {
    doctor["data"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no check {name}"))
}

#[test]
fn import_enables_it_in_the_active_and_every_client_profile() {
    let w = World::new("import");
    let plan = w.import();
    assert!(plan.changed());
    let reg = w.reg();
    assert_eq!(self_entries(&reg), 1);
    for profile in [ACTIVE, "claude-code", "cursor"] {
        assert!(w.enabled(profile), "{profile}");
    }
    assert!(!w.enabled("work"), "tag profiles are not client scopes");
    assert_eq!(
        register::enabled_in(&reg),
        vec![ACTIVE, "claude-code", "cursor"]
    );
    assert_eq!(register::status(&reg).standing, Standing::Enabled);
}

#[test]
fn second_import_changes_nothing_and_keeps_the_membership() {
    let w = World::new("twice");
    w.import();
    let first = w.registry_text();
    let again = w.import();
    assert!(!again.changed(), "{:?}", again.counts);
    assert_eq!(again.counts.get(&Action::Updated).copied().unwrap_or(0), 0);
    assert_eq!(w.registry_text(), first);
    assert_eq!(self_entries(&w.reg()), 1);
    for profile in [ACTIVE, "claude-code", "cursor"] {
        assert!(w.enabled(profile), "{profile}");
    }
}

#[test]
fn a_changed_mcpm_profile_keeps_the_self_server_in_it() {
    let w = World::new("changed");
    w.import();
    std::fs::write(
        w.root.join("servers.json"),
        json!({
            "alpha": {"name": "alpha", "profile_tags": ["work"],
                      "command": "alpha-server", "args": []},
            "beta": {"name": "beta", "command": "beta-server", "args": []},
            "gamma": {"name": "gamma", "command": "gamma-server", "args": []}
        })
        .to_string(),
    )
    .unwrap();
    w.client("cursor", "gamma");
    let plan = w.import();
    assert!(plan.changed());
    let reg = w.reg();
    let cursor = reg.profiles.iter().find(|p| p.id == "cursor").unwrap();
    assert!(cursor.enabled_server_ids.contains(&"gamma".to_string()));
    assert!(w.enabled("cursor"));
}

#[test]
fn import_never_switches_a_profile_back_on_after_the_user_turned_it_off() {
    let w = World::new("off");
    w.import();
    w.switch("cursor", false);
    w.switch(ACTIVE, false);
    let before = w.registry_text();
    let again = w.import();
    assert!(!again.changed());
    assert_eq!(w.registry_text(), before);
    assert!(!w.enabled("cursor"));
    assert!(!w.enabled(ACTIVE));
    assert!(w.enabled("claude-code"));
    let (outcome, enabled) = {
        let installed = register::install(None, Intent::Ensure).unwrap();
        (installed.outcome, installed.enabled)
    };
    assert_eq!(outcome, Ensured::Unchanged);
    assert!(enabled.is_empty());
    assert!(!w.enabled("cursor"));
    assert_eq!(register::status(&w.reg()).standing, Standing::Disabled);
}

#[test]
fn a_client_profile_that_appears_later_is_switched_on_once() {
    let w = World::new("later");
    w.import();
    w.switch("cursor", false);
    w.client("windsurf", "alpha");
    w.import();
    assert!(w.enabled("windsurf"));
    assert!(!w.enabled("cursor"));
    w.switch("windsurf", false);
    w.import();
    assert!(!w.enabled("windsurf"));
}

#[test]
fn an_entry_registered_before_enabling_existed_is_switched_on() {
    let w = World::new("legacy");
    registry::update(|reg| {
        assert_eq!(
            register::apply_ensure_self_server(reg, "/opt/tp/toolport-selfmcp"),
            Ensured::Created
        );
        Ok(())
    })
    .unwrap();
    assert!(!w.enabled(ACTIVE));
    assert_eq!(register::status(&w.reg()).standing, Standing::NotEnabled);
    let installed = register::install(None, Intent::Ensure).unwrap();
    assert_eq!(installed.outcome, Ensured::Updated);
    assert_eq!(installed.enabled, vec![ACTIVE]);
    assert!(w.enabled(ACTIVE));
}

#[test]
fn uninstall_keeps_it_out_of_import_and_ensure() {
    let w = World::new("uninstall");
    w.import();
    let (code, gone) = w.ctl(&["mcp", "uninstall"]);
    assert_eq!(code, 0, "{gone}");
    assert_eq!(gone["data"]["removed"], true);
    let before = w.registry_text();
    w.import();
    assert_eq!(self_entries(&w.reg()), 0);
    assert_eq!(w.registry_text(), before);
    let installed = register::install(None, Intent::Ensure).unwrap();
    assert_eq!(installed.outcome, Ensured::OptedOut);
    assert!(installed.id.is_empty());
    let handled = crate::plus::dispatch("plus.selfmcp.ensure", json!({})).unwrap();
    assert_eq!(handled["action"], "opted-out");
    assert_eq!(self_entries(&w.reg()), 0);
    assert_eq!(register::status(&w.reg()).standing, Standing::OptedOut);
    for profile in [ACTIVE, "claude-code", "cursor"] {
        assert!(!w.enabled(profile), "{profile}");
    }
}

#[test]
fn uninstall_before_any_install_still_records_the_opt_out() {
    let w = World::new("uninstall-first");
    let (code, none) = w.ctl(&["mcp", "uninstall"]);
    assert_eq!(code, 0, "{none}");
    assert_eq!(none["data"]["removed"], false);
    w.import();
    assert_eq!(self_entries(&w.reg()), 0);
}

#[test]
fn install_brings_an_uninstalled_server_back_everywhere() {
    let w = World::new("reinstall");
    w.import();
    w.ctl(&["mcp", "uninstall"]);
    let (code, back) = w.ctl(&["mcp", "install"]);
    assert_eq!(code, 0, "{back}");
    assert_eq!(back["data"]["action"], "created");
    let mut enabled: Vec<String> = serde_json::from_value(back["data"]["enabled"].clone()).unwrap();
    enabled.sort();
    assert_eq!(enabled, vec!["claude-code", "cursor", "default"]);
    assert_eq!(register::status(&w.reg()).standing, Standing::Enabled);
    assert_eq!(self_entries(&w.reg()), 1);
}

#[test]
fn install_leaves_a_profile_the_user_turned_off_unless_it_is_named() {
    let w = World::new("install");
    w.import();
    w.switch("cursor", false);
    let (code, plain) = w.ctl(&["mcp", "install"]);
    assert_eq!(code, 0, "{plain}");
    assert_eq!(plain["data"]["action"], "unchanged");
    assert_eq!(plain["data"]["enabled"], json!([]));
    assert!(!w.enabled("cursor"));
    let (code, named) = w.ctl(&["mcp", "install", "--profile", "cursor"]);
    assert_eq!(code, 0, "{named}");
    assert_eq!(named["data"]["enabled"], json!(["cursor"]));
    assert!(w.enabled("cursor"));
    let (_, again) = w.ctl(&["mcp", "install", "--profile", "cursor"]);
    assert_eq!(again["data"]["action"], "unchanged");
}

#[test]
fn install_with_a_profile_also_enables_the_active_profile_once() {
    let w = World::new("named");
    registry::update(|reg| {
        reg.add_profile("Review");
        Ok(())
    })
    .unwrap();
    let (code, done) = w.ctl(&["mcp", "install", "--profile", "review"]);
    assert_eq!(code, 0, "{done}");
    assert!(w.enabled(ACTIVE));
    assert!(w.enabled("review"));
}

#[test]
fn deleting_the_entry_by_hand_is_an_opt_out_too() {
    let w = World::new("delete");
    w.import();
    let id = w.self_id();
    registry::update(|reg| reg.remove_server(&id)).unwrap();
    w.import();
    assert_eq!(self_entries(&w.reg()), 0);
    assert_eq!(register::status(&w.reg()).standing, Standing::OptedOut);
}

#[test]
fn the_ipc_handler_enables_once_and_reports_an_opt_out() {
    let _w = World::new("ipc");
    let first = crate::plus::dispatch("plus.selfmcp.ensure", json!({})).unwrap();
    assert_eq!(first["action"], "created");
    assert_eq!(first["enabled"], json!([ACTIVE]));
    let second = crate::plus::dispatch("plus.selfmcp.ensure", json!({})).unwrap();
    assert_eq!(second["action"], "unchanged");
    assert_eq!(second["enabled"], json!([]));
    register::uninstall_self_server().unwrap();
    let third = crate::plus::dispatch("plus.selfmcp.ensure", json!({})).unwrap();
    assert_eq!(third["action"], "opted-out");
    assert_eq!(third["id"], "");
}

#[test]
fn the_record_lives_under_plus_and_keeps_other_personal_data() {
    let w = World::new("record");
    registry::update(|reg| {
        reg.unknown_fields
            .insert("plus".into(), json!({"other": {"kept": true}}));
        Ok(())
    })
    .unwrap();
    w.import();
    let raw: Value = serde_json::from_str(&w.registry_text()).unwrap();
    assert_eq!(raw["plus"]["other"]["kept"], true);
    assert_eq!(
        raw["plus"]["selfmcp"]["enabledIn"],
        json!([ACTIVE, "claude-code", "cursor"])
    );
    w.ctl(&["mcp", "uninstall"]);
    let raw: Value = serde_json::from_str(&w.registry_text()).unwrap();
    assert_eq!(raw["plus"]["other"]["kept"], true);
    assert_eq!(raw["plus"]["selfmcp"]["optOut"], true);
}

#[test]
fn doctor_reports_enabled_disabled_and_opted_out() {
    let w = World::new("doctor");
    w.give_binary();
    w.import();

    let (code, on) = w.ctl(&["mcp", "doctor"]);
    assert_eq!(code, 0, "{on}");
    assert_eq!(on["data"]["state"], "enabled");
    assert_eq!(check(&on, "enabled_in_active_profile")["detail"], ACTIVE);
    assert_eq!(
        check(&on, "enabled_in_client_profiles")["detail"],
        "claude-code, cursor"
    );

    w.switch("cursor", false);
    let (code, off) = w.ctl(&["mcp", "doctor"]);
    assert_eq!(code, 0, "{off}");
    assert_eq!(off["data"]["state"], "disabled");
    assert_eq!(
        check(&off, "enabled_in_client_profiles")["detail"],
        "disabled on purpose in cursor"
    );
    let cursor = off["data"]["clientProfiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "cursor")
        .unwrap()
        .clone();
    assert_eq!(cursor["enabled"], false);
    assert_eq!(cursor["optedOut"], true);

    w.ctl(&["mcp", "uninstall"]);
    let (code, gone) = w.ctl(&["mcp", "doctor"]);
    assert_eq!(code, 0, "{gone}");
    assert_eq!(gone["data"]["state"], "opted-out");
    assert_eq!(check(&gone, "registry_entry")["ok"], true);
}

#[test]
fn doctor_fails_while_the_server_is_missing_or_was_never_switched_on() {
    let w = World::new("doctor-fail");
    w.give_binary();
    let (code, missing) = w.ctl(&["mcp", "doctor"]);
    assert_eq!(code, 1, "{missing}");
    assert_eq!(missing["data"]["state"], "missing");
    assert_eq!(check(&missing, "registry_entry")["ok"], false);

    registry::update(|reg| {
        let command = register::binary_path();
        register::apply_ensure_self_server(reg, &command);
        Ok(())
    })
    .unwrap();
    let (code, bad) = w.ctl(&["mcp", "doctor"]);
    assert_eq!(code, 1, "{bad}");
    assert_eq!(bad["data"]["state"], "not-enabled");
    assert_eq!(check(&bad, "enabled_in_active_profile")["ok"], false);
}

#[test]
fn doctor_flags_a_client_profile_the_server_never_reached() {
    let w = World::new("doctor-client");
    w.give_binary();
    w.import();
    registry::update(|reg| {
        let id = register::find_self(reg).unwrap().id.clone();
        reg.set_server_enabled("cursor", &id, false)?;
        let recorded = reg
            .unknown_fields
            .get_mut("plus")
            .and_then(|plus| plus.pointer_mut("/selfmcp/enabledIn"))
            .and_then(Value::as_array_mut)
            .unwrap();
        recorded.retain(|p| p != "cursor");
        Ok(())
    })
    .unwrap();
    let (code, bad) = w.ctl(&["mcp", "doctor"]);
    assert_eq!(code, 1, "{bad}");
    assert_eq!(bad["data"]["state"], "not-enabled");
    assert_eq!(check(&bad, "enabled_in_client_profiles")["ok"], false);
}
