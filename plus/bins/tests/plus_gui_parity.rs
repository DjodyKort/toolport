//! GUI parity gates (D-058, D-059): the registry that `toolportctl commands --json` prints must
//! describe exactly `COMMANDS` and the self-MCP catalog, with a policy for each row, and
//! `src/plus/gui-parity/` must give every command and tool a screen or terminal action.
//! This is the Rust side; the TypeScript side joins when the screen lands.
//!
//! While the screen is not built, a row's action is `planned` and counts as pending. Set
//! `GUI_PARITY_REQUIRE_BUILT=1` to turn pending rows into failures (the gate for the cutover).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use toolport_plus::ctl::{registry, terminal_only, Tier, COMMANDS, SCHEMA_VERSION};
use toolport_plus::selfmcp::TOOLS;

mod common;
use common::world::{World, SECRET_KEY};

fn registry_from_binary() -> Value {
    let world = World::new("gui-parity");
    let output = Command::new(env!("CARGO_BIN_EXE_toolportctl"))
        .args(["--json", "commands"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &world.home)
        .env("TOOLPORT_DATA_DIR", &world.data)
        .env("TOOLPORT_SECRET_KEY", SECRET_KEY)
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
        let mut words: Vec<String> = id.split(' ').map(String::from).collect();
        words.push("claude".into());
        assert!(
            terminal_only(&words).is_some(),
            "`{id}` is not refused by the bridge guard"
        );
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

/// The manifest rebuilt from `src/plus/gui-parity/`: `_meta.json` plus one file per entry in
/// `routes/`, `actions/`, `owners/`, `commands/` and `tools/`. A command id is stored with its
/// spaces as underscores.
fn manifest() -> Value {
    let dir = repo_root().join("src/plus/gui-parity");
    let mut manifest = read_json(&dir.join("_meta.json"));
    for section in ["routes", "actions", "owners", "commands", "tools"] {
        let mut rows = serde_json::Map::new();
        for entry in std::fs::read_dir(dir.join(section)).unwrap().flatten() {
            let path = entry.path();
            let Some(stem) = path
                .file_name()
                .and_then(|n| n.to_str()?.strip_suffix(".json"))
            else {
                continue;
            };
            let key = if section == "commands" {
                stem.replace('_', " ")
            } else {
                stem.to_string()
            };
            rows.insert(key, read_json(&path));
        }
        manifest[section] = Value::Object(rows);
    }
    manifest
}

#[derive(Default, Debug)]
struct Report {
    errors: Vec<String>,
    pending_commands: Vec<String>,
    pending_tools: Vec<String>,
}

const HINT: &str = "map it to a built route and action";

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

fn entries<'a>(manifest: &'a Value, key: &str) -> BTreeMap<&'a str, &'a Value> {
    manifest[key]
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.as_str(), v)).collect())
        .unwrap_or_default()
}

