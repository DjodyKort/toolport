//! Seeded randomized and edge-case tests for the tool-reference rewrite: overlapping server
//! prefixes, punctuation next to references, idempotence, huge inputs and the file walk.

use super::name_map::{exposed_tool_name, old_tool_name};
use super::rename_refs::collect;
use super::*;
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

const SERVERS: &[(&str, &str)] = &[
    ("alpha", "alpha"),
    ("alpha-extra", "alphax"),
    ("alpha_extra", "alpha-u"),
    ("alpha-extra-two", "alphax2"),
    ("beta", "beta"),
    ("a", "a"),
    ("a-b", "ab"),
];

const TOOLS: &[&str] = &[
    "search",
    "list-items",
    "get_item",
    "a",
    "a-b",
    "a_b",
    "a-b-c",
    "extra__search",
    "x",
];

const SEPARATORS: &[&str] = &[
    " ", "\n", "\t", ".", ",", ";", ":", "(", ")", "[", "]", "{", "}", "\"", "'", "`", "/", "\\",
    "|", "<", ">", "=", "+", "*", "&", "!", "?", "~", "日本", "é", "🙂", ". ", ", ", "`\n",
];

fn map() -> NameMap {
    let mut map = BTreeMap::new();
    let mut servers = BTreeMap::new();
    for (name, id) in SERVERS {
        servers.insert(name.to_string(), id.to_string());
        for tool in TOOLS {
            map.insert(old_tool_name(name, tool), exposed_tool_name(id, tool));
        }
    }
    NameMap { map, servers }
}

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Independent re-implementation of the scan: leftmost `mcp__mcpm_` token, longest known prefix
/// that ends before a `-` or `_`.
fn expected_rewrite(text: &str, map: &NameMap) -> (String, usize, BTreeSet<String>) {
    const PREFIX: &str = "mcp__mcpm_";
    let (mut out, mut replaced, mut orphans) = (String::new(), 0, BTreeSet::new());
    let mut rest = text;
    while let Some(at) = rest.find(PREFIX) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let len = tail
            .char_indices()
            .find(|(_, c)| !token_char(*c))
            .map_or(tail.len(), |(i, _)| i);
        if len == PREFIX.len() {
            out.push_str(PREFIX);
            rest = &tail[PREFIX.len()..];
            continue;
        }
        let token = &tail[..len];
        let mut cuts: Vec<usize> = token
            .char_indices()
            .filter(|(i, c)| (*c == '-' || *c == '_') && *i > PREFIX.len())
            .map(|(i, _)| i)
            .collect();
        cuts.push(token.len());
        match cuts
            .iter()
            .rev()
            .find(|c| map.map.contains_key(&token[..**c]))
        {
            Some(cut) => {
                replaced += 1;
                out.push_str(&map.map[&token[..*cut]]);
                out.push_str(&token[*cut..]);
            }
            None => {
                out.push_str(token);
                orphans.insert(token.trim_end_matches(['-', '_']).to_string());
            }
        }
        rest = &tail[len..];
    }
    out.push_str(rest);
    (out, replaced, orphans)
}

fn reference(rng: &mut Rng, map: &NameMap) -> String {
    match rng.below(10) {
        0 => format!("mcp__mcpm_{}", rng.string("abx_-", 10)),
        1 => "mcp__mcpm_".to_string(),
        2 => format!(
            "{}{}",
            rng.pick(&map.map.keys().collect::<Vec<_>>()),
            rng.string("-_x", 3)
        ),
        _ => rng.pick(&map.map.keys().collect::<Vec<_>>()).to_string(),
    }
}

fn document(rng: &mut Rng, map: &NameMap) -> String {
    let mut text = String::new();
    for _ in 0..rng.range(0, 12) {
        match rng.below(4) {
            0 => text.push_str(&rng.garbage(8).replace("mcp__", "")),
            _ => text.push_str(&reference(rng, map)),
        }
        text.push_str(rng.pick(SEPARATORS));
    }
    text
}

#[test]
fn rewrite_text_matches_the_reference_scan_on_random_documents() {
    let map = map();
    let mut touched = 0;
    run_cases("rename-refs-model", 3000, |_, rng| {
        let text = document(rng, &map);
        let (out, n, orphans) = rewrite_text(&text, &map);
        let (want, want_n, want_orphans) = expected_rewrite(&text, &map);
        assert_eq!((&out, n), (&want, want_n), "{text:?}");
        assert_eq!(
            orphans,
            want_orphans.into_iter().collect::<Vec<_>>(),
            "{text:?}"
        );
        assert_eq!(
            rewrite_text(&text, &map),
            (out, n, orphans),
            "deterministic"
        );
        touched += n;
    });
    assert!(touched > 2000, "{touched}");
}

