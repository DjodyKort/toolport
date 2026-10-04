//! Wildcard permission rules (`server__prefix*`) in `rename-refs`: which ones map, which are
//! reported and why, the separate list for servers that are gone, and the apply refusal that
//! keeps an `ask` or `deny` rule from silently losing its gate.

use super::name_map::{exposed_tool_name, old_tool_name};
use super::rename_refs::Orphan;
use crate::router::sanitize_segment;
use super::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn shop_map() -> NameMap {
    let pairs = [
        (
            "shop",
            "shop",
            &[
                "create_order",
                "create_note",
                "update_order",
                "delete_order",
                "get-order",
                "get_item",
                "list_orders",
                "search",
            ][..],
        ),
        ("shop-eu", "shopeu", &["create_order", "list_orders"][..]),
    ];
    let mut map = BTreeMap::new();
    let mut servers = BTreeMap::new();
    for (name, id, tools) in pairs {
        servers.insert(name.to_string(), id.to_string());
        for tool in tools {
            map.insert(old_tool_name(name, tool), exposed_tool_name(id, tool));
        }
    }
    let mut imported: std::collections::BTreeSet<String> = servers.keys().cloned().collect();
    imported.insert("quiet".to_string());
    NameMap {
        imported,
        map,
        servers,
    }
}

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rename-wild-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

fn miss(text: &str) -> Vec<String> {
    rewrite_text(text, &shop_map()).2
}

const SETTINGS: &str = r#"{
  "permissions": {
    "allow": ["mcp__mcpm_shop__search", "mcp__mcpm_playwright__browser_click"],
    "ask": [
      "mcp__mcpm_shop__create_*",
      "mcp__mcpm_shop__update_*",
      "mcp__mcpm_shop__delete_*",
      "mcp__mcpm_shop-eu__create_*",
      "mcp__mcpm_playwright__*"
    ],
    "deny": ["mcp__mcpm_shop__get-*"]
  }
}
"#;

#[test]
fn server_and_prefix_wildcards_map_through_the_name_map() {
    let (out, n, orphans) = rewrite_text(
        "mcp__mcpm_shop__create_* mcp__mcpm_shop__* mcp__mcpm_shop-eu__create_* mcp__mcpm_shop__list_o*",
        &shop_map(),
    );
    assert_eq!(n, 4);
    assert!(orphans.is_empty(), "{orphans:?}");
    assert_eq!(
        out,
        "mcp__toolport__shop__create_* mcp__toolport__shop__* mcp__toolport__shopeu__create_* mcp__toolport__shop__list_o*"
    );
}

#[test]
fn a_wildcard_that_matches_no_tool_today_still_maps_as_a_prefix() {
    let (out, n, orphans) = rewrite_text("mcp__mcpm_shop__archive_*", &shop_map());
    assert_eq!((n, orphans.len()), (1, 0));
    assert_eq!(out, "mcp__toolport__shop__archive_*");
}

#[test]
fn bold_markers_after_a_reference_are_not_a_wildcard() {
    let (out, n, orphans) = rewrite_text("**mcp__mcpm_shop__search** and *mcp__mcpm_shop__search*", &shop_map());
    assert_eq!((n, orphans.len()), (2, 0));
    assert_eq!(
        out,
        "**mcp__toolport__shop__search** and *mcp__toolport__shop__search*"
    );
}

