use super::commands_json::{meta_for, registry, row_flags, GLOBAL, OVERRIDES};
use super::policy::{
    row_of, sub_spec, terminal_only, Needs, Preview, Surface, Tier, ToolPreview, ROWS, SUBS,
    TOOL_ROWS,
};
use super::{find_command, COMMANDS};
use crate::plus::selfmcp::{Gate, ToolDef, TOOLS};
use std::collections::BTreeSet;

fn all_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for command in COMMANDS {
        let id = command.path.join(" ");
        ids.extend(SUBS.iter().filter(|s| s.parent == id).map(|s| s.id()));
        ids.insert(
            ids.len() - SUBS.iter().filter(|s| s.parent == id).count(),
            id,
        );
    }
    ids
}

fn leaf_ids() -> BTreeSet<String> {
    let ids = all_ids();
    ids.iter()
        .filter(|id| {
            !ids.iter()
                .any(|other| other.len() > id.len() && other.starts_with(&format!("{id} ")))
        })
        .cloned()
        .collect()
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ids_are_unique_and_every_sub_row_has_a_parent_row() {
    let ids = all_ids();
    let unique: BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "duplicate command ids");
    for sub in SUBS {
        assert!(
            COMMANDS.iter().any(|c| c.path.join(" ") == sub.parent),
            "{}: no such parent row",
            sub.id()
        );
        assert!(!sub.summary.is_empty(), "{}", sub.id());
    }
}

#[test]
fn every_leaf_command_has_a_policy_row_and_every_policy_row_a_command() {
    let leaves = leaf_ids();
    let rows: Vec<&str> = ROWS.iter().map(|r| r.id).collect();
    let unique: BTreeSet<&str> = rows.iter().copied().collect();
    assert_eq!(unique.len(), rows.len(), "duplicate policy rows");
    let without_policy: Vec<&String> = leaves
        .iter()
        .filter(|id| !unique.contains(id.as_str()))
        .collect();
    assert!(
        without_policy.is_empty(),
        "commands without a row in ctl/policy.rs: {without_policy:?}"
    );
    let without_command: Vec<&&str> = rows.iter().filter(|id| !leaves.contains(**id)).collect();
    assert!(
        without_command.is_empty(),
        "policy rows without a leaf command: {without_command:?}"
    );
    assert_eq!(leaves.len(), ROWS.len());
    assert_eq!(
        leaves.len(),
        133,
        "effective commands: 107 leaf rows and 26 sub rows"
    );
}

#[test]
fn group_rows_carry_no_policy_and_the_planned_stubs_are_groups() {
    let leaves = leaf_ids();
    for command in COMMANDS {
        let id = command.path.join(" ");
        if !leaves.contains(&id) {
            assert!(row_of(&id).is_none(), "{id} is a group");
        }
    }
    for stub in ["server", "secret", "import"] {
        assert!(!leaves.contains(stub), "{stub}");
    }
}