#[test]
fn rewriting_never_panics_on_garbage_and_leaves_plain_text_alone() {
    let map = map();
    run_cases("rename-refs-garbage", 3000, |_, rng| {
        let text = rng.garbage(120);
        let (out, n, orphans) = rewrite_text(&text, &map);
        if !text.contains("mcp__mcpm_") {
            assert_eq!((out.as_str(), n, orphans.len()), (text.as_str(), 0, 0));
        }
    });
}

#[test]
fn rewriting_is_idempotent_and_removes_every_known_reference() {
    let map = map();
    run_cases("rename-refs-idempotent", 2000, |_, rng| {
        let mut text = String::new();
        for _ in 0..rng.range(1, 10) {
            text.push_str(rng.pick(&map.map.keys().collect::<Vec<_>>()));
            text.push_str(rng.pick(SEPARATORS));
            text.push_str(&rng.garbage(6).replace("mcp__", ""));
        }
        let (once, n, orphans) = rewrite_text(&text, &map);
        assert!(n >= 1 && orphans.is_empty(), "{text:?}");
        assert!(!once.contains("mcp__mcpm_"), "{once:?}");
        assert_eq!(rewrite_text(&once, &map), (once.clone(), 0, Vec::new()));
    });
}

#[test]
fn adjacent_punctuation_and_overlapping_prefixes() {
    let map = map();
    let a = old_tool_name("alpha", "search");
    let ax = old_tool_name("alpha-extra", "search");
    let au = old_tool_name("alpha_extra", "search");
    let axt = old_tool_name("alpha-extra-two", "search");
    let text = format!("{a}, {ax}. ({au}) \"{axt}\" `{a}`\n{ax}-\n{a}_\n{ax}--x");
    let (out, n, orphans) = rewrite_text(&text, &map);
    assert_eq!(n, 8);
    assert!(orphans.is_empty());
    assert!(out.contains("mcp__toolport__alpha__search, mcp__toolport__alphax__search."));
    assert!(out.contains("(mcp__toolport__alpha_u__search)"));
    assert!(out.contains("\"mcp__toolport__alphax2__search\""));
    assert!(out.contains("mcp__toolport__alphax__search-\n"));
    assert!(out.contains("mcp__toolport__alpha__search_\n"));
    assert!(out.ends_with("mcp__toolport__alphax__search--x"));
}

#[test]
fn degenerate_references() {
    let map = map();
    for text in [
        "",
        "mcp__mcpm_",
        "mcp__mcpm_ ",
        "mcp__mcpm__",
        "mcp__mcpm_-",
        "mcp__mcpm_mcp__mcpm_",
        "mcp__mcpm_alpha",
        "mcp__mcpm_alpha__",
        "mcp__mcpm_alpha__search__",
        "MCP__MCPM_ALPHA__SEARCH",
        "mcp__mcpm_日本",
        "xmcp__mcpm_alpha__searchx",
    ] {
        let (out, n, _) = rewrite_text(text, &map);
        let (want, want_n, _) = expected_rewrite(text, &map);
        assert_eq!((out, n), (want, want_n), "{text:?}");
    }
    let empty = NameMap {
        map: BTreeMap::new(),
        servers: BTreeMap::new(),
    };
    let (out, n, orphans) = rewrite_text("mcp__mcpm_a__b", &empty);
    assert_eq!((out.as_str(), n, orphans.len()), ("mcp__mcpm_a__b", 0, 1));
}

#[test]
fn huge_inputs_stay_linear_and_correct() {
    let map = map();
    let line = format!(
        "see {} and {}.\n",
        old_tool_name("alpha", "search"),
        old_tool_name("beta", "list-items")
    );
    let text = line.repeat(15_000);
    let (out, n, orphans) = rewrite_text(&text, &map);
    assert_eq!(n, 30_000);
    assert!(orphans.is_empty());
    assert!(!out.contains("mcp__mcpm_"));
    assert_eq!(out.lines().count(), 15_000);

    let long_dashes = format!("mcp__mcpm_{}", "a-".repeat(150_000));
    let (out, n, orphans) = rewrite_text(&long_dashes, &map);
    assert_eq!((out.len(), n, orphans.len()), (long_dashes.len(), 0, 1));

    let nested = "mcp__mcpm_".repeat(50_000);
    assert_eq!(rewrite_text(&nested, &map).1, 0);
}

