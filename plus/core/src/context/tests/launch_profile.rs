use crate::context::fsx;
use crate::context::launch_profile::{self as lp, AddArgs};
use crate::context::testkit::ScratchDir;
use crate::reach::FOLDER_RULE;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use toolport_plus_api::{
    convert, AccessSetInfo, MemoryClients, MemoryHost, MemoryRegistry, MemorySecrets, ServerRecord,
};

fn server(id: &str) -> ServerRecord {
    convert(
        &json!({"id": id, "name": id.to_uppercase(), "enabled": true,
        "transport": "stdio", "command": "run-it"}),
    )
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
    data: PathBuf,
    cwd: PathBuf,
}

fn fx(tag: &str) -> Fx {
    let scratch = ScratchDir::new(tag);
    let dir = fsx::canonical(scratch.path());
    let (home, data, cwd) = (dir.join("home"), dir.join("data"), dir.join("proj"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(cwd.join(".git/info")).unwrap();
    let registry = MemoryRegistry::with_servers(vec![server("srv-a"), server("srv-b")]);
    *registry.access_sets.borrow_mut() = vec![
        set("default", &["srv-a", "srv-b"]),
        set("bare", &["srv-a"]),
        set("wide", &["srv-a", "srv-b"]),
    ];
    *registry.default_access.borrow_mut() = Some("default".into());
    let host = MemoryHost {
        registry,
        clients: MemoryClients {
            gateway: Some("/opt/toolport/toolport-gateway".into()),
            ..MemoryClients::with_client("claude-code", "Claude Code")
        },
        secrets: MemorySecrets {
            data_dir: Some(data.clone()),
            ..Default::default()
        },
        home: Some(home.clone()),
        ..Default::default()
    };
    Fx {
        _scratch: scratch,
        host,
        home,
        data,
        cwd,
    }
}

impl Fx {
    fn add(&self, name: &str, set: Option<&str>, deny: Option<bool>, dry: bool) -> Value {
        lp::add(
            &self.host,
            &AddArgs {
                name,
                set,
                deny_direct: deny,
                dry_run: dry,
            },
        )
        .unwrap()
    }

    fn context_json(&self) -> Value {
        let text = fs::read_to_string(self.home.join(".config/mcpm/context.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn mcp_file(&self, name: &str) -> PathBuf {
        self.home
            .join(".config/toolport/profiles")
            .join(name)
            .join("mcp.json")
    }

    fn scope(&self, id: &str) -> Option<String> {
        self.host.registry.client_scopes.borrow().get(id).cloned()
    }

    fn record_direct(&self, entry: &str, server: &str) {
        let mut fields = self.host.registry.unknown_fields.borrow_mut();
        let plus = fields.entry("plus").or_insert(json!({}));
        plus["directEntries"]["claude-code"][entry] = json!({"server": server});
    }

    fn settings(&self) -> Value {
        let text = fs::read_to_string(self.cwd.join(".claude/settings.local.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

fn steps(out: &Value) -> Vec<String> {
    out["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["op"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn test01_dry_run_shows_the_plan_and_writes_nothing() {
    let fx = fx("lp01");
    let out = fx.add("bare", Some("bare"), None, true);
    assert_eq!(steps(&out), ["update", "create", "create"]);
    assert_eq!(out["result"], Value::Null);
    assert!(out["plan"]["undo"]
        .as_str()
        .unwrap()
        .ends_with("launch-profile rm bare"));
    assert!(fx.scope("claude-code-bare").is_none());
    assert!(!fx.mcp_file("bare").exists());
    assert!(!fx.home.join(".config/mcpm/context.json").exists());
}

#[test]
fn test02_add_binds_the_set_copies_discovery_and_writes_one_gateway_entry() {
    let fx = fx("lp02");
    fx.host
        .registry
        .client_discovery
        .borrow_mut()
        .insert("claude-code".into(), "full".into());
    fs::create_dir_all(fx.home.join(".config/mcpm")).unwrap();
    fs::write(
        fx.home.join(".config/mcpm/context.json"),
        r#"{"wrapDefaultClaude": true}"#,
    )
    .unwrap();
    let out = fx.add("bare", Some("bare"), None, false);
    assert_eq!(fx.scope("claude-code-bare").as_deref(), Some("bare"));
    assert_eq!(
        fx.host
            .registry
            .client_discovery
            .borrow()
            .get("claude-code-bare"),
        Some(&"full".to_string())
    );
    let mcp: Value =
        serde_json::from_str(&fs::read_to_string(fx.mcp_file("bare")).unwrap()).unwrap();
    let entry = &mcp["mcpServers"]["toolport"];
    assert_eq!(entry["command"], "/opt/toolport/toolport-gateway");
    assert_eq!(entry["env"]["TOOLPORT_CLIENT_ID"], "claude-code-bare");
    assert_eq!(entry["env"]["TOOLPORT_PROFILE"], "bare");
    let config = fx.context_json();
    assert_eq!(config["wrapDefaultClaude"], true);
    assert_eq!(config["launchProfiles"]["bare"]["accessSet"], "bare");
    assert!(out["profile"]["command"]
        .as_str()
        .unwrap()
        .starts_with("claude --strict-mcp-config --mcp-config "));
    assert_eq!(out["profile"]["state"], "ok");
}

#[test]
fn test03_adding_the_same_profile_again_has_nothing_to_do() {
    let fx = fx("lp03");
    fx.add("bare", Some("bare"), None, false);
    let again = fx.add("bare", None, None, true);
    assert_eq!(
        again["plan"]["summary"],
        "Launch profile bare is up to date"
    );
    assert!(steps(&again).iter().all(|op| op == "note"));
}

#[test]
fn test04_an_unknown_access_set_is_refused_with_the_known_names() {
    let fx = fx("lp04");
    let err = lp::add(
        &fx.host,
        &AddArgs {
            name: "bare",
            set: Some("nope"),
            deny_direct: None,
            dry_run: true,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("no access set called nope"));
    assert!(err.message.contains("default, bare, wide"));
}

#[test]
fn test05_a_name_outside_the_shell_safe_set_is_refused() {
    let fx = fx("lp05");
    let err = lp::add(
        &fx.host,
        &AddArgs {
            name: "Bad Name",
            set: Some("bare"),
            deny_direct: None,
            dry_run: true,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("must match"));
}

#[test]
fn test06_a_new_profile_needs_a_set() {
    let fx = fx("lp06");
    let err = lp::add(
        &fx.host,
        &AddArgs {
            name: "bare",
            set: None,
            deny_direct: None,
            dry_run: true,
        },
    )
    .unwrap_err();
    assert!(err.message.contains("--set"));
}

#[test]
fn test07_remove_takes_the_binding_the_file_and_the_record_away() {
    let fx = fx("lp07");
    fx.add("bare", Some("bare"), None, false);
    let plan = lp::rm(&fx.host, "bare", true).unwrap();
    assert_eq!(steps(&plan), ["update", "delete", "update"]);
    assert!(fx.mcp_file("bare").exists());
    let done = lp::rm(&fx.host, "bare", false).unwrap();
    assert_eq!(
        done["result"]["undo"],
        "toolportctl context launch-profile add bare --set bare"
    );
    assert!(fx.scope("claude-code-bare").is_none());
    assert!(!fx.mcp_file("bare").exists());
    assert!(fx.context_json().get("launchProfiles").is_none());
    assert_eq!(
        lp::rm(&fx.host, "bare", false).unwrap_err().message,
        "no launch profile called bare"
    );
}

#[test]
fn test08_a_folder_mapping_that_decides_instead_is_named_with_the_rule() {
    let fx = fx("lp08");
    let folder = fx.cwd.to_string_lossy().to_string();
    fx.host
        .registry
        .folder_routes
        .borrow_mut()
        .push((folder.clone(), "wide".into()));
    let out = fx.add("bare", Some("bare"), None, true);
    let warnings = out["plan"]["warnings"].as_array().unwrap();
    let text = warnings
        .iter()
        .filter_map(Value::as_str)
        .find(|w| w.contains(&folder))
        .expect("a warning names the folder");
    assert!(text.contains(FOLDER_RULE));
    assert!(text.contains("launch profile bare's own set"));
    assert_eq!(out["folderWarnings"][0]["rule"], FOLDER_RULE);
}

#[test]
fn test09_ls_narrows_the_folder_warning_to_the_folder_the_launch_starts_in() {
    let fx = fx("lp09");
    fx.add("bare", Some("bare"), None, false);
    fx.host
        .registry
        .folder_routes
        .borrow_mut()
        .push((fx.cwd.to_string_lossy().to_string(), "wide".into()));
    let inside = lp::ls(&fx.host, None, Some(fx.cwd.to_str().unwrap())).unwrap();
    assert_eq!(
        inside["profiles"][0]["folderWarnings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let elsewhere = fx.home.to_string_lossy().to_string();
    let outside = lp::ls(&fx.host, Some("bare"), Some(&elsewhere)).unwrap();
    assert!(outside["profiles"][0]["folderWarnings"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn test10_ls_reports_a_lost_binding_a_missing_file_and_an_edited_one() {
    let fx = fx("lp10");
    fx.add("bare", Some("bare"), None, false);
    fx.host.registry.client_scopes.borrow_mut().clear();
    let unbound = lp::ls(&fx.host, Some("bare"), None).unwrap();
    assert_eq!(unbound["profiles"][0]["state"], "unbound");
    fx.add("bare", None, None, false);
    fs::remove_file(fx.mcp_file("bare")).unwrap();
    let missing = lp::ls(&fx.host, Some("bare"), None).unwrap();
    assert_eq!(missing["profiles"][0]["state"], "file-missing");
    fx.add("bare", None, None, false);
    fs::write(fx.mcp_file("bare"), "{}").unwrap();
    let edited = lp::ls(&fx.host, Some("bare"), None).unwrap();
    assert_eq!(edited["profiles"][0]["state"], "stale-file");
}

#[test]
fn test11_policy_rendering_is_off_by_default_and_writes_nothing() {
    let fx = fx("lp11");
    fx.record_direct("beta", "srv-b");
    fx.add("bare", Some("bare"), None, false);
    let out = lp::policy(&fx.host, "bare", fx.cwd.to_str().unwrap(), false).unwrap();
    assert_eq!(out["denyDirect"], false);
    assert!(out["plan"]["steps"].as_array().unwrap().is_empty());
    assert!(!fx.cwd.join(".claude/settings.local.json").exists());
}

#[test]
fn test12_policy_denies_the_direct_entries_the_set_leaves_out() {
    let fx = fx("lp12");
    fx.record_direct("alpha", "srv-a");
    fx.record_direct("beta", "srv-b");
    fx.add("bare", Some("bare"), Some(true), false);
    let cwd = fx.cwd.to_str().unwrap();
    let plan = lp::policy(&fx.host, "bare", cwd, true).unwrap();
    assert_eq!(plan["keys"], json!([{"serverName": "beta"}]));
    assert!(!fx.cwd.join(".claude/settings.local.json").exists());
    let warnings = plan["plan"]["warnings"].to_string();
    assert!(warnings.contains("unverified"));
    lp::policy(&fx.host, "bare", cwd, false).unwrap();
    assert_eq!(
        fx.settings()["deniedMcpServers"],
        json!([{"serverName": "beta"}])
    );
    let ledger = crate::context::bundle_ledger::load(&fx.data);
    assert!(ledger
        .controls
        .values()
        .any(|controls| controls.contains_key("launch-policy:bare")));
}

#[test]
fn test13_switching_the_policy_off_puts_the_folder_file_back() {
    let fx = fx("lp13");
    fx.record_direct("beta", "srv-b");
    fx.add("bare", Some("bare"), Some(true), false);
    let cwd = fx.cwd.to_str().unwrap();
    lp::policy(&fx.host, "bare", cwd, false).unwrap();
    assert!(fx.cwd.join(".claude/settings.local.json").exists());
    let off = fx.add("bare", None, Some(false), false);
    assert!(off["result"]["changed"]
        .to_string()
        .contains("settings.local.json"));
    assert!(!fx.cwd.join(".claude/settings.local.json").exists());
}

#[test]
fn test14_remove_also_takes_the_rendered_policy_away() {
    let fx = fx("lp14");
    fx.record_direct("beta", "srv-b");
    fx.add("bare", Some("bare"), Some(true), false);
    lp::policy(&fx.host, "bare", fx.cwd.to_str().unwrap(), false).unwrap();
    lp::rm(&fx.host, "bare", false).unwrap();
    assert!(!fx.cwd.join(".claude/settings.local.json").exists());
}

#[test]
fn test15_policy_leaves_the_users_own_denied_servers_alone() {
    let fx = fx("lp15");
    fx.record_direct("beta", "srv-b");
    fs::create_dir_all(fx.cwd.join(".claude")).unwrap();
    fs::write(
        fx.cwd.join(".claude/settings.local.json"),
        r#"{"deniedMcpServers": [{"serverName": "mine"}]}"#,
    )
    .unwrap();
    fx.add("bare", Some("bare"), Some(true), false);
    let cwd = fx.cwd.to_str().unwrap();
    lp::policy(&fx.host, "bare", cwd, false).unwrap();
    assert_eq!(
        fx.settings()["deniedMcpServers"],
        json!([{"serverName": "mine"}, {"serverName": "beta"}])
    );
    fx.add("bare", None, Some(false), false);
    assert_eq!(
        fx.settings()["deniedMcpServers"],
        json!([{"serverName": "mine"}])
    );
}