#[test]
fn unmappable_wildcards_are_listed_with_the_reason_and_left_alone() {
    let text = "mcp__mcpm_shop__get-* mcp__mcpm_sh* mcp__mcpm_shop__create*x mcp__mcpm_quiet__x*";
    let (out, n, refs) = rewrite_text(text, &shop_map());
    assert_eq!(n, 0);
    assert_eq!(out, text);
    assert_eq!(
        refs,
        vec![
            "mcp__mcpm_quiet__x*",
            "mcp__mcpm_sh*",
            "mcp__mcpm_shop__create*",
            "mcp__mcpm_shop__get-*",
        ]
    );

    let root = scratch("reasons");
    std::fs::write(root.join("a.md"), text).unwrap();
    let report = rename_refs(&[root.clone()], &shop_map(), true).unwrap();
    assert!(report.dead.is_empty());
    let reason = |reference: &str| {
        report
            .orphans
            .iter()
            .find(|o| o.reference == reference)
            .unwrap_or_else(|| panic!("{reference} missing: {:?}", report.orphans))
            .reason
            .clone()
    };
    assert!(reason("mcp__mcpm_shop__get-*").contains("would also match mcp__toolport__shop__get_item"));
    assert!(reason("mcp__mcpm_sh*").contains("cuts a server name"));
    assert!(reason("mcp__mcpm_shop__create*").contains("not at the end"));
    assert!(reason("mcp__mcpm_quiet__x*").contains("no entry in the tool manifest"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_wildcard_over_two_servers_is_not_mappable() {
    let mut map = shop_map();
    map.servers.insert("shop__create".to_string(), "shopcreate".to_string());
    map.imported.insert("shop__create".to_string());
    map.map.insert(
        old_tool_name("shop__create", "order"),
        exposed_tool_name("shopcreate", "order"),
    );
    let (out, n, refs) = rewrite_text("mcp__mcpm_shop__create*", &map);
    assert_eq!(n, 0);
    assert_eq!(out, "mcp__mcpm_shop__create*");
    assert_eq!(refs, vec!["mcp__mcpm_shop__create*"]);
}

#[test]
fn removed_servers_are_dead_not_orphans() {
    let root = scratch("dead");
    std::fs::write(
        root.join("a.md"),
        "mcp__mcpm_playwright__browser_click mcp__mcpm_playwright__* mcp__mcpm_shop__nope mcp__mcpm_shop__search",
    )
    .unwrap();
    let report = rename_refs(&[root.clone()], &shop_map(), false).unwrap();
    let refs = |list: &[Orphan]| list.iter().map(|o| o.reference.clone()).collect::<Vec<_>>();
    assert_eq!(
        refs(&report.dead),
        vec!["mcp__mcpm_playwright__*", "mcp__mcpm_playwright__browser_click"]
    );
    assert_eq!(refs(&report.orphans), vec!["mcp__mcpm_shop__nope"]);
    assert_eq!(report.replaced, 1);
    assert!(report.summary().contains("1 orphans, 2 dead"));
    assert!(report
        .summary()
        .contains("dead mcp__mcpm_playwright__* in "));
    assert_eq!(
        read(&root.join("a.md")),
        "mcp__mcpm_playwright__browser_click mcp__mcpm_playwright__* mcp__mcpm_shop__nope mcp__toolport__shop__search"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dry_run_reports_the_blocking_rule_and_apply_refuses_without_writing() {
    let root = scratch("refuse");
    let settings = root.join("settings.json");
    let other = root.join("notes.md");
    std::fs::write(&settings, SETTINGS).unwrap();
    std::fs::write(&other, "use mcp__mcpm_shop__search\n").unwrap();

    let dry = rename_refs(&[root.clone()], &shop_map(), true).unwrap();
    assert_eq!(dry.replaced, 6);
    let blocking = dry.blocking();
    assert_eq!(blocking.len(), 1, "{:?}", dry.orphans);
    assert_eq!(blocking[0].reference, "mcp__mcpm_shop__get-*");
    assert_eq!(blocking[0].rule.as_deref(), Some("deny"));
    let dead: Vec<_> = dry.dead.iter().map(|o| (o.reference.as_str(), o.rule.as_deref())).collect();
    assert_eq!(
        dead,
        vec![
            ("mcp__mcpm_playwright__*", Some("ask")),
            ("mcp__mcpm_playwright__browser_click", None),
        ]
    );
    assert!(dry.summary().contains("error: apply would refuse"));
    assert_eq!(read(&settings), SETTINGS);

    let err = rename_refs(&[root.clone()], &shop_map(), false).unwrap_err();
    assert!(err.contains("1 ask/deny rule(s) would be left without a replacement"), "{err}");
    assert!(err.contains("mcp__mcpm_shop__get-* in "), "{err}");
    assert!(err.contains("[deny rule]"), "{err}");
    assert_eq!(read(&settings), SETTINGS, "nothing is written when a gate would be lost");
    assert_eq!(read(&other), "use mcp__mcpm_shop__search\n");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn apply_rewrites_the_gates_once_the_unmappable_rule_is_gone() {
    let root = scratch("apply");
    let settings = root.join("settings.json");
    std::fs::write(
        &settings,
        SETTINGS.replace(r#""deny": ["mcp__mcpm_shop__get-*"]"#, r#""deny": []"#),
    )
    .unwrap();
    let report = rename_refs(&[root.clone()], &shop_map(), false).unwrap();
    assert!(report.blocking().is_empty());
    assert_eq!(report.replaced, 5);
    assert_eq!(report.dead.len(), 2);
    let after: serde_json::Value = serde_json::from_str(&read(&settings)).unwrap();
    assert_eq!(
        after["permissions"]["ask"],
        serde_json::json!([
            "mcp__toolport__shop__create_*",
            "mcp__toolport__shop__update_*",
            "mcp__toolport__shop__delete_*",
            "mcp__toolport__shopeu__create_*",
            "mcp__mcpm_playwright__*"
        ])
    );
    assert_eq!(
        after["permissions"]["allow"],
        serde_json::json!(["mcp__toolport__shop__search", "mcp__mcpm_playwright__browser_click"])
    );

    let again = rename_refs(&[root.clone()], &shop_map(), false).unwrap();
    assert_eq!(again.replaced, 0);
    assert_eq!(again.dead.len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_unmappable_pattern_in_an_allow_list_is_reported_but_does_not_block() {
    let root = scratch("allow");
    let settings = root.join("settings.json");
    std::fs::write(
        &settings,
        r#"{"permissions":{"allow":["mcp__mcpm_shop__get-*"],"ask":["mcp__mcpm_shop__create_*"]}}"#,
    )
    .unwrap();
    let report = rename_refs(&[root.clone()], &shop_map(), false).unwrap();
    assert_eq!(report.orphans.len(), 1);
    assert_eq!(report.orphans[0].rule, None);
    assert!(report.blocking().is_empty());
    assert_eq!(
        read(&settings),
        r#"{"permissions":{"allow":["mcp__mcpm_shop__get-*"],"ask":["mcp__toolport__shop__create_*"]}}"#
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_whole_server_reference_is_an_orphan_not_a_dead_one() {
    assert_eq!(miss("mcp__mcpm_shop"), vec!["mcp__mcpm_shop"]);
    let root = scratch("whole");
    std::fs::write(root.join("a.json"), r#"{"deny":["mcp__mcpm_shop"]}"#).unwrap();
    let report = rename_refs(&[root.clone()], &shop_map(), true).unwrap();
    assert!(report.dead.is_empty());
    assert_eq!(report.blocking().len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

fn overlap_map() -> NameMap {
    let servers = [
        ("alpha", "alpha"),
        ("alpha-extra", "alphax"),
        ("a", "a"),
        ("a-b", "ab"),
    ];
    let tools = [
        "search",
        "list-items",
        "list_items",
        "get_item",
        "a",
        "a-b",
        "a_b",
        "a-b-c",
        "extra__search",
    ];
    let mut map = BTreeMap::new();
    let mut by_name = BTreeMap::new();
    for (name, id) in servers {
        by_name.insert(name.to_string(), id.to_string());
        for tool in tools {
            map.insert(old_tool_name(name, tool), exposed_tool_name(id, tool));
        }
    }
    NameMap {
        imported: by_name.keys().cloned().collect(),
        map,
        servers: by_name,
    }
}

#[test]
fn a_wildcard_maps_exactly_when_the_new_pattern_matches_the_renamed_tools() {
    use crate::plus::randutil::run_cases;
    use std::collections::BTreeSet;
    let map = overlap_map();
    let servers: Vec<(String, String)> = map
        .servers
        .iter()
        .map(|(name, id)| (name.clone(), id.clone()))
        .collect();
    let tools = [
        "search",
        "list-items",
        "list_items",
        "get_item",
        "a",
        "a-b",
        "a_b",
        "a-b-c",
        "extra__search",
    ];
    let (mut mapped, mut refused) = (0, 0);
    run_cases("rename-refs-wildcard-model", 3000, |_, rng| {
        let (name, id) = rng.pick(&servers);
        let tool = rng.pick(&tools);
        let prefix = &tool[..rng.below(tool.len() + 1)];
        let body = format!("mcp__mcpm_{name}__{prefix}");
        let text = format!("{body}* ");
        let (out, n, refs) = rewrite_text(&text, &map);
        let image: BTreeSet<&String> = map
            .map
            .iter()
            .filter(|(old, _)| old.starts_with(&body))
            .map(|(_, new)| new)
            .collect();
        let new_body = format!("{}{}", exposed_prefix(id), sanitize_segment(prefix));
        let matched: BTreeSet<&String> = map
            .map
            .values()
            .filter(|new| new.starts_with(&new_body))
            .collect();
        if n == 1 {
            assert_eq!(out, format!("{new_body}* "), "{text:?}");
            assert_eq!(matched, image, "{text:?}");
            assert!(refs.is_empty());
            mapped += 1;
        } else {
            assert_eq!((out.as_str(), refs.len()), (text.as_str(), 1), "{text:?}");
            assert_ne!(matched, image, "{text:?}");
            refused += 1;
        }
    });
    assert!(mapped > 500 && refused > 100, "{mapped} mapped, {refused} refused");
}
