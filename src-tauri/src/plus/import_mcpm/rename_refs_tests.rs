use super::name_map;
use super::rename_refs::collect;
use super::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn nm() -> NameMap {
    let pairs = [
        ("alpha", "alpha", &["list-items", "get_item", "search"][..]),
        ("alpha-extra", "alphax", &["search"][..]),
        ("beta", "beta", &["create-note", "list-items"][..]),
    ];
    let mut map = BTreeMap::new();
    let mut servers = BTreeMap::new();
    for (name, id, tools) in pairs {
        servers.insert(name.to_string(), id.to_string());
        for t in tools {
            map.insert(
                name_map::old_tool_name(name, t),
                name_map::exposed_tool_name(id, t),
            );
        }
    }
    NameMap { map, servers }
}

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rename-refs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn body(i: usize) -> String {
    match i % 5 {
        0 => format!("Rule {i}: call `mcp__mcpm_alpha__list-items` first.\n"),
        1 => {
            format!("Rule {i}: use mcp__mcpm_beta__create-note, then mcp__mcpm_alpha__get_item.\n")
        }
        2 => format!(
            "Rule {i}: allowed-tools: mcp__mcpm_alpha-extra__search, mcp__mcpm_beta__list-items\n"
        ),
        3 => format!("Rule {i}: no tool references here.\n"),
        _ => format!("Rule {i}: (mcp__mcpm_alpha__search) and \"mcp__mcpm_alpha__list-items\".\n"),
    }
}

fn populate(root: &Path) {
    for i in 0..35 {
        let p = root.join(format!("rules/rule-{i:02}.md"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body(i)).unwrap();
    }
    for i in 0..15 {
        let (dir, name) = if i % 2 == 0 {
            ("skills/demo", format!("SKILL-{i:02}.md"))
        } else {
            ("agents", format!("agent-{i:02}.md"))
        };
        let p = root.join(dir).join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body(i + 100)).unwrap();
    }
}

fn snapshot(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    collect(root, &mut files).unwrap();
    files
        .into_iter()
        .map(|p| {
            (
                p.display().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect()
}

#[test]
fn rewrites_known_references_and_leaves_prose() {
    let (out, n, orphans) = rewrite_text(
        "a mcp__mcpm_alpha__list-items, mcp__mcpm_alpha-extra__search.\nmcp__mcpm_beta__create-note-",
        &nm(),
    );
    assert_eq!(n, 3);
    assert!(orphans.is_empty());
    assert_eq!(
        out,
        "a mcp__toolport__alpha__list_items, mcp__toolport__alphax__search.\nmcp__toolport__beta__create_note-"
    );
}

#[test]
fn unknown_references_are_orphans_and_untouched() {
    let (out, n, orphans) = rewrite_text("mcp__mcpm_gamma__thing and mcp__mcpm_alpha__nope", &nm());
    assert_eq!(n, 0);
    assert_eq!(out, "mcp__mcpm_gamma__thing and mcp__mcpm_alpha__nope");
    assert_eq!(
        orphans,
        vec!["mcp__mcpm_alpha__nope", "mcp__mcpm_gamma__thing"]
    );
}

#[test]
fn fifty_files_rewrite_then_rerun_is_a_noop() {
    let root = scratch("fifty");
    populate(&root);
    let map = nm();
    let dry = rename_refs(&[root.clone()], &map, true).unwrap();
    assert_eq!(dry.scanned, 50);
    assert!(dry.changed());
    assert_eq!(
        snapshot(&root)
            .iter()
            .filter(|(_, t)| t.contains("mcpm_"))
            .count(),
        40
    );

    let first = rename_refs(&[root.clone()], &map, false).unwrap();
    assert_eq!(first.files.len(), 40);
    assert_eq!(first.replaced, dry.replaced);
    assert!(first.orphans.is_empty());
    let after = snapshot(&root);
    assert!(after.iter().all(|(_, t)| !t.contains("mcpm_")));

    let second = rename_refs(&[root.clone()], &map, false).unwrap();
    assert!(!second.changed());
    assert_eq!(second.replaced, 0);
    assert_eq!(snapshot(&root), after);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn orphans_are_listed_with_their_file() {
    let root = scratch("orphans");
    std::fs::write(
        root.join("a.md"),
        "mcp__mcpm_ghost__tool mcp__mcpm_beta__list-items",
    )
    .unwrap();
    std::fs::write(root.join("b.bin"), "mcp__mcpm_ghost__other").unwrap();
    let r = rename_refs(&[root.clone()], &nm(), false).unwrap();
    assert_eq!(r.orphans.len(), 1);
    assert_eq!(r.orphans[0].reference, "mcp__mcpm_ghost__tool");
    assert!(r.orphans[0].path.ends_with("a.md"));
    assert!(r.summary().contains("1 orphans"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dry_run_writes_nothing_and_missing_root_errors() {
    let root = scratch("dry");
    std::fs::write(root.join("a.md"), "mcp__mcpm_alpha__search").unwrap();
    let r = rename_refs(&[root.clone()], &nm(), true).unwrap();
    assert_eq!(r.replaced, 1);
    assert_eq!(
        std::fs::read_to_string(root.join("a.md")).unwrap(),
        "mcp__mcpm_alpha__search"
    );
    assert!(rename_refs(&[root.join("missing")], &nm(), true).is_err());
    let _ = std::fs::remove_dir_all(&root);
}
