//! What an export must not leave or hold in the wrong place: scratch files, whole-file reads of
//! sessions, secret-looking Claude files.

use std::path::PathBuf;

use super::export;
use super::restore::{self, RestoreOptions};
use super::scratch;
use super::tests::{options, World};

const DEAD_PID: u32 = 2_000_000_000;

#[cfg(feature = "cipher")]
mod sessions {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::super::crypto::{self, Cost};
    use super::super::mem::slug;
    use super::super::tests::PASS;
    use super::super::PartId;
    use super::*;
    use crate::testutil::alloc::largest_allocation;

    const CHEAP: Cost = Cost {
        memory_kib: 64,
        iterations: 1,
        lanes: 1,
    };
    const BIG_SESSION: u64 = 16 << 20;
    /// A streamed run works in pieces of a few hundred KiB; a read of the session would be 16 MiB.
    const BOUND: usize = 4 << 20;

    fn big_session(world: &World, repo: &Path) {
        let dir = world.home.join(".claude/projects").join(slug(repo));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::File::create(dir.join("big.jsonl"))
            .unwrap()
            .set_len(BIG_SESSION)
            .unwrap();
    }

    fn sessions_options(world: &World, out: &Path) -> export::Options {
        let mut opts = options(world);
        opts.parts.insert(PartId::History);
        opts.out = Some(out.to_path_buf());
        opts
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .map(|read| {
                read.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    fn scratch_dir(world: &World) -> PathBuf {
        world.ctx().state_dir().join("scratch")
    }

    /// Every name that shows up in `dir` while `work` runs.
    fn names_seen_during<T>(dir: &Path, work: impl FnOnce() -> T) -> (T, BTreeSet<String>) {
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(BTreeSet::new()));
        let watcher = {
            let (stop, seen, dir) = (stop.clone(), seen.clone(), dir.to_path_buf());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    seen.lock().unwrap().extend(names_in(&dir));
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            })
        };
        let result = work();
        stop.store(true, Ordering::Relaxed);
        watcher.join().unwrap();
        let seen = seen.lock().unwrap().clone();
        (result, seen)
    }

    fn world_with_big_session() -> World {
        crypto::TEST_COST.with(|cost| cost.set(Some(CHEAP)));
        let world = World::new();
        let repo = world.repo("web");
        world.session_of(&repo, "abc", "a transcript\n");
        big_session(&world, &repo);
        world
    }

    #[test]
    fn test01_a_large_session_is_streamed_and_only_the_file_appears_in_the_destination() {
        let world = world_with_big_session();
        let out = world.out_dir();
        let opts = sessions_options(&world, &out.join("setup.zip"));
        let (run, seen) = names_seen_during(&out, || {
            largest_allocation(|| export::run(&world.ctx(), &opts, Some(PASS)))
        });
        let (data, largest) = run;
        let data = data.unwrap();
        assert_eq!(data["verified"], true);
        assert!(
            largest < BOUND,
            "the export held {largest} bytes in one piece"
        );
        let allowed: BTreeSet<String> = ["setup.zip", "setup.zip.part"]
            .map(String::from)
            .into_iter()
            .collect();
        assert!(seen.is_subset(&allowed), "next to the file: {seen:?}");
        assert_eq!(names_in(&out), ["setup.zip"]);
        assert_eq!(names_in(&scratch_dir(&world)), Vec::<String>::new());
    }

    #[test]
    fn test02_a_restore_plan_hashes_a_large_file_in_pieces() {
        let from = world_with_big_session();
        let out = from.out_dir();
        let opts = sessions_options(&from, &out.join("setup.zip"));
        export::run(&from.ctx(), &opts, Some(PASS)).unwrap();
        let to = World::new();
        let restore_opts = RestoreOptions {
            file: out.join("setup.zip"),
            ..Default::default()
        };
        let (data, largest) = largest_allocation(|| restore::run(&to.ctx(), &restore_opts));
        assert_eq!(data.unwrap()["dryRun"], true);
        assert!(
            largest < BOUND,
            "the restore held {largest} bytes in one piece"
        );
    }

