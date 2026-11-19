use super::fixture::{hostile_tap, put, SECRET};
use super::*;
use std::os::unix::fs::symlink;
use tempfile::TempDir;

fn dirs() -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    let (tap, outside) = (base.join("tap"), base.join("outside"));
    fs::create_dir_all(&tap).unwrap();
    fs::create_dir_all(&outside).unwrap();
    (dir, tap, outside)
}

fn small(files: usize) -> Limits {
    Limits {
        files,
        ..Limits::ITEM
    }
}

#[test]
fn test01_names_with_a_separator_dots_nul_or_a_root_are_refused() {
    for bad in [
        "", ".", "..", "a/b", "a\\b", "/etc", "C:\\x", "a\0b", "../x",
    ] {
        assert!(check_name(bad).is_err(), "{bad:?}");
    }
    for good in ["SKILL.md", "..hidden", "a b", "v1.2", "ü"] {
        assert!(check_name(good).is_ok(), "{good:?}");
    }
}

#[test]
fn test02_a_relative_path_is_refused_when_any_part_is_not_a_plain_name() {
    for bad in ["", "/a", "a//b", "a/../b", "../a", "a/./b", "a/b/", "a\0/b"] {
        assert!(check_rel(bad).is_err(), "{bad:?}");
    }
    assert!(check_rel("skills/odh/SKILL.md").is_ok());
}

#[test]
fn test03_kind_reports_a_link_instead_of_its_target() {
    let (_dir, tap, outside) = dirs();
    put(&outside.join("secret.txt"), SECRET);
    symlink(outside.join("secret.txt"), tap.join("file-link")).unwrap();
    symlink(&outside, tap.join("dir-link")).unwrap();
    symlink(tap.join("gone"), tap.join("dangling")).unwrap();
    for name in ["file-link", "dir-link", "dangling"] {
        assert_eq!(kind(&tap.join(name)), Some(Kind::Link), "{name}");
        assert!(!is_file(&tap.join(name)) && !is_dir(&tap.join(name)));
    }
    assert_eq!(kind(&tap.join("absent")), None);
}

#[test]
fn test04_a_file_is_not_read_through_a_link_or_past_the_cap() {
    let (_dir, tap, outside) = dirs();
    put(&outside.join("secret.txt"), SECRET);
    put(&tap.join("big.txt"), "0123456789");
    symlink(outside.join("secret.txt"), tap.join("leak.txt")).unwrap();
    let why = read_bytes(&tap.join("leak.txt"), 1024).unwrap_err();
    assert!(why.contains("symbolic link"), "{why}");
    assert!(read_text(&tap.join("leak.txt"), 1024).is_none());
    let why = read_bytes(&tap.join("big.txt"), 4).unwrap_err();
    assert!(why.contains("larger than 4 bytes"), "{why}");
    assert_eq!(read_bytes(&tap.join("big.txt"), 10).unwrap().len(), 10);
}

#[test]
fn test05_a_listing_names_links_and_does_not_enter_a_linked_folder() {
    let (_dir, tap, outside) = dirs();
    put(&outside.join("a.md"), SECRET);
    put(&tap.join("real/b.md"), "b");
    symlink(&outside, tap.join("linked")).unwrap();
    let names: Vec<(String, Kind)> = list_dir(&tap)
        .into_iter()
        .map(|e| (e.name, e.kind))
        .collect();
    assert_eq!(
        names,
        [
            ("linked".to_string(), Kind::Link),
            ("real".to_string(), Kind::Dir)
        ]
    );
    assert!(list_dir(&tap.join("linked")).is_empty());
}

#[test]
fn test06_below_refuses_a_path_through_a_link_or_out_of_the_root() {
    let (_dir, tap, outside) = dirs();
    put(&outside.join("hooks.json"), "{}");
    put(&tap.join("scripts/run.sh"), "x");
    symlink(&outside, tap.join("hooks")).unwrap();
    assert_eq!(
        below(&tap, "scripts/run.sh").unwrap(),
        tap.join("scripts/run.sh")
    );
    assert!(below(&tap, "scripts/missing.sh").is_ok());
    let why = below(&tap, "hooks/hooks.json").unwrap_err();
    assert!(why.contains("symbolic link"), "{why}");
    assert!(below(&tap, "../outside/hooks.json").is_err());
    assert!(below(&tap, "/etc/passwd").is_err());
}