/// The rules of R3 that a test can prove; the same rules run in `src/plus/guiParityCheck.ts`.
fn check_parity(manifest: &Value, registry: &Value, repo: &Path) -> Report {
    let mut report = Report::default();
    let mut fail = |message: String| report.errors.push(message);

    if manifest["schemaVersion"] != 1 {
        fail(format!(
            "schemaVersion is {}, not 1",
            manifest["schemaVersion"]
        ));
    }
    let routes = entries(manifest, "routes");
    let actions = entries(manifest, "actions");
    let owners = entries(manifest, "owners");
    let commands = entries(manifest, "commands");
    let tools = entries(manifest, "tools");

    let mut surface_of: BTreeMap<String, String> = BTreeMap::new();
    let mut groups = BTreeSet::new();
    for row in registry["commands"].as_array().unwrap() {
        if row["kind"] != "command" {
            continue;
        }
        surface_of.insert(
            field(row, "id").to_string(),
            row["surface"].as_str().unwrap_or("screen").to_string(),
        );
        groups.insert(row["path"][0].as_str().unwrap().to_string());
    }
    let tool_command: BTreeMap<String, Option<String>> = registry["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                field(t, "name").to_string(),
                t["command"].as_str().map(String::from),
            )
        })
        .collect();

    for (id, surface) in &surface_of {
        if !commands.contains_key(id.as_str()) {
            fail(format!(
                "command `{id}` has no entry in commands; {HINT}, e.g. \"{id}\": {{\"route\":\"<route>\",\"action\":\"<route>.<action>\",\"surface\":\"{surface}\"}}"
            ));
        }
    }
    for id in commands.keys() {
        if !surface_of.contains_key(*id) {
            fail(format!(
                "commands entry `{id}` is not a command of the registry"
            ));
        }
    }
    for name in tool_command.keys() {
        if !tools.contains_key(name.as_str()) {
            fail(format!(
                "tool `{name}` has no entry in tools; {HINT}, e.g. \"{name}\": {{\"route\":\"<route>\",\"action\":\"<route>.<action>\",\"surface\":\"screen\"}}"
            ));
        }
    }
    for name in tools.keys() {
        if !tool_command.contains_key(*name) {
            fail(format!("tools entry `{name}` is not a tool of the catalog"));
        }
    }
    for group in &groups {
        if !owners.contains_key(group.as_str()) {
            fail(format!(
                "command group `{group}` has no owner item in owners"
            ));
        }
    }

    let mut referenced = BTreeSet::new();
    let mut check_entry = |kind: &str, id: &str, entry: &Value, fail: &mut dyn FnMut(String)| {
        let where_ = format!("{kind} `{id}`");
        let surface = field(entry, "surface");
        if !["screen", "terminal"].contains(&surface) {
            fail(format!(
                "{where_}: surface `{surface}` is neither screen nor terminal"
            ));
        }
        let route = field(entry, "route");
        if !routes.contains_key(route) {
            fail(format!("{where_}: route `{route}` is not in routes"));
        }
        let action = field(entry, "action");
        match actions.get(action) {
            None => fail(format!("{where_}: action `{action}` is not in actions")),
            Some(found) => {
                referenced.insert(action.to_string());
                if field(found, "route") != route {
                    fail(format!(
                        "{where_}: action `{action}` belongs to route `{}`, not `{route}`",
                        field(found, "route")
                    ));
                }
            }
        }
    };
    for (id, entry) in &commands {
        check_entry("command", id, entry, &mut fail);
        if let Some(registered) = surface_of.get(*id) {
            if registered != field(entry, "surface") {
                fail(format!(
                    "command `{id}`: surface is {} but the registry says {registered}",
                    field(entry, "surface")
                ));
            }
        }
    }
    for (name, entry) in &tools {
        check_entry("tool", name, entry, &mut fail);
        if let Some(Some(mapped)) = tool_command.get(*name) {
            let expected = surface_of.get(mapped).map_or("screen", String::as_str);
            if field(entry, "surface") != expected {
                fail(format!(
                    "tool `{name}`: surface is {} but {mapped} is {expected}",
                    field(entry, "surface")
                ));
            }
        } else if field(entry, "surface") != "screen" {
            fail(format!(
                "tool `{name}`: surface is {} but a tool is screen",
                field(entry, "surface")
            ));
        }
    }

    let valid = |status: &str| ["planned", "built"].contains(&status);
    for (id, route) in &routes {
        if !valid(field(route, "status")) {
            fail(format!(
                "route `{id}`: unknown status `{}`",
                field(route, "status")
            ));
        }
        if field(route, "status") == "built" {
            let component = field(route, "component");
            if component.is_empty() {
                fail(format!("route `{id}` is built but names no component"));
            } else if !repo.join(component).is_file() {
                fail(format!(
                    "route `{id}`: component {component} does not exist"
                ));
            }
        }
        if !actions.values().any(|a| field(a, "route") == *id) {
            fail(format!("route `{id}` has no action"));
        }
    }
    for (id, action) in &actions {
        if !valid(field(action, "status")) {
            fail(format!(
                "action `{id}`: unknown status `{}`",
                field(action, "status")
            ));
        }
        if !routes.contains_key(field(action, "route")) {
            fail(format!(
                "action `{id}`: route `{}` is not in routes",
                field(action, "route")
            ));
        }
        if !referenced.contains(*id) {
            fail(format!("action `{id}` is not used by any command or tool"));
        }
        if field(action, "status") != "built" {
            continue;
        }
        if routes
            .get(field(action, "route"))
            .map(|r| field(r, "status"))
            != Some("built")
        {
            fail(format!(
                "action `{id}` is built but its route `{}` is not",
                field(action, "route")
            ));
        }
        let test = field(action, "test");
        if test.is_empty() {
            fail(format!("action `{id}` is built but has no component test"));
        } else if !repo.join(test).is_file() {
            fail(format!("action `{id}`: test {test} does not exist"));
        } else if !std::fs::read_to_string(repo.join(test))
            .unwrap_or_default()
            .contains(*id)
        {
            fail(format!("action `{id}`: test {test} never names the action"));
        }
    }

    let pending = |entry: Option<&&Value>| match entry {
        None => true,
        Some(entry) => {
            actions
                .get(field(entry, "action"))
                .map(|a| field(a, "status"))
                != Some("built")
        }
    };
    report.pending_commands = surface_of
        .keys()
        .filter(|id| pending(commands.get(id.as_str())))
        .cloned()
        .collect();
    report.pending_tools = tool_command
        .keys()
        .filter(|name| pending(tools.get(name.as_str())))
        .cloned()
        .collect();
    report
}

