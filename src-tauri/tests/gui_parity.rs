//! GUI parity gates (MIG-GUI-0, D-058/D-059): the registry that `toolportctl commands --json`
//! prints must describe exactly `COMMANDS` and the self-MCP catalog, with a policy for each row.

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use conduit_lib::plus::ctl::{registry, terminal_only, Tier, COMMANDS, SCHEMA_VERSION};
use conduit_lib::plus::selfmcp::TOOLS;
use serde_json::Value;

#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;

use ctl_world::CtlWorld;

fn registry_from_binary() -> Value {
    let world = CtlWorld::new("gui-parity", env!("CARGO_BIN_EXE_mock-mcp-server"));
    let output = Command::new(env!("CARGO_BIN_EXE_toolportctl"))
        .args(["--json", "commands"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &world.home)
        .env("TOOLPORT_DATA_DIR", &world.data)
        .env("TOOLPORT_SECRET_KEY", ctl_world::SECRET_KEY)
        .stdin(Stdio::null())
        .output()
        .expect("run toolportctl commands");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("one JSON envelope");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["command"], "commands");
    assert_eq!(envelope["schemaVersion"], SCHEMA_VERSION);
    envelope["data"].clone()
}

fn ids(data: &Value) -> Vec<String> {
    data["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_binary_prints_exactly_the_library_registry() {
    assert_eq!(registry_from_binary(), registry());
}

#[test]
fn the_registry_lists_every_command_row_and_its_sub_rows() {
    let data = registry();
    let listed: BTreeSet<String> = ids(&data).into_iter().collect();
    assert_eq!(listed.len(), ids(&data).len(), "duplicate ids");

    let top: BTreeSet<String> = COMMANDS.iter().map(|c| c.path.join(" ")).collect();
    for id in &top {
        assert!(
            listed.contains(id),
            "COMMANDS row `{id}` is missing from the registry"
        );
    }

    let rows = data["commands"].as_array().unwrap();
    for row in rows {
        let id = row["id"].as_str().unwrap();
        if top.contains(id) {
            continue;
        }
        let parent = row["parent"].as_str().unwrap_or_else(|| {
            panic!("`{id}` is neither a COMMANDS row nor a sub row of one");
        });
        assert!(
            top.contains(parent),
            "sub row `{id}` has an unknown parent `{parent}`"
        );
        assert_eq!(row["kind"], "command", "{id}");
    }

    let commands = rows.iter().filter(|r| r["kind"] == "command").count();
    let groups = rows.iter().filter(|r| r["kind"] == "group").count();
    assert_eq!(data["counts"]["commands"], commands);
    assert_eq!(data["counts"]["groups"], groups);
    assert_eq!(data["counts"]["rows"], rows.len());
}

#[test]
fn every_command_carries_a_complete_policy_and_groups_carry_none() {
    let data = registry();
    let tiers: BTreeSet<&str> = ["read", "write", "destructive"].into();
    for row in data["commands"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        if row["kind"] == "group" {
            assert!(row["tier"].is_null(), "group `{id}` must not carry a tier");
            continue;
        }
        let tier = row["tier"]
            .as_str()
            .unwrap_or_else(|| panic!("`{id}` has no tier"));
        assert!(tiers.contains(tier), "`{id}` tier {tier}");
        assert!(tiers.contains(row["baseTier"].as_str().unwrap()), "{id}");
        assert!(row["dryRun"].is_boolean(), "{id}");
        assert!(row["needs"].is_array(), "{id}");
        let surface = row["surface"].as_str().unwrap();
        assert!(["screen", "terminal"].contains(&surface), "{id}");
        for flag in row["flags"].as_array().unwrap() {
            let name = flag["name"].as_str().unwrap();
            assert!(
                !flag["valueType"].as_str().unwrap().is_empty(),
                "{id} {name}"
            );
            assert!(
                flag["hidden"] == true || !flag["effect"].as_str().unwrap().is_empty(),
                "`{id}` flag {name} has no effect text"
            );
        }
        if row["dryRun"] == true {
            let flags: Vec<&str> = row["flags"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["name"].as_str().unwrap())
                .collect();
            let preview = row["preview"]["flag"].as_str().unwrap_or("--dry-run");
            assert!(
                flags.contains(&preview),
                "`{id}` previews with {preview}, not in its flags"
            );
        }
    }
}

#[test]
fn the_registry_lists_every_self_mcp_tool_with_the_tier_and_default_of_the_catalog() {
    let data = registry();
    let listed = data["tools"].as_array().unwrap();
    assert_eq!(listed.len(), TOOLS.len());
    for def in TOOLS.iter() {
        let tool = listed
            .iter()
            .find(|t| t["name"] == def.name)
            .unwrap_or_else(|| panic!("tool `{}` missing from the registry", def.name));
        assert_eq!(
            tool["tier"],
            Tier::of_tool(def.tier).as_str(),
            "{}",
            def.name
        );
        assert_eq!(tool["toolTier"], def.tier, "{}", def.name);
        let previews = tool["dryRun"].as_str().unwrap();
        let expected = match def.params.iter().find(|p| p.name == "dry_run") {
            None => "none",
            Some(param) if param.default == Some(true) => "default-on",
            Some(_) => "param",
        };
        assert_eq!(previews, expected, "{}", def.name);
        if let Some(command) = tool["command"].as_str() {
            assert!(
                ids(&data).iter().any(|id| id == command),
                "tool `{}` maps to unknown command `{command}`",
                def.name
            );
        }
    }
}

#[test]
fn terminal_only_rows_agree_with_the_bridge_guard() {
    let argv = |parts: &[&str]| -> Vec<String> { parts.iter().map(|s| s.to_string()).collect() };
    let data = registry();
    for row in data["commands"].as_array().unwrap() {
        if row["surface"] != "terminal" {
            continue;
        }
        let id = row["id"].as_str().unwrap();
        assert!(
            row["needs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == "terminal-only"),
            "{id}"
        );
        let mut words = argv(&id.split(' ').collect::<Vec<_>>());
        words.push("claude".into());
        assert!(
            terminal_only(&words).is_some(),
            "`{id}` is not refused by the bridge guard"
        );
    }
    assert!(terminal_only(&argv(&["compression", "run", "--plan", "claude"])).is_none());
}
