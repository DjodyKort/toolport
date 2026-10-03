//! Seeded randomized tests for the lockfile codec and the skills sync: the lock round-trips,
//! garbage never panics, a tampered lock cannot delete outside the output root, and syncing the
//! same sources again converges to the same files and the same lock.

use super::clock::{FixedClock, Instant};
use super::lock::{load_lockfile, save_lockfile, LockEntry, LockFile, LOCKFILE_NAME};
use super::parser::discover_skills;
use super::sync::{collect_stale_files, sync_skills, SyncOptions};
use super::transpiler::TranspilerRegistry;
use super::transpilers::register_all_with_home;
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn unique_keys(rng: &mut Rng, max: usize) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for _ in 0..rng.below(max + 1) {
        let key = rng.garbage(8);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn string_list(rng: &mut Rng) -> Vec<String> {
    (0..rng.below(4)).map(|_| rng.garbage(10)).collect()
}

fn list_map(rng: &mut Rng) -> Vec<(String, Vec<String>)> {
    unique_keys(rng, 3)
        .into_iter()
        .map(|k| (k, string_list(rng)))
        .collect()
}

fn entry(rng: &mut Rng) -> LockEntry {
    let mut e = LockEntry::new(rng.chance(50).then(|| rng.garbage(6)), rng.garbage(12));
    e.source = rng.garbage(6);
    e.clients_synced = string_list(rng);
    e.warnings = string_list(rng);
    e.output_files = list_map(rng);
    e.hooks_installed = list_map(rng);
    e
}

fn entries(rng: &mut Rng) -> Vec<(String, LockEntry)> {
    unique_keys(rng, 3)
        .into_iter()
        .map(|k| (k, entry(rng)))
        .collect()
}

fn lock(rng: &mut Rng) -> LockFile {
    let mut l = LockFile::new(rng.garbage(20));
    l.version = rng.below(5) as i64 - 1;
    l.scope = rng.garbage(8);
    l.output_root = rng.garbage(20);
    l.skills = entries(rng);
    l.rules = entries(rng);
    l.agents = entries(rng);
    l.styles = entries(rng);
    l.active_styles = unique_keys(rng, 3)
        .into_iter()
        .map(|k| (k, rng.garbage(8)))
        .collect();
    l
}

#[test]
fn lockfile_serialisation_round_trips() {
    run_cases("lock-roundtrip", 1500, |_, rng| {
        let original = lock(rng);
        let text = original.serialize();
        assert_eq!(LockFile::parse(&text).as_ref(), Some(&original), "{text}");
        assert_eq!(original.serialize(), text, "deterministic");
        assert_eq!(
            LockFile::parse(&text).unwrap().serialize(),
            text,
            "idempotent"
        );
    });
}

#[test]
fn lockfile_parsing_never_panics_on_garbage_or_truncation() {
    run_cases("lock-garbage", 3000, |_, rng| {
        let _ = LockFile::parse(&rng.garbage(60));
        let text = lock(rng).serialize();
        let mut mutated = text.clone();
        let at = rng.below(mutated.len().max(1));
        if let Some((i, c)) = mutated.char_indices().find(|(i, _)| *i >= at) {
            mutated.replace_range(i..i + c.len_utf8(), &rng.garbage(2));
        }
        let _ = LockFile::parse(&mutated);
    });
    run_cases("lock-truncation", 150, |_, rng| {
        let text = lock(rng).serialize();
        for _ in 0..30 {
            let mut cut = rng.below(text.len());
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            assert!(LockFile::parse(&text[..cut]).is_none(), "prefix {cut}");
        }
    });
    for text in [
        "",
        "null",
        "[]",
        "{}",
        "{\"version\": \"x\"}",
        "{\"skills\": []}",
        "{\"skills\": {\"a\": 1}}",
    ] {
        let _ = LockFile::parse(text);
    }
    assert!(LockFile::parse("{}").is_some());
    assert!(LockFile::parse("[]").is_none());
}

#[test]
fn load_lockfile_ignores_missing_and_invalid_files() {
    let tmp = ScratchDir::new("lock-load");
    assert!(load_lockfile(tmp.path()).is_none());
    fs::write(tmp.path().join(LOCKFILE_NAME), [0xff, 0xfe, 0x00]).unwrap();
    assert!(load_lockfile(tmp.path()).is_none());
    fs::write(tmp.path().join(LOCKFILE_NAME), "{not json").unwrap();
    assert!(load_lockfile(tmp.path()).is_none());
    let good = LockFile::new("t".into());
    save_lockfile(tmp.path(), &good).unwrap();
    assert_eq!(load_lockfile(tmp.path()), Some(good));
}

fn clock() -> FixedClock {
    FixedClock(Instant {
        unix_secs: 1_767_225_600,
        micros: 0,
    })
}

fn opts<'a>(root: &Path, clock: &'a FixedClock, dry_run: bool) -> SyncOptions<'a> {
    SyncOptions {
        output_root: root.to_path_buf(),
        lock_dir: root.to_path_buf(),
        global_mode: false,
        dry_run,
        migrate: None,
        client_keys: None,
        clock,
    }
}