/// The counts of the parity gate, the same numbers as `paritySummary` in `guiParityCheck.ts`.
#[derive(Debug, PartialEq)]
struct Summary {
    commands: usize,
    tools: usize,
    command_rows: usize,
    tool_rows: usize,
    rows_on_built: usize,
    actions_built: usize,
    pending: usize,
    waivers: usize,
}

impl Summary {
    fn of(manifest: &Value, registry: &Value, report: &Report) -> Summary {
        let commands = entries(manifest, "commands");
        let tools = entries(manifest, "tools");
        let actions = entries(manifest, "actions");
        let waivers = commands
            .values()
            .chain(tools.values())
            .filter(|entry| !["screen", "terminal"].contains(&field(entry, "surface")))
            .count();
        Summary {
            commands: registry["commands"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["kind"] == "command")
                .count(),
            tools: registry["tools"].as_array().unwrap().len(),
            command_rows: commands.len(),
            tool_rows: tools.len(),
            rows_on_built: commands
                .values()
                .chain(tools.values())
                .filter(|entry| {
                    actions
                        .get(field(entry, "action"))
                        .map(|a| field(a, "status"))
                        == Some("built")
                })
                .count(),
            actions_built: actions
                .values()
                .filter(|action| field(action, "status") == "built")
                .count(),
            pending: report.pending_commands.len() + report.pending_tools.len(),
            waivers,
        }
    }

    fn line(&self) -> String {
        format!(
            "gui parity: {} commands + {} tools = {} manifest rows, {} rows on built screen actions ({} distinct actions), {} pending, {} waivers",
            self.commands,
            self.tools,
            self.command_rows + self.tool_rows,
            self.rows_on_built,
            self.actions_built,
            self.pending,
            self.waivers
        )
    }
}

#[test]
fn the_manifest_gives_every_command_and_tool_an_action() {
    let manifest = manifest();
    let registry = registry();
    let report = check_parity(&manifest, &registry, &repo_root());
    let summary = Summary::of(&manifest, &registry, &report);
    println!("{}", summary.line());

    assert!(report.errors.is_empty(), "{}", report.errors.join("\n"));
    assert_eq!(summary.command_rows, summary.commands);
    assert_eq!(summary.tool_rows, summary.tools);
    assert_eq!(summary.waivers, 0);
    assert_eq!(
        summary.rows_on_built + summary.pending,
        summary.commands + summary.tools
    );
    if std::env::var("GUI_PARITY_REQUIRE_BUILT").is_ok_and(|v| v == "1") {
        assert!(
            report.pending_commands.is_empty() && report.pending_tools.is_empty(),
            "rows without a built screen action: {report:?}"
        );
    }
}