#[test]
fn test07_a_walk_names_the_first_link_at_any_depth_and_the_skip_mode_lists_them() {
    let (_dir, tap, outside) = dirs();
    put(&outside.join("secret.txt"), SECRET);
    put(&tap.join("a/b/real.txt"), "ok");
    symlink(outside.join("secret.txt"), tap.join("a/b/leak.txt")).unwrap();
    symlink(&outside, tap.join("a/dir")).unwrap();
    let why = walk(&tap, Links::Reject, &Limits::ITEM, &[]).unwrap_err();
    assert!(
        why.contains("a/b/leak.txt is a symbolic link") || why.contains("a/dir is a symbolic link"),
        "{why}"
    );
    let tree = walk(&tap, Links::Skip, &Limits::ITEM, &[]).unwrap();
    assert_eq!(tree.skipped, ["a/b/leak.txt", "a/dir"]);
    let files: Vec<&str> = tree.files().map(|(rel, _, _)| rel).collect();
    assert_eq!(files, ["a/b/real.txt"]);
}

#[test]
fn test08_a_walk_of_a_linked_root_is_refused() {
    let (_dir, tap, outside) = dirs();
    put(&outside.join("a.md"), SECRET);
    symlink(&outside, tap.join("skill")).unwrap();
    let why = walk(&tap.join("skill"), Links::Skip, &Limits::ITEM, &[]).unwrap_err();
    assert!(why.contains("symbolic link"), "{why}");
}

#[test]
fn test09_a_folder_over_the_file_count_the_byte_total_or_the_depth_is_refused() {
    let (_dir, tap, _outside) = dirs();
    for n in 0..6 {
        put(&tap.join(format!("many/f{n}.txt")), "x");
    }
    let why = walk(&tap.join("many"), Links::Reject, &small(5), &[]).unwrap_err();
    assert!(why.contains("more than 5 files"), "{why}");
    assert!(walk(&tap.join("many"), Links::Reject, &small(6), &[]).is_ok());

    put(&tap.join("heavy/a.bin"), "0123456789");
    put(&tap.join("heavy/b.bin"), "0123456789");
    let bytes = Limits {
        bytes: 15,
        ..Limits::ITEM
    };
    let why = walk(&tap.join("heavy"), Links::Reject, &bytes, &[]).unwrap_err();
    assert!(why.contains("more than 15 bytes"), "{why}");
    let one = Limits {
        file_bytes: 5,
        ..Limits::ITEM
    };
    let why = walk(&tap.join("heavy"), Links::Reject, &one, &[]).unwrap_err();
    assert!(why.contains("larger than 5 bytes"), "{why}");

    put(&tap.join("deep/a/b/c/d.txt"), "x");
    let shallow = Limits {
        depth: 2,
        ..Limits::ITEM
    };
    let why = walk(&tap.join("deep"), Links::Reject, &shallow, &[]).unwrap_err();
    assert!(why.contains("nested deeper than 2"), "{why}");
}

#[test]
fn test10_a_huge_file_count_is_refused_before_anything_is_copied() {
    let (_dir, tap, outside) = dirs();
    for n in 0..=MAX_ITEM_FILES {
        put(&tap.join(format!("skill/data/f{n}.txt")), "x");
    }
    let to = outside.join("copy");
    let why = copy_tree(&tap.join("skill"), &to, Links::Reject, &Limits::ITEM, &[]).unwrap_err();
    assert!(why.contains("too many to copy"), "{why}");
    assert!(!to.exists());
}