#[test]
fn dispatching_rows_list_every_sub_command_and_reach_its_handler() {
    let parents: BTreeSet<&str> = SUBS.iter().map(|s| s.parent).collect();
    assert_eq!(
        parents.into_iter().collect::<Vec<_>>(),
        [
            "cc",
            "compression ledger",
            "compression proxy",
            "council",
            "mcp",
            "sync"
        ]
    );
    let _lock = crate::registry::data_dir_test_lock();
    let dir = std::env::temp_dir().join(format!("policy-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let _data = crate::registry::DataDirOverride::set(&dir);
    for parent in [
        "sync",
        "council",
        "mcp",
        "cc",
        "compression proxy",
        "compression ledger",
    ] {
        let command = COMMANDS
            .iter()
            .find(|c| c.path.join(" ") == parent)
            .expect("a dispatching row");
        let usage = match (command.handler)(&[]) {
            Err(error) => error.message,
            Ok(_) if parent == "compression ledger" => String::from("summary record"),
            Ok(_) => panic!("{parent}: a bare call must print its usage"),
        };
        for sub in SUBS.iter().filter(|s| s.parent == parent) {
            assert!(
                usage.contains(sub.name),
                "{parent}: the usage text {usage:?} does not name {}",
                sub.name
            );
        }
    }
    for sub in SUBS
        .iter()
        .filter(|s| ["sync", "council", "mcp", "compression ledger"].contains(&s.parent))
    {
        let command = COMMANDS
            .iter()
            .find(|c| c.path.join(" ") == sub.parent)
            .unwrap();
        let error = (command.handler)(&strings(&[sub.name, "--no-such-flag-zz"]))
            .err()
            .unwrap_or_else(|| panic!("{}: a bogus flag must fail", sub.id()));
        assert!(
            error.message.contains("--no-such-flag-zz"),
            "{} did not reach its flag parser: {}",
            sub.id(),
            error.message
        );
    }
    for sub in SUBS.iter().filter(|s| s.parent == "sync") {
        assert!(sub_spec(&sub.id()).is_some(), "{}", sub.id());
    }
}

#[test]
fn dry_run_in_the_policy_equals_what_the_parser_accepts() {
    for row in ROWS {
        let flags = row_flags(row.id, Some(row));
        let has = |name: &str| flags.iter().any(|f| f.name() == name);
        match row.preview {
            Preview::None => assert!(
                !has("--dry-run") && !has("--plan"),
                "{}: the parser takes a preview flag but the row says none",
                row.id
            ),
            Preview::Flag(flag) | Preview::UnlessApplied(flag) => assert!(
                has(flag),
                "{}: the preview flag {flag} is not a flag of the command",
                row.id
            ),
        }
        if let Preview::UnlessApplied(flag) = row.preview {
            assert!(row.escalators.contains(&flag), "{}", row.id);
        }
    }
}

#[test]
fn declared_flags_operands_and_groups_name_real_flags() {
    for row in ROWS {
        let flags = row_flags(row.id, Some(row));
        let names: Vec<&str> = flags.iter().map(|f| f.name()).collect();
        for list in [row.requires, row.escalators, row.only] {
            for name in list {
                assert!(names.contains(name), "{}: {name} is not a flag", row.id);
            }
        }
        for group in row.one_of {
            assert!(group.len() >= 2, "{}", row.id);
            for name in *group {
                assert!(names.contains(name), "{}: {name} is not a flag", row.id);
            }
        }
        if row.reads_by_default {
            assert!(
                row.tier > Tier::Read,
                "{}: reads_by_default on a read row",
                row.id
            );
            assert!(
                !row.escalators.is_empty() || row.operand_escalates,
                "{}",
                row.id
            );
        } else {
            assert!(
                !row.operand_escalates,
                "{}: operand_escalates needs reads_by_default",
                row.id
            );
            assert!(
                row.escalators.is_empty() || matches!(row.preview, Preview::UnlessApplied(_)),
                "{}",
                row.id
            );
        }
        let limit = row
            .specs
            .iter()
            .map(|spec| spec.operand_limit())
            .try_fold(None, |acc: Option<usize>, next| match (acc, next) {
                (_, None) => Some(acc),
                (None, Some(n)) => Some(Some(n)),
                (Some(a), Some(n)) => Some(Some(a.max(n))),
            })
            .flatten();
        let count = row.operands.len();
        let variadic = row.operands.iter().any(|o| o.variadic);
        if let Some(limit) = limit {
            assert!(
                count <= limit && !variadic || row.specs.is_empty(),
                "{}: {count} operands against a parser that takes {limit}",
                row.id
            );
        }
        let required = row.operands.iter().filter(|o| o.required).count();
        assert!(
            row.operands.iter().skip(required).all(|o| !o.required),
            "{}: required operands come first",
            row.id
        );
    }
    for sub in SUBS.iter().filter(|s| s.parent == "sync") {
        let (spec, positional) = sub_spec(&sub.id()).unwrap();
        let row = row_of(&sub.id()).unwrap();
        assert_eq!(
            row.operands.iter().map(|o| o.name).collect::<Vec<_>>(),
            positional,
            "{}",
            sub.id()
        );
        assert_eq!(spec.operand_limit(), None);
    }
}

#[test]
fn every_flag_has_a_type_and_an_effect() {
    let mut used = BTreeSet::new();
    for row in ROWS {
        for flag in row_flags(row.id, Some(row)) {
            used.insert(flag.name());
            let meta = meta_for(row.id, flag.name())
                .unwrap_or_else(|| panic!("{}: no metadata for {}", row.id, flag.name()));
            assert!(!meta.effect.is_empty(), "{} {}", row.id, flag.name());
            use super::commands_json::Ty;
            match meta.ty {
                Ty::Bool => assert!(
                    !flag.takes_value(),
                    "{} {}: a bool takes no value",
                    row.id,
                    flag.name()
                ),
                Ty::Paths => assert!(flag.is_greedy(), "{} {}", row.id, flag.name()),
                Ty::Choice(choices) => {
                    assert!(choices.len() >= 2);
                    assert!(flag.takes_value(), "{} {}", row.id, flag.name());
                }
                _ => assert!(
                    flag.takes_value() && !flag.is_greedy(),
                    "{} {}",
                    row.id,
                    flag.name()
                ),
            }
            if flag.is_whole_number() {
                assert_eq!(meta.ty, Ty::Int, "{} {}", row.id, flag.name());
            }
        }
    }
    for meta in GLOBAL {
        assert!(
            used.contains(meta.flag),
            "{} is not used by any command",
            meta.flag
        );
    }
    for (id, meta) in OVERRIDES {
        let row = row_of(id).unwrap_or_else(|| panic!("override for unknown row {id}"));
        assert!(
            row_flags(id, Some(row))
                .iter()
                .any(|f| f.name() == meta.flag),
            "{id}: override for {} which the command does not take",
            meta.flag
        );
    }
    let globals: BTreeSet<&str> = GLOBAL.iter().map(|m| m.flag).collect();
    assert_eq!(
        globals.len(),
        GLOBAL.len(),
        "duplicate global flag metadata"
    );
}

#[test]
fn terminal_rows_and_the_terminal_need_agree_and_the_bridge_guard_follows_them() {
    for row in ROWS {
        assert_eq!(
            row.surface == Surface::Terminal,
            row.needs.contains(&Needs::TerminalOnly),
            "{}",
            row.id
        );
    }
    let terminal: Vec<&str> = ROWS
        .iter()
        .filter(|r| r.surface == Surface::Terminal)
        .map(|r| r.id)
        .collect();
    assert_eq!(terminal, ["direct run", "compression run"]);

    for refused in [
        vec!["direct", "run", "srv-alpha"],
        vec!["--json", "direct", "run", "srv-alpha"],
        vec!["compression", "run"],
        vec!["compression", "run", "--cwd", "/tmp/x"],
        vec!["compression", "run", "--", "--plan"],
        vec!["compression", "run", "-p", "--plan"],
    ] {
        assert!(terminal_only(&strings(&refused)).is_some(), "{refused:?}");
    }
    for allowed in [
        vec!["compression", "run", "--plan"],
        vec!["--json", "compression", "run", "--plan", "--cwd", "/tmp/x"],
        vec!["compression", "status"],
        vec!["status"],
        vec!["sync", "push"],
        vec!["nonsense"],
    ] {
        assert!(terminal_only(&strings(&allowed)).is_none(), "{allowed:?}");
    }
}

fn tool_preview(tool: &ToolDef) -> ToolPreview {
    match tool.params.iter().find(|p| p.name == "dry_run") {
        None => ToolPreview::None,
        Some(param) if param.default == Some(true) => ToolPreview::DefaultOn,
        Some(_) => ToolPreview::Param,
    }
}

#[test]
fn every_self_mcp_tool_has_one_policy_row_with_the_same_tier_and_dry_run_default() {
    let names: Vec<&str> = TOOL_ROWS.iter().map(|t| t.name).collect();
    let unique: BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(unique.len(), names.len(), "duplicate tool rows");
    assert_eq!(TOOLS.len(), TOOL_ROWS.len());
    for tool in TOOLS {
        let row = TOOL_ROWS
            .iter()
            .find(|t| t.name == tool.name)
            .unwrap_or_else(|| panic!("tool {} has no row in ctl/policy.rs", tool.name));
        assert_eq!(
            row.tier,
            Tier::of_tool(tool.tier),
            "{}: tier {} in the catalog",
            tool.name,
            tool.tier
        );
        assert_eq!(
            row.preview,
            tool_preview(tool),
            "{}: dry_run default in the catalog",
            tool.name
        );
        match row.tier {
            Tier::Read => assert_eq!(tool.gate, Gate::None, "{}", tool.name),
            _ => assert!(tool.tier >= 2, "{}", tool.name),
        }
    }
    for row in TOOL_ROWS {
        assert!(
            TOOLS.iter().any(|t| t.name == row.name),
            "policy row for the unknown tool {}",
            row.name
        );
    }
}

#[test]
fn a_tool_that_maps_to_a_command_agrees_with_it() {
    let mut mapped = 0;
    for tool in TOOL_ROWS {
        let Some(command) = tool.command else {
            continue;
        };
        mapped += 1;
        let row = row_of(command).unwrap_or_else(|| {
            panic!(
                "{}: maps to {command}, which is not a leaf command",
                tool.name
            )
        });
        assert_eq!(
            row.tier, tool.tier,
            "{} ({command}): the tool and the command differ in tier",
            tool.name
        );
        if tool.preview == ToolPreview::DefaultOn {
            assert!(
                row.preview != Preview::None,
                "{} previews by default but {command} has no preview",
                tool.name
            );
        }
        if row.preview == Preview::None {
            assert!(
                tool.preview == ToolPreview::None,
                "{} takes dry_run but {command} has no preview flag",
                tool.name
            );
        }
    }
    assert!(mapped >= 50, "{mapped} tools map to a command");
}

#[test]
fn the_registry_lists_every_row_with_the_policy_fields() {
    let data = registry();
    let rows = data["commands"].as_array().unwrap();
    let ids: Vec<String> = rows
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        ids,
        all_ids()
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(data["counts"]["rows"], rows.len());
    assert_eq!(data["counts"]["commands"], 133);
    assert_eq!(data["counts"]["tools"], TOOLS.len());
    for row in rows {
        let id = row["id"].as_str().unwrap();
        let words: Vec<String> = id.split(' ').map(String::from).collect();
        assert!(find_command(&words).is_some(), "{id}");
        match row["kind"].as_str().unwrap() {
            "command" => {
                assert!(
                    ["read", "write", "destructive"].contains(&row["tier"].as_str().unwrap()),
                    "{id}"
                );
                assert!(
                    row["flags"].is_array() && row["operands"].is_array(),
                    "{id}"
                );
                for flag in row["flags"].as_array().unwrap() {
                    assert!(flag["name"].as_str().unwrap().starts_with('-'), "{id}");
                    assert!(!flag["effect"].as_str().unwrap().is_empty(), "{id} {flag}");
                }
            }
            "group" => assert!(row["tier"].is_null(), "{id}"),
            other => panic!("{id}: kind {other}"),
        }
    }
    let unique: BTreeSet<&str> = data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(unique.len(), TOOLS.len());
}