#[test]
fn the_manifest_lists_every_row_of_commands_and_every_tool_by_name() {
    let manifest = manifest();
    let registry = registry();
    let rows = registry["commands"].as_array().unwrap();
    for command in COMMANDS {
        let id = command.path.join(" ");
        let row = rows
            .iter()
            .find(|row| row["id"] == id.as_str())
            .unwrap_or_else(|| panic!("COMMANDS row `{id}` is missing from the registry"));
        assert_eq!(
            manifest["commands"].get(&id).is_some(),
            row["kind"] == "command",
            "`{id}` is a {}: a command has a manifest entry, a group has none",
            row["kind"]
        );
    }
    for tool in TOOLS {
        assert!(
            manifest["tools"].get(tool.name).is_some(),
            "tool `{}` has no manifest entry",
            tool.name
        );
    }
    for (id, entry) in manifest["commands"].as_object().unwrap() {
        let words: Vec<String> = id.split(' ').map(String::from).collect();
        let terminal =
            terminal_only(&[words.clone(), vec!["claude".to_string()]].concat()).is_some();
        assert_eq!(
            entry["surface"] == "terminal",
            terminal,
            "`{id}`: surface and the bridge guard disagree"
        );
    }
}

mod checker {
    use super::*;
    use serde_json::json;