#[test]
fn test11_a_copy_of_a_linked_source_is_refused_at_the_top_and_inside() {
    let (_dir, tap, outside) = dirs();
    put(&outside.join("secret.txt"), SECRET);
    put(&tap.join("inside/real.md"), "ok");
    symlink(outside.join("secret.txt"), tap.join("top.md")).unwrap();
    symlink(&outside, tap.join("topdir")).unwrap();
    symlink(outside.join("secret.txt"), tap.join("inside/leak.md")).unwrap();
    let to = outside.join("copy");
    for from in ["top.md", "topdir", "inside"] {
        let why = copy_tree(
            &tap.join(from),
            &to.join(from),
            Links::Reject,
            &Limits::ITEM,
            &[],
        )
        .unwrap_err();
        assert!(why.contains("symbolic link"), "{from}: {why}");
    }
    assert!(!to.join("top.md").exists() && !to.join("inside").exists());
}

#[test]
fn test12_a_copy_that_skips_links_keeps_the_regular_files_and_reports_the_rest() {
    let (_dir, tap, outside) = dirs();
    put(&outside.join("secret.txt"), SECRET);
    put(&tap.join("s/SKILL.md"), "skill");
    put(&tap.join("s/.git/config"), "git");
    put(&tap.join("s/scripts/run.sh"), "run");
    symlink(outside.join("secret.txt"), tap.join("s/leak.txt")).unwrap();
    let to = outside.join("copy");
    let copied = copy_tree(&tap.join("s"), &to, Links::Skip, &Limits::ITEM, &[".git"]).unwrap();
    assert_eq!(copied.files, 2);
    assert_eq!(copied.skipped, ["leak.txt"]);
    assert_eq!(
        fs::read_to_string(to.join("scripts/run.sh")).unwrap(),
        "run"
    );
    assert!(!to.join("leak.txt").exists() && !to.join(".git").exists());
}

#[test]
fn test13_a_copy_keeps_the_mode_of_a_script() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, tap, outside) = dirs();
    put(&tap.join("s/run.sh"), "#!/bin/sh\n");
    fs::set_permissions(tap.join("s/run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    let to = outside.join("copy");
    copy_tree(&tap.join("s"), &to, Links::Reject, &Limits::ITEM, &[]).unwrap();
    let mode = fs::metadata(to.join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
}

#[test]
fn test14_a_copy_never_writes_through_a_link_that_sits_in_the_destination() {
    let (_dir, tap, outside) = dirs();
    put(&tap.join("s/sub/file.txt"), "payload");
    put(&tap.join("s/top.txt"), "payload");
    let to = outside.join("copy");
    let elsewhere = outside.join("elsewhere");
    fs::create_dir_all(&to).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    symlink(&elsewhere, to.join("sub")).unwrap();
    symlink(elsewhere.join("planted"), to.join("top.txt")).unwrap();
    let why = copy_tree(&tap.join("s"), &to, Links::Reject, &Limits::ITEM, &[]).unwrap_err();
    assert!(why.contains("symbolic link"), "{why}");
    assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
}

#[test]
fn test15_a_write_is_checked_against_the_resolved_parent() {
    let (_dir, tap, outside) = dirs();
    fs::create_dir_all(tap.join("real")).unwrap();
    symlink(&outside, tap.join("out")).unwrap();
    assert!(check_write(&tap, &tap.join("real/new/file.txt")).is_ok());
    assert!(check_write(&tap, &tap.join("out/file.txt")).is_err());
    assert!(check_write(&tap, &tap.join("out")).is_err());
    assert!(check_write(&tap, &tap.join("real/../../outside/x")).is_err());
}

#[test]
fn test16_the_hostile_tap_fixture_leaks_nothing_through_a_copy_of_any_of_its_parts() {
    let (_dir, tap, outside) = dirs();
    hostile_tap(&tap, &outside);
    let to = outside.join("copy");
    for part in [
        "skills/linked-dir",
        "skills/linked-file",
        "skills/nested",
        "commands/top.md",
        "agents",
        "hooks/hooks.json",
    ] {
        let result = copy_tree(
            &tap.join(part),
            &to.join(part),
            Links::Reject,
            &Limits::ITEM,
            &[],
        );
        assert!(result.is_err(), "{part} was copied");
    }
    let tree = walk(&to, Links::Skip, &Limits::ITEM, &[]);
    for (_, path, _) in tree.iter().flat_map(|t| t.files()) {
        assert!(!fs::read_to_string(path).unwrap().contains(SECRET));
    }
}