    #[test]
    fn test03_a_failed_export_leaves_nothing_in_the_destination_or_the_scratch_folder() {
        let world = world_with_big_session();
        let out = world.out_dir();
        // The partial file cannot be created, after the plaintext sessions zip was built.
        std::fs::create_dir(out.join("setup.zip.part")).unwrap();
        let opts = sessions_options(&world, &out.join("setup.zip"));
        let (error, seen) = names_seen_during(&out, || {
            export::run(&world.ctx(), &opts, Some(PASS)).unwrap_err()
        });
        assert!(
            error.message.contains("setup.zip.part"),
            "{}",
            error.message
        );
        assert!(
            seen.iter().all(|n| n == "setup.zip.part"),
            "next to the file: {seen:?}"
        );
        assert_eq!(names_in(&out), ["setup.zip.part"]);
        assert_eq!(names_in(&scratch_dir(&world)), Vec::<String>::new());
    }
}

#[cfg(unix)]
#[test]
fn test04_the_scratch_folder_is_in_the_data_folder_and_private() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new();
    let ctx = world.ctx();
    let folder = scratch::Scratch::new(&ctx).unwrap();
    assert!(folder.path().starts_with(ctx.state_dir()));
    let mode = std::fs::metadata(folder.path())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    let path = folder.path().to_path_buf();
    drop(folder);
    assert!(!path.exists());
}

/// A folder of a run that died, one of this process (a run still going) and one that is not ours.
fn leftovers(world: &World) -> ([PathBuf; 2], PathBuf, PathBuf) {
    let state = world.ctx().state_dir();
    let stale = [
        state.join("scratch").join(format!("export-{DEAD_PID}-0")),
        state.join(format!(".toolport-setup-{DEAD_PID}-1")),
    ];
    let live = state
        .join("scratch")
        .join(format!("export-{}-999999", std::process::id()));
    let unrelated = state.join("scratch").join("notes");
    for dir in stale.iter().chain([&live, &unrelated]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("history.inner.zip"), b"plaintext").unwrap();
    }
    (stale, live, unrelated)
}

#[test]
fn test05_an_export_sweeps_the_scratch_of_runs_that_are_gone() {
    let world = World::new();
    world.repo("web");
    let (stale, live, unrelated) = leftovers(&world);
    let mut opts = options(&world);
    opts.dry_run = true;
    export::run(&world.ctx(), &opts, None).unwrap();
    assert!(stale.iter().all(|dir| !dir.exists()), "{stale:?}");
    assert!(live.exists() && unrelated.exists());
}

#[test]
fn test06_a_restore_sweeps_the_scratch_of_runs_that_are_gone() {
    let from = World::new();
    from.repo("web");
    let file = export::run(&from.ctx(), &options(&from), None).unwrap()["file"]
        .as_str()
        .map(PathBuf::from)
        .unwrap();
    let to = World::new();
    let (stale, live, unrelated) = leftovers(&to);
    let opts = RestoreOptions {
        file,
        ..Default::default()
    };
    restore::run(&to.ctx(), &opts).unwrap();
    assert!(stale.iter().all(|dir| !dir.exists()), "{stale:?}");
    assert!(live.exists() && unrelated.exists());
}

mod held_back {
    use serde_json::{json, Value};

    use super::super::tests::token;
    use super::super::zipio::ZipFile;
    use super::*;

    fn exported(world: &World) -> (Value, PathBuf) {
        let data = export::run(&world.ctx(), &options(world), None).unwrap();
        let file = PathBuf::from(data["file"].as_str().unwrap());
        (data, file)
    }

    fn entry_names(file: &std::path::Path) -> Vec<String> {
        ZipFile::open(file)
            .unwrap()
            .names()
            .map(String::from)
            .collect()
    }