fn registry(home: &Path) -> TranspilerRegistry {
    let mut reg = TranspilerRegistry::new();
    register_all_with_home(&mut reg, Some(home.to_path_buf()));
    reg
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, base: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn write_skill(src: &Path, name: &str, rng: &mut Rng) {
    let dir = src.join(*rng.pick(&["skills", "rules"])).join(name);
    fs::create_dir_all(&dir).unwrap();
    let mut map = Mapping::new();
    map.insert("name".into(), Value::String(name.into()));
    let mut description = rng.string("abc XYZ 019.,:!?()é日", 40);
    description.push('d');
    map.insert("description".into(), Value::String(description));
    if rng.chance(40) {
        map.insert("activation".into(), Value::String("always".into()));
    }
    if rng.chance(30) {
        map.insert("globs".into(), Value::String("src/**, **/*.py".into()));
    }
    let yaml = serde_yaml::to_string(&map).unwrap();
    let body = rng.garbage(120).replace("---", "-_-");
    fs::write(dir.join("SKILL.md"), format!("---\n{yaml}---\n{body}\n")).unwrap();
    for _ in 0..rng.below(3) {
        let sub = rng.pick(&["scripts", "reference", "assets", "modules"]);
        let file = format!("{}.{}", rng.slug(8), rng.pick(&["md", "txt", "json", "py"]));
        fs::create_dir_all(dir.join(sub)).unwrap();
        fs::write(dir.join(sub).join(file), rng.bytes(60)).unwrap();
    }
}

#[test]
fn syncing_the_same_sources_converges_and_dry_runs_write_nothing() {
    let src = ScratchDir::new("sync-src");
    let out = ScratchDir::new("sync-out");
    let home = ScratchDir::new("sync-home");
    let reg = registry(home.path());
    let clk = clock();
    run_cases("skills-sync-converges", 80, |_, rng| {
        src.reset();
        out.reset();
        let mut names: Vec<String> = Vec::new();
        for _ in 0..rng.range(1, 4) {
            let name = format!("n-{}", rng.slug(10));
            if !names.contains(&name) {
                write_skill(src.path(), &name, rng);
                names.push(name);
            }
        }
        let skills = discover_skills(src.path());
        assert_eq!(skills.len(), names.len());
        sync_skills(&skills, &reg, &opts(out.path(), &clk, true)).unwrap();
        assert!(snapshot(out.path()).is_empty(), "dry run must not write");

        let sync = || {
            let skills = discover_skills(src.path());
            let result = sync_skills(&skills, &reg, &opts(out.path(), &clk, false)).unwrap();
            save_lockfile(out.path(), &result.lockfile).unwrap();
            (result.lockfile, snapshot(out.path()))
        };
        let (_, first_files) = sync();
        let (second_lock, second_files) = sync();
        let (third_lock, third_files) = sync();
        assert_eq!(second_lock, third_lock);
        assert_eq!(second_files, third_files, "stable after the second sync");
        let missing: Vec<_> = first_files
            .keys()
            .filter(|k| !second_files.contains_key(*k))
            .collect();
        assert!(missing.is_empty(), "{missing:?}");

        let gone = rng.pick(&names).clone();
        for sub in ["skills", "rules"] {
            let _ = fs::remove_dir_all(src.path().join(sub).join(&gone));
        }
        let (lock_after, files_after) = sync();
        assert!(lock_after
            .skills
            .iter()
            .chain(&lock_after.rules)
            .all(|(n, _)| *n != gone));
        let dir_marker = format!("/{gone}/");
        let dot_marker = format!("/{gone}.");
        for path in files_after.keys() {
            let in_aggregate = path == "AGENTS.md" || path == ".rules";
            assert!(
                in_aggregate
                    || !(format!("/{path}").contains(&dir_marker)
                        || format!("/{path}").contains(&dot_marker)),
                "stale {path}"
            );
        }
        assert_eq!(sync().1, files_after, "stable after cleanup");
    });
}

#[test]
fn a_tampered_lock_cannot_delete_outside_the_output_root() {
    let out = ScratchDir::new("tamper-out");
    let elsewhere = ScratchDir::new("tamper-elsewhere");
    let home = ScratchDir::new("tamper-home");
    let reg = registry(home.path());
    let clk = clock();
    let victim = elsewhere.path().join("victim.txt");
    let sibling = out.path().parent().unwrap().join(format!(
        "{}-sibling.txt",
        out.path().file_name().unwrap().to_string_lossy()
    ));
    fs::write(&victim, "keep").unwrap();
    fs::write(&sibling, "keep").unwrap();
    let inside = out.path().join(".claude/skills/ghost/SKILL.md");
    fs::create_dir_all(inside.parent().unwrap()).unwrap();
    fs::write(&inside, "managed").unwrap();

    let mut ghost = LockEntry::new(None, "sha256:0".into());
    ghost.extend_outputs(
        "claude-code",
        vec![
            victim.to_string_lossy().into_owned(),
            format!("../{}", sibling.file_name().unwrap().to_string_lossy()),
            ".claude/skills/ghost/SKILL.md".into(),
        ],
    );
    let mut tampered = LockFile::new("t".into());
    tampered.skills.push(("ghost".into(), ghost.clone()));
    save_lockfile(out.path(), &tampered).unwrap();

    let stale = collect_stale_files(&tampered, &LockFile::new("t".into()), out.path());
    assert_eq!(stale, vec![inside.clone()]);

    let result = sync_skills(&[], &reg, &opts(out.path(), &clk, false)).unwrap();
    assert_eq!(result.cleaned, vec![inside.clone()]);
    assert!(!inside.exists());
    assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
    assert_eq!(fs::read_to_string(&sibling).unwrap(), "keep");
    let _ = fs::remove_file(&sibling);
}

#[test]
fn a_tampered_lock_cannot_clean_outside_the_output_root() {
    use super::agents::all_agent_transpilers;
    use super::ops::{clean_agents, clean_skills, clean_styles};
    let tmp = ScratchDir::new("tamper-clean");
    let root = tmp.path().join("root");
    let home = ScratchDir::new("tamper-clean-home");
    let reg = registry(home.path());
    let victims: Vec<PathBuf> = [
        "victim.md",
        "victim.toml",
        "victim.agent.md",
        "victim/RULE.md",
    ]
    .iter()
    .map(|rel| tmp.path().join(rel))
    .collect();
    for victim in &victims {
        fs::create_dir_all(victim.parent().unwrap()).unwrap();
        fs::write(victim, "keep").unwrap();
    }
    for dir in [
        ".windsurf/rules",
        ".claude/rules",
        ".claude/agents",
        ".codex/agents",
        ".github/agents",
        ".cursor/rules/x",
        ".claude/output-styles",
    ] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    let mut tampered = LockFile::new("t".into());
    for name in [
        "../../../victim",
        "../../../../victim",
        "x/../../../../victim",
        "/abs/victim",
    ] {
        let e = LockEntry::new(None, "sha256:0".into());
        tampered.skills.push((name.into(), e.clone()));
        tampered.rules.push((name.into(), e.clone()));
        tampered.agents.push((name.into(), e.clone()));
        tampered.styles.push((name.into(), e));
    }
    clean_skills(&root, &root, &reg, None, Some(&tampered), false);
    clean_agents(&root, &all_agent_transpilers(), None, Some(&tampered));
    clean_styles(&root, Some(&mut tampered.clone()));
    let deleted: Vec<&PathBuf> = victims.iter().filter(|v| !v.exists()).collect();
    assert!(deleted.is_empty(), "deleted outside the root: {deleted:?}");
}
