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