fn populate(root: &std::path::Path, rng: &mut Rng, map: &NameMap) -> Vec<(PathBuf, String)> {
    let mut written = Vec::new();
    let dirs = [
        "",
        "rules",
        "skills/demo",
        ".git",
        "node_modules/pkg",
        "target/debug",
        "docs/deep/er",
    ];
    let names = [
        "a.md",
        "B.MD",
        "c.mdc",
        "d.markdown",
        "e.txt",
        "f.yaml",
        "g.yml",
        "h.toml",
        "i.json",
        "j.rs",
        "k.bin",
        "noext",
        "l.md.bak",
    ];
    for _ in 0..rng.range(1, 10) {
        let path = root.join(rng.pick(&dirs)).join(rng.pick(&names));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = document(rng, map);
        fs::write(&path, &text).unwrap();
        written.retain(|(p, _): &(PathBuf, String)| *p != path);
        written.push((path, text));
    }
    if rng.chance(30) {
        let path = root.join("binary.md");
        fs::write(&path, [b'm', 0xff, 0xfe, 0x00]).unwrap();
    }
    written
}

fn scanned(path: &std::path::Path) -> bool {
    let skipped_dir = path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some(".git" | "node_modules" | "target")
        )
    });
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    !skipped_dir
        && ext.is_some_and(|e| {
            [
                "md", "mdc", "markdown", "txt", "yaml", "yml", "toml", "json",
            ]
            .contains(&e.as_str())
        })
}

#[test]
fn rename_refs_over_random_trees_matches_per_file_rewrites_and_converges() {
    let map = map();
    let tmp = ScratchDir::new("rename-tree");
    run_cases("rename-refs-tree", 60, |_, rng| {
        tmp.reset();
        let files = populate(tmp.path(), rng, &map);
        let roots = vec![tmp.path().to_path_buf()];

        let dry = rename_refs(&roots, &map, true).unwrap();
        for (path, text) in &files {
            assert_eq!(
                &fs::read_to_string(path).unwrap(),
                text,
                "dry run wrote {path:?}"
            );
        }
        let mut expected_replaced = 0;
        let mut expected_changed = 0;
        for (path, text) in &files {
            if scanned(path.strip_prefix(tmp.path()).unwrap()) {
                let (out, n, _) = rewrite_text(text, &map);
                expected_replaced += n;
                expected_changed += usize::from(n > 0 && out != *text);
            }
        }
        assert_eq!(dry.replaced, expected_replaced);
        assert_eq!(dry.files.len(), expected_changed);
        let mut found = Vec::new();
        collect(tmp.path(), &mut found).unwrap();
        assert!(dry.scanned <= found.len());

        let applied = rename_refs(&roots, &map, false).unwrap();
        assert_eq!(
            (applied.replaced, applied.files.len()),
            (dry.replaced, dry.files.len())
        );
        for (path, text) in &files {
            let want = if scanned(path.strip_prefix(tmp.path()).unwrap()) {
                rewrite_text(text, &map).0
            } else {
                text.clone()
            };
            assert_eq!(fs::read_to_string(path).unwrap(), want, "{path:?}");
        }
        let again = rename_refs(&roots, &map, false).unwrap();
        assert_eq!((again.replaced, again.changed()), (0, false));
        assert_eq!(again.orphans, applied.orphans);
    });
}

#[test]
fn rename_refs_accepts_files_and_rejects_missing_roots() {
    let map = map();
    let tmp = ScratchDir::new("rename-roots");
    let file = tmp.path().join("notes.unknown-extension");
    fs::write(&file, old_tool_name("alpha", "search")).unwrap();
    let report = rename_refs(&[file.clone()], &map, false).unwrap();
    assert_eq!((report.scanned, report.replaced), (1, 1));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "mcp__toolport__alpha__search"
    );
    assert!(rename_refs(&[tmp.path().join("missing")], &map, true).is_err());
    let none = rename_refs(&[], &map, true).unwrap();
    assert_eq!((none.scanned, none.changed()), (0, false));
    #[cfg(unix)]
    {
        let outside = ScratchDir::new("rename-outside");
        fs::write(
            outside.path().join("x.md"),
            old_tool_name("alpha", "search"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();
        let report = rename_refs(&[tmp.path().to_path_buf()], &map, false).unwrap();
        assert_eq!(report.scanned, 0, "symlinks are never followed");
        assert!(fs::read_to_string(outside.path().join("x.md"))
            .unwrap()
            .contains("mcp__mcpm_"));
    }
}