    /// A scratch repo with one component and one test that names `servers.list`, made once.
    fn scratch_repo() -> PathBuf {
        static REPO: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        REPO.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("plus-parity-repo-{}", std::process::id()));
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::write(dir.join("src/Servers.tsx"), "export {}\n").unwrap();
            std::fs::write(dir.join("src/Servers.test.tsx"), "// servers.list\n").unwrap();
            dir
        })
        .clone()
    }

    fn registry() -> Value {
        json!({
            "commands": [
                {"id": "status", "kind": "command", "path": ["status"], "surface": "screen"},
                {"id": "server", "kind": "group", "path": ["server"], "surface": null},
                {"id": "server ls", "kind": "command", "path": ["server", "ls"], "surface": "screen"},
                {"id": "direct run", "kind": "command", "path": ["direct", "run"], "surface": "terminal"},
            ],
            "tools": [
                {"name": "where_am_i", "command": null},
                {"name": "servers_list", "command": "server ls"},
            ],
        })
    }

    fn good() -> Value {
        json!({
            "schemaVersion": 1,
            "routes": {
                "pending": {"title": "Pending", "status": "planned"},
                "servers": {"title": "Servers", "status": "built", "component": "src/Servers.tsx"},
            },
            "actions": {
                "pending.run": {"route": "pending", "status": "planned", "summary": "run"},
                "pending.terminal": {"route": "pending", "status": "planned", "summary": "t"},
                "pending.tool": {"route": "pending", "status": "planned", "summary": "tool"},
                "servers.list": {"route": "servers", "status": "built", "summary": "list",
                                 "test": "src/Servers.test.tsx"},
            },
            "owners": {"status": "MIG-2X-S1", "server": "MIG-2X-S2", "direct": "MIG-2X-S11"},
            "commands": {
                "status": {"route": "pending", "action": "pending.run", "surface": "screen"},
                "server ls": {"route": "servers", "action": "servers.list", "surface": "screen"},
                "direct run": {"route": "pending", "action": "pending.terminal", "surface": "terminal"},
            },
            "tools": {
                "where_am_i": {"route": "pending", "action": "pending.tool", "surface": "screen"},
                "servers_list": {"route": "servers", "action": "servers.list", "surface": "screen"},
            },
        })
    }

    fn errors(change: impl FnOnce(&mut Value)) -> String {
        let mut manifest = good();
        change(&mut manifest);
        check_parity(&manifest, &registry(), &scratch_repo())
            .errors
            .join("\n")
    }

    #[test]
    fn a_consistent_manifest_passes_and_lists_its_pending_rows() {
        let report = check_parity(&good(), &registry(), &scratch_repo());
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.pending_commands, ["direct run", "status"]);
        assert_eq!(report.pending_tools, ["where_am_i"]);
    }

    #[test]
    fn the_summary_counts_pending_rows_and_waivers() {
        let mut manifest = good();
        manifest["commands"]["status"]["surface"] = "waived".into();
        let report = check_parity(&manifest, &registry(), &scratch_repo());
        let summary = Summary::of(&manifest, &registry(), &report);
        assert_eq!(
            summary,
            Summary {
                commands: 3,
                tools: 2,
                command_rows: 3,
                tool_rows: 2,
                rows_on_built: 2,
                actions_built: 1,
                pending: 3,
                waivers: 1,
            }
        );
        assert_eq!(
            summary.line(),
            "gui parity: 3 commands + 2 tools = 5 manifest rows, 2 rows on built screen actions (1 distinct actions), 3 pending, 1 waivers"
        );
        assert!(report
            .errors
            .join("\n")
            .contains("neither screen nor terminal"));
    }

    #[test]
    fn a_command_or_tool_without_an_entry_fails_with_the_line_to_add() {
        let message = errors(|m| {
            m["commands"].as_object_mut().unwrap().remove("server ls");
            m["tools"].as_object_mut().unwrap().remove("where_am_i");
        });
        assert!(
            message.contains("map it to a built route and action"),
            "{message}"
        );
        assert!(
            message.contains(
                "\"server ls\": {\"route\":\"<route>\",\"action\":\"<route>.<action>\",\"surface\":\"screen\"}"
            ),
            "{message}"
        );
        assert!(
            message.contains("tool `where_am_i` has no entry"),
            "{message}"
        );
    }

    #[test]
    fn a_stale_entry_fails() {
        let message = errors(|m| {
            m["commands"]["gone"] = m["commands"]["status"].clone();
            m["tools"]["gone"] = m["tools"]["where_am_i"].clone();
        });
        assert!(
            message.contains("commands entry `gone` is not a command"),
            "{message}"
        );
        assert!(
            message.contains("tools entry `gone` is not a tool"),
            "{message}"
        );
    }

    #[test]
    fn an_entry_pointing_at_a_missing_route_or_action_fails() {
        assert!(
            errors(|m| m["commands"]["status"]["route"] = "nowhere".into())
                .contains("route `nowhere` is not in routes")
        );
        assert!(
            errors(|m| m["commands"]["status"]["action"] = "nothing.run".into())
                .contains("action `nothing.run` is not in actions")
        );
        assert!(
            errors(|m| m["commands"]["status"]["action"] = "servers.list".into())
                .contains("belongs to route `servers`")
        );
    }

    #[test]
    fn a_surface_that_differs_from_the_registry_fails() {
        assert!(
            errors(|m| m["commands"]["direct run"]["surface"] = "screen".into())
                .contains("the registry says terminal")
        );
        assert!(
            errors(|m| m["tools"]["servers_list"]["surface"] = "terminal".into())
                .contains("tool `servers_list`: surface is terminal")
        );
    }

    #[test]
    fn a_built_action_needs_a_component_test_that_names_it() {
        assert!(errors(|m| {
            m["actions"]["servers.list"]
                .as_object_mut()
                .unwrap()
                .remove("test");
        })
        .contains("built but has no component test"));
        assert!(
            errors(|m| m["actions"]["servers.list"]["test"] = "src/None.test.tsx".into())
                .contains("does not exist")
        );
        assert!(
            errors(|m| m["actions"]["servers.list"]["test"] = "src/Servers.tsx".into())
                .contains("never names the action")
        );
    }

    #[test]
    fn built_routes_dead_actions_and_unowned_groups_fail() {
        assert!(errors(|m| {
            m["routes"]["servers"]
                .as_object_mut()
                .unwrap()
                .remove("component");
        })
        .contains("built but names no component"));
        assert!(
            errors(|m| m["routes"]["servers"]["status"] = "planned".into())
                .contains("is built but its route `servers` is not")
        );
        assert!(errors(|m| {
            m["actions"]["unused"] =
                json!({"route": "servers", "status": "planned", "summary": "x"});
        })
        .contains("action `unused` is not used by any command or tool"));
        assert!(errors(|m| {
            m["owners"].as_object_mut().unwrap().remove("direct");
        })
        .contains("group `direct` has no owner"));
    }

    #[test]
    fn dotted_and_camel_case_names_are_not_skipped() {
        let mut registry = registry();
        registry["commands"].as_array_mut().unwrap().push(json!(
            {"id": "context bundle apply", "kind": "command",
             "path": ["context", "bundle", "apply"], "surface": "screen"}
        ));
        registry["tools"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "context_bundle_apply", "command": null}));
        let message = check_parity(&good(), &registry, &scratch_repo())
            .errors
            .join("\n");
        assert!(
            message.contains("command `context bundle apply` has no entry"),
            "{message}"
        );
        assert!(
            message.contains("tool `context_bundle_apply` has no entry"),
            "{message}"
        );
    }
}