    fn part<'a>(data: &'a Value, id: &str) -> &'a Value {
        data["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .unwrap()
    }

    fn holds(file: &std::path::Path, needle: &str) -> bool {
        std::fs::read(file)
            .unwrap()
            .windows(needle.len())
            .any(|w| w == needle.as_bytes())
    }

    #[test]
    fn test07_claude_user_files_that_look_like_they_hold_a_token_stay_out_and_are_named() {
        let world = World::new();
        world.claude("CLAUDE.md", &format!("deploy with {}\n", token()));
        world.claude("rules/style.md", "short answers\n");
        world.claude("rules/nested/leak.md", "password = hunter2hunter2hunter2\n");
        world.claude("agents/mine.md", "an agent\n");
        world.claude("commands/deploy.md", &format!("curl -H 'x: {}'\n", token()));
        world.claude("keybindings.json", &json!({"key": token()}).to_string());
        let (data, file) = exported(&world);

        let all = entry_names(&file);
        for kept in ["claude-user/rules/style.md", "claude-user/agents/mine.md"] {
            assert!(all.contains(&kept.to_string()), "{kept} in {all:?}");
        }
        for held in [
            "claude-user/CLAUDE.md",
            "claude-user/rules/nested/leak.md",
            "claude-user/commands/deploy.md",
            "claude-user/keybindings.json",
        ] {
            assert!(!all.contains(&held.to_string()), "{held} in {all:?}");
        }
        assert!(!holds(&file, &token()));
        assert!(!holds(&file, "hunter2hunter2hunter2"));

        let claude = part(&data, "claudeUser");
        assert_eq!(claude["count"], 2);
        assert_eq!(
            claude["flagged"],
            json!([
                "~/.claude/CLAUDE.md",
                "~/.claude/rules/nested/leak.md",
                "~/.claude/commands/deploy.md",
                "~/.claude/keybindings.json"
            ])
        );
        assert!(claude["notes"][0].as_str().unwrap().contains("4 files"));
    }

    #[test]
    fn test08_memory_instruction_files_that_look_like_they_hold_a_token_stay_out_and_are_named() {
        let world = World::new();
        let repo = world.repo("web");
        world.put(&repo.join("CLAUDE.local.md"), "local notes\n");
        world.put(&repo.join("AGENTS.md"), &format!("token {}\n", token()));
        world.put(
            &repo.join(".claude/rules/deploy.md"),
            &format!("export KEY={}\n", token()),
        );
        world.put(&repo.join(".claude/rules/style.md"), "short answers\n");
        let (data, file) = exported(&world);

        let index: Value = serde_json::from_slice(
            &ZipFile::open(&file)
                .unwrap()
                .read("memory/index.json")
                .unwrap(),
        )
        .unwrap();
        let repo_entry = &index["repos"][0];
        assert_eq!(
            repo_entry["instructions"],
            json!(["CLAUDE.local.md", ".claude/rules/style.md"])
        );
        assert_eq!(
            repo_entry["instructionsHeld"],
            json!(["AGENTS.md", ".claude/rules/deploy.md"])
        );
        let all = entry_names(&file);
        assert!(!all.iter().any(|n| n.ends_with("AGENTS.md")));
        assert!(!all.iter().any(|n| n.ends_with("deploy.md")));
        assert!(!holds(&file, &token()));

        let memory = part(&data, "memory");
        assert_eq!(
            memory["flagged"],
            json!(["web/AGENTS.md", "web/.claude/rules/deploy.md"])
        );
        assert!(memory["notes"][0]
            .as_str()
            .unwrap()
            .contains("2 instruction files"));
    }

    #[test]
    fn test09_the_parts_listing_names_the_held_back_files_too() {
        let world = World::new();
        world.claude("rules/leak.md", &format!("{}\n", token()));
        let data = export::list(&world.ctx(), &[world.home.join("code")]).unwrap();
        assert_eq!(
            part(&data, "claudeUser")["flagged"],
            json!(["~/.claude/rules/leak.md"])
        );
    }

    #[test]
    fn test10_a_held_back_file_is_named_with_what_the_scan_found_and_never_the_text() {
        let world = World::new();
        let repo = world.repo("web");
        world.claude("CLAUDE.md", &format!("deploy with {}\n", token()));
        world.claude("rules/pw.md", "password = hunter2hunter2hunter2\n");
        world.put(&repo.join("AGENTS.md"), &format!("token {}\n", token()));
        let (data, _) = exported(&world);
        assert_eq!(data["heldBackSealed"], false);
        assert_eq!(
            part(&data, "claudeUser")["heldBack"],
            json!([
                {"file": "~/.claude/CLAUDE.md", "kind": "claude-user", "reason": "a GitHub token"},
                {"file": "~/.claude/rules/pw.md", "kind": "claude-user",
                 "reason": "a password, token or key assigned a value"},
            ])
        );
        assert_eq!(
            part(&data, "memory")["heldBack"],
            json!([{"file": "web/AGENTS.md", "kind": "instruction", "reason": "a GitHub token"}])
        );
        assert!(!data.to_string().contains(&token()));
        assert!(!data.to_string().contains("hunter2"));
    }

    #[cfg(feature = "cipher")]
    #[test]
    fn test13_sessions_ticked_with_none_to_export_leave_the_part_out_and_say_so() {
        use super::super::crypto::{self, Cost};
        use super::super::PartId;
        crypto::TEST_COST.with(|cost| {
            cost.set(Some(Cost {
                memory_kib: 64,
                iterations: 1,
                lanes: 1,
            }))
        });
        let world = World::new();
        world.repo("web");
        let mut opts = options(&world);
        opts.parts.insert(PartId::History);
        let data = export::run(&world.ctx(), &opts, Some(super::super::tests::PASS)).unwrap();
        let file = PathBuf::from(data["file"].as_str().unwrap());
        assert!(!entry_names(&file).contains(&"history.enc".to_string()));
        assert!(data["parts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["id"] != "history"));
        assert_eq!(
            data["skipped"],
            json!(["sessions: none found for the chosen repositories"])
        );
    }

    #[cfg(feature = "cipher")]
    mod sealed {
        use super::super::super::crypto::{self, Cost};
        use super::super::super::tests::PASS;
        use super::super::super::{unhex, PartId};
        use super::*;

        const CHEAP: Cost = Cost {
            memory_kib: 64,
            iterations: 1,
            lanes: 1,
        };

        fn export_with_secrets(world: &World) -> (Value, PathBuf) {
            crypto::TEST_COST.with(|cost| cost.set(Some(CHEAP)));
            let mut opts = options(world);
            opts.parts.insert(PartId::Secrets);
            let data = export::run(&world.ctx(), &opts, Some(PASS)).unwrap();
            let file = PathBuf::from(data["file"].as_str().unwrap());
            (data, file)
        }

        fn unsealed(file: &std::path::Path) -> Value {
            let sealed = ZipFile::open(file).unwrap().read("secrets.enc").unwrap();
            serde_json::from_slice(&crypto::open_bytes(PASS, &sealed).unwrap()).unwrap()
        }

        fn bytes_of(row: &Value) -> String {
            String::from_utf8(unhex(row["hex"].as_str().unwrap()).unwrap()).unwrap()
        }

        #[test]
        fn test11_secrets_on_carries_the_held_back_files_in_the_sealed_part_only() {
            let world = World::new();
            let repo = world.repo("web");
            let global = format!("deploy with {}\n", token());
            let local = format!("token {}\n", token());
            world.claude("CLAUDE.md", &global);
            world.claude("rules/style.md", "short answers\n");
            world.put(&repo.join("AGENTS.md"), &local);
            world.put(&repo.join("CLAUDE.local.md"), "local notes\n");
            let (data, file) = export_with_secrets(&world);
            assert_eq!(data["heldBackSealed"], true);
            assert!(!holds(&file, &token()));
            let all = entry_names(&file);
            assert!(!all
                .iter()
                .any(|n| n.ends_with("CLAUDE.md") && n.starts_with("claude-user")));
            assert!(!all.iter().any(|n| n.ends_with("AGENTS.md")));
            assert!(all.contains(&"claude-user/rules/style.md".to_string()));

            let doc = unsealed(&file);
            let claude = doc["claudeFiles"].as_array().unwrap();
            assert_eq!(claude.len(), 1);
            assert_eq!(claude[0]["name"], "CLAUDE.md");
            assert_eq!(bytes_of(&claude[0]), global);
            let instructions = doc["instructionFiles"].as_array().unwrap();
            assert_eq!(instructions.len(), 1);
            assert_eq!(instructions[0]["name"], "AGENTS.md");
            assert_eq!(bytes_of(&instructions[0]), local);
            let index: Value = serde_json::from_slice(
                &ZipFile::open(&file)
                    .unwrap()
                    .read("memory/index.json")
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(index["repos"][0]["dir"], instructions[0]["dir"]);
            assert_eq!(
                part(&data, "claudeUser")["notes"][0],
                "1 files in ~/.claude look like they hold a token: they go into the encrypted part"
            );
        }

        #[test]
        fn test12_without_secrets_the_sealed_document_is_not_even_built_for_them() {
            let world = World::new();
            let repo = world.repo("web");
            world.session_of(&repo, "abc", "a transcript\n");
            world.claude("CLAUDE.md", &format!("deploy with {}\n", token()));
            let mut opts = options(&world);
            opts.parts.insert(PartId::History);
            let data = export::run(&world.ctx(), &opts, Some(PASS)).unwrap();
            let file = PathBuf::from(data["file"].as_str().unwrap());
            assert!(!entry_names(&file).contains(&"secrets.enc".to_string()));
            assert!(!holds(&file, &token()));
            assert_eq!(
                part(&data, "claudeUser")["heldBack"][0]["file"],
                "~/.claude/CLAUDE.md"
            );
        }
    }
}
