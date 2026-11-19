//! Toolport's own trash and archive for memory and sessions. Both live in Toolport's data folder,
//! never inside `~/.claude/projects`: Claude Code reads what is there as memory and sessions, and
//! a session found twice makes `--resume <id>` report not-found. Everything is moved, so a clear
//! and its undo leave exactly one copy; across volumes the move is a verified copy first.

use crate::context::fsx;
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub const TRASH_DAYS: i64 = 30;
const DAY: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Trash,
    Archive,
}

impl Store {
    pub fn dir_name(self) -> &'static str {
        match self {
            Store::Trash => "trash",
            Store::Archive => "archive",
        }
    }
}

/// Where a clear sends what it removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    Trash,
    Archive,
    Forever,
}

impl Dest {
    pub fn store(self) -> Option<Store> {
        match self {
            Dest::Trash => Some(Store::Trash),
            Dest::Archive => Some(Store::Archive),
            Dest::Forever => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub from: String,
    pub payload: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    /// `memory` or `sessions`.
    pub kind: String,
    pub created: i64,
    pub repo_root: String,
    pub repo_name: String,
    pub items: Vec<Item>,
    /// The memory switch position to put back when the clear also turned memory off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub switch_before: Option<String>,
}

impl Entry {
    pub fn bytes(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }

    pub fn expires(&self, store: Store) -> Option<i64> {
        (store == Store::Trash).then_some(self.created + TRASH_DAYS * DAY)
    }
}

fn base(data_dir: &Path, store: Store) -> PathBuf {
    data_dir.join("plus").join("memory").join(store.dir_name())
}

fn entry_dir(data_dir: &Path, store: Store, id: &str) -> PathBuf {
    base(data_dir, store).join(id)
}

fn stamp(now: i64) -> String {
    fsx::zulu(now)
        .replace(['-', ':'], "")
        .replace('T', "-")
        .replace('Z', "")
}

fn failed(message: impl Into<String>) -> OpError {
    OpError::failed("memory_failed", message)
}

pub fn size_of(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| size_of(&e.path()))
            .sum()
    } else {
        meta.len()
    }
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(fs::read_link(from)?, to)
        }
        #[cfg(not(unix))]
        {
            fs::copy(from, to).map(|_| ())
        }
    } else {
        fs::copy(from, to)?;
        // Session retention reads file ages, so a copy keeps them.
        if let Ok(modified) = meta.modified() {
            let _ = fs::File::options()
                .write(true)
                .open(to)
                .and_then(|file| file.set_modified(modified));
        }
        Ok(())
    }
}

pub fn remove_tree(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// What a file looks like to the check that a copy is whole.
#[derive(Debug, PartialEq, Eq)]
enum Print {
    Dir,
    Link(PathBuf),
    File { size: u64, hash: [u8; 32] },
}

fn hash_file(path: &Path) -> io::Result<(u64, [u8; 32])> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((size, hasher.finalize().into()))
}

fn prints(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Print>) -> io::Result<()> {
    let meta = fs::symlink_metadata(at)?;
    let rel = at.strip_prefix(root).unwrap_or(at).to_path_buf();
    if meta.is_dir() {
        out.insert(rel, Print::Dir);
        for entry in fs::read_dir(at)? {
            prints(root, &entry?.path(), out)?;
        }
    } else if meta.file_type().is_symlink() {
        out.insert(rel, Print::Link(fs::read_link(at)?));
    } else {
        let (size, hash) = hash_file(at)?;
        out.insert(rel, Print::File { size, hash });
    }
    Ok(())
}

fn verify_copy(from: &Path, copy: &Path) -> io::Result<()> {
    let (mut want, mut got) = (BTreeMap::new(), BTreeMap::new());
    prints(from, from, &mut want)?;
    prints(copy, copy, &mut got)?;
    if want == got {
        return Ok(());
    }
    let differs = want
        .iter()
        .find(|(rel, print)| got.get(*rel) != Some(*print))
        .map(|(rel, _)| rel.as_path())
        .or_else(|| {
            got.keys()
                .find(|rel| !want.contains_key(*rel))
                .map(PathBuf::as_path)
        })
        .unwrap_or(Path::new(""));
    Err(io::Error::other(format!(
        "the copy of {} differs from the original at {}",
        from.display(),
        differs.display()
    )))
}

/// The calls a move makes that can fail on a second volume; tests replace them to simulate one.
trait MoveFs {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn copy_tree(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_source(&self, path: &Path) -> io::Result<()>;
}

struct RealFs;

impl MoveFs for RealFs {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn copy_tree(&self, from: &Path, to: &Path) -> io::Result<()> {
        copy_tree(from, to)
    }

    fn remove_source(&self, path: &Path) -> io::Result<()> {
        remove_tree(path)
    }
}

/// How a move ended when it did not fail.
#[derive(Debug)]
enum Moved {
    Whole,
    /// The destination holds the complete, verified copy, but the source is not (fully) gone.
    SourceLeft(io::Error),
}

fn staging_path(to: &Path) -> PathBuf {
    let name = to
        .file_name()
        .map_or_else(|| "item".into(), |n| n.to_string_lossy().into_owned());
    to.with_file_name(format!(".{name}.copying"))
}

/// A rename. Only when the rename itself says the two places are on different volumes is the
/// tree copied: into a hidden sibling, compared with the original by size and hash, renamed into
/// place, and only then is the source removed. Any failure before that leaves the source as it
/// was; after it the copy is never touched again.
fn move_with(fs: &dyn MoveFs, from: &Path, to: &Path) -> io::Result<Moved> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs.rename(from, to) {
        Ok(()) => Ok(Moved::Whole),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => copy_across(fs, from, to),
        Err(e) => Err(e),
    }
}

fn copy_across(fs: &dyn MoveFs, from: &Path, to: &Path) -> io::Result<Moved> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists already", to.display()),
        ));
    }
    let staging = staging_path(to);
    remove_tree(&staging)?;
    let copied = fs
        .copy_tree(from, &staging)
        .and_then(|()| verify_copy(from, &staging))
        .and_then(|()| fs::rename(&staging, to));
    if let Err(e) = copied {
        let _ = remove_tree(&staging);
        return Err(e);
    }
    Ok(match fs.remove_source(from) {
        Ok(()) => Moved::Whole,
        Err(e) => Moved::SourceLeft(e),
    })
}

fn write_manifest(dir: &Path, entry: &Entry) -> Result<(), OpError> {
    let text = serde_json::to_string_pretty(entry).map_err(|e| failed(e.to_string()))?;
    atomic_write(&dir.join("manifest.json"), &text, Mode::Private)
        .map_err(|e| failed(format!("{}: {e}", dir.display())))
}

pub struct Put<'a> {
    pub store: Store,
    pub kind: &'a str,
    pub now: i64,
    pub repo_root: &'a Path,
    pub repo_name: &'a str,
    pub froms: &'a [PathBuf],
    pub switch_before: Option<String>,
}

fn build_entry(id: String, put: &Put, items: Vec<Item>) -> Entry {
    Entry {
        id,
        kind: put.kind.to_string(),
        created: put.now,
        repo_root: fsx::display(put.repo_root),
        repo_name: put.repo_name.to_string(),
        items,
        switch_before: put.switch_before.clone(),
    }
}

/// Moves `froms` into a new entry. A failure puts back what was already moved; an item that
/// cannot be put back stays in the entry, which is then kept and listed. When a copy is whole
/// but its source could not be fully removed, the entry keeps the copy and the call fails
/// saying what is left in place.
pub fn put(data_dir: &Path, put: &Put) -> Result<Entry, OpError> {
    put_with(&RealFs, data_dir, put)
}

fn put_with(fs: &dyn MoveFs, data_dir: &Path, put: &Put) -> Result<Entry, OpError> {
    let mut id = format!("{}-{}", put.kind, stamp(put.now));
    let mut n = 1;
    while entry_dir(data_dir, put.store, &id).exists() {
        n += 1;
        id = format!("{}-{}-{n}", put.kind, stamp(put.now));
    }
    let dir = entry_dir(data_dir, put.store, &id);
    let mut items: Vec<Item> = Vec::new();
    for (index, from) in put.froms.iter().enumerate() {
        let name = from
            .file_name()
            .map_or_else(|| "item".into(), |n| n.to_string_lossy().into_owned());
        let payload = format!("{index}/{name}");
        let bytes = size_of(from);
        let moved = move_with(fs, from, &dir.join("payload").join(&payload));
        let item = Item {
            from: fsx::display(from),
            payload,
            bytes,
        };
        match moved {
            Ok(Moved::Whole) => items.push(item),
            Ok(Moved::SourceLeft(e)) => {
                items.push(item);
                let entry = build_entry(id, put, items);
                write_manifest(&dir, &entry)?;
                return Err(failed(format!(
                    "{} was copied whole into Toolport's {} as {}, but it could not be fully removed from its place: {e}. The copy is kept; what is left at {} is yours to remove",
                    from.display(),
                    put.store.dir_name(),
                    entry.id,
                    from.display()
                )));
            }
            Err(e) => {
                let stuck = put_back(fs, &dir, &items);
                if stuck.is_empty() {
                    let _ = remove_tree(&dir);
                    return Err(failed(format!("could not move {}: {e}", from.display())));
                }
                let kept = stuck.len();
                write_manifest(&dir, &build_entry(id.clone(), put, stuck))?;
                return Err(failed(format!(
                    "could not move {}: {e}; {kept} item(s) already moved could not be put back and stay in the {} as {id}",
                    from.display(),
                    put.store.dir_name()
                )));
            }
        }
    }
    let entry = build_entry(id, put, items);
    write_manifest(&dir, &entry)?;
    Ok(entry)
}

/// Moves items out of an entry back to where they came from, newest first; the ones that
/// would not move are returned.
fn put_back(fs: &dyn MoveFs, dir: &Path, items: &[Item]) -> Vec<Item> {
    items
        .iter()
        .rev()
        .filter(|item| {
            move_with(
                fs,
                &dir.join("payload").join(&item.payload),
                Path::new(&item.from),
            )
            .is_err()
        })
        .cloned()
        .collect()
}

fn read_entry(dir: &Path) -> Option<Entry> {
    serde_json::from_str(&fsx::read_text(&dir.join("manifest.json"), fsx::TEXT_CAP)?).ok()
}

/// The entries of a store, newest first; trash entries past their 30 days are not listed.
pub fn list(data_dir: &Path, store: Store, now: i64) -> Vec<Entry> {
    let mut entries: Vec<Entry> = fsx::list_dir(&base(data_dir, store))
        .into_iter()
        .filter(|e| e.kind == fsx::Kind::Dir)
        .filter_map(|e| read_entry(&e.path))
        .filter(|e| e.expires(store).is_none_or(|at| at > now))
        .collect();
    entries.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    entries
}

pub fn find(
    data_dir: &Path,
    store: Store,
    kind: &str,
    id: Option<&str>,
    repo_root: Option<&Path>,
    now: i64,
) -> Result<Entry, OpError> {
    let all = list(data_dir, store, now);
    let word = match store {
        Store::Trash => "trash",
        Store::Archive => "archive",
    };
    match id {
        Some(id) => {
            let entry = all
                .into_iter()
                .find(|e| e.id == id)
                .ok_or_else(|| OpError::not_found(format!("nothing called {id} in the {word}")))?;
            if entry.kind != kind {
                return Err(OpError::usage(format!(
                    "{id} holds {}, not {kind}",
                    entry.kind
                )));
            }
            Ok(entry)
        }
        None => all
            .into_iter()
            .find(|e| {
                e.kind == kind && repo_root.is_none_or(|root| e.repo_root == fsx::display(root))
            })
            .ok_or_else(|| OpError::not_found(format!("nothing of {kind} to undo in the {word}"))),
    }
}

/// Moves an entry's items back. Nothing moves when any original place is taken.
pub fn restore(data_dir: &Path, store: Store, entry: &Entry) -> Result<Vec<String>, OpError> {
    restore_with(&RealFs, data_dir, store, entry)
}

fn restore_with(
    fs: &dyn MoveFs,
    data_dir: &Path,
    store: Store,
    entry: &Entry,
) -> Result<Vec<String>, OpError> {
    for item in &entry.items {
        if Path::new(&item.from).exists() {
            return Err(OpError::conflict(format!(
                "{} exists again; move it away first, nothing was restored",
                item.from
            )));
        }
    }
    let dir = entry_dir(data_dir, store, &entry.id);
    let mut done: Vec<&Item> = Vec::new();
    for item in &entry.items {
        if let Err(e) = move_with(
            fs,
            &dir.join("payload").join(&item.payload),
            Path::new(&item.from),
        ) {
            let mut stayed = Vec::new();
            for back in done.iter().rev() {
                let target = dir.join("payload").join(&back.payload);
                if move_with(fs, Path::new(&back.from), &target).is_err() {
                    stayed.push(back.from.clone());
                }
            }
            let note = if stayed.is_empty() {
                String::new()
            } else {
                format!("; these stay restored: {}", stayed.join(", "))
            };
            return Err(failed(format!(
                "could not restore {}: {e}{note}",
                item.from
            )));
        }
        done.push(item);
    }
    remove_tree(&dir).map_err(|e| failed(e.to_string()))?;
    Ok(entry.items.iter().map(|i| i.from.clone()).collect())
}

/// Deletes the payloads for good.
pub fn forget(data_dir: &Path, store: Store, id: &str) -> Result<(), OpError> {
    remove_tree(&entry_dir(data_dir, store, id)).map_err(|e| failed(e.to_string()))
}

/// Drops the trash entries older than 30 days.
pub fn purge_expired(data_dir: &Path, now: i64) {
    for entry in fsx::list_dir(&base(data_dir, Store::Trash)) {
        if let Some(e) = read_entry(&entry.path) {
            if e.expires(Store::Trash).is_some_and(|at| at <= now) {
                let _ = remove_tree(&entry.path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let base = fsx::canonical(dir.path());
        let data = base.join("data");
        let src = base.join("projects/p/memory");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("MEMORY.md"), "- one\n").unwrap();
        (dir, data, src)
    }

    fn put_memory(data: &Path, src: &Path, now: i64) -> Entry {
        put(
            data,
            &Put {
                store: Store::Trash,
                kind: "memory",
                now,
                repo_root: Path::new("/r"),
                repo_name: "r",
                froms: &[src.to_path_buf()],
                switch_before: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn a_moved_folder_is_gone_from_its_place_and_comes_back_whole() {
        let (_d, data, src) = world();
        let entry = put_memory(&data, &src, 1_000_000);
        assert!(!src.exists());
        assert_eq!(entry.bytes(), 6);
        let found = find(&data, Store::Trash, "memory", None, None, 1_000_001).unwrap();
        assert_eq!(found.id, entry.id);
        restore(&data, Store::Trash, &found).unwrap();
        assert_eq!(
            fs::read_to_string(src.join("MEMORY.md")).unwrap(),
            "- one\n"
        );
        assert!(list(&data, Store::Trash, 1_000_001).is_empty());
    }

    #[test]
    fn restore_refuses_when_the_place_is_taken_and_moves_nothing() {
        let (_d, data, src) = world();
        let entry = put_memory(&data, &src, 1_000_000);
        fs::create_dir_all(&src).unwrap();
        let refused = restore(&data, Store::Trash, &entry).unwrap_err();
        assert!(refused.message.contains("exists again"));
        assert_eq!(list(&data, Store::Trash, 1_000_001).len(), 1);
    }

    #[test]
    fn trash_expires_after_thirty_days_and_the_archive_does_not() {
        let (_d, data, src) = world();
        let entry = put_memory(&data, &src, 1_000_000);
        let late = 1_000_000 + 30 * DAY;
        assert!(list(&data, Store::Trash, late - 1).len() == 1);
        assert!(list(&data, Store::Trash, late).is_empty());
        let err = find(&data, Store::Trash, "memory", Some(&entry.id), None, late).unwrap_err();
        assert!(err.message.contains("nothing called"));
        purge_expired(&data, late);
        assert!(!entry_dir(&data, Store::Trash, &entry.id).exists());
    }

    #[test]
    fn two_clears_in_one_second_get_two_entries() {
        let (_d, data, src) = world();
        let first = put_memory(&data, &src, 5);
        fs::create_dir_all(&src).unwrap();
        let second = put_memory(&data, &src, 5);
        assert_ne!(first.id, second.id);
    }

    type Rule = Box<dyn Fn(&Path, &Path) -> bool>;

    /// A second volume: renames across it report `CrossesDevices`, and each step of the copy
    /// fallback can be made to fail.
    struct Volumes {
        cross: bool,
        deny_rename: Option<Rule>,
        corrupt_copy: bool,
        fail_copy: bool,
        fail_remove: bool,
        copies: std::cell::Cell<u32>,
    }

    impl Volumes {
        fn apart() -> Self {
            Self {
                cross: true,
                deny_rename: None,
                corrupt_copy: false,
                fail_copy: false,
                fail_remove: false,
                copies: std::cell::Cell::new(0),
            }
        }

        fn same() -> Self {
            Self {
                cross: false,
                ..Self::apart()
            }
        }
    }

    impl MoveFs for Volumes {
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if self.deny_rename.as_ref().is_some_and(|deny| deny(from, to)) {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            if self.cross {
                return Err(io::ErrorKind::CrossesDevices.into());
            }
            fs::rename(from, to)
        }

        fn copy_tree(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.copies.set(self.copies.get() + 1);
            copy_tree(from, to)?;
            if self.fail_copy {
                return Err(io::Error::other("disk full"));
            }
            if self.corrupt_copy {
                let mut files = Vec::new();
                collect_files(to, &mut files);
                fs::write(&files[0], "x")?;
            }
            Ok(())
        }

        fn remove_source(&self, path: &Path) -> io::Result<()> {
            if self.fail_remove {
                let mut files = Vec::new();
                collect_files(path, &mut files);
                if let Some(first) = files.first() {
                    fs::remove_file(first)?;
                }
                return Err(io::Error::other("busy"));
            }
            remove_tree(path)
        }
    }

    fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut names: Vec<_> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        names.sort();
        for path in names {
            if path.is_dir() {
                collect_files(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    fn files_of(dir: &Path) -> Vec<(String, String)> {
        let mut files = Vec::new();
        collect_files(dir, &mut files);
        files
            .iter()
            .map(|p| {
                (
                    p.strip_prefix(dir).unwrap().to_string_lossy().into_owned(),
                    fs::read_to_string(p).unwrap(),
                )
            })
            .collect()
    }

    fn tree(src: &Path) {
        fs::create_dir_all(src.join("notes/deep")).unwrap();
        fs::write(src.join("notes/a.md"), "alpha\n").unwrap();
        fs::write(src.join("notes/deep/b.md"), "bravo\n").unwrap();
    }

    fn put_all(fs: &dyn MoveFs, data: &Path, froms: &[PathBuf]) -> Result<Entry, OpError> {
        put_with(
            fs,
            data,
            &Put {
                store: Store::Trash,
                kind: "memory",
                now: 1_000_000,
                repo_root: Path::new("/r"),
                repo_name: "r",
                froms,
                switch_before: None,
            },
        )
    }

    fn payload_of(data: &Path, entry: &Entry, index: usize) -> PathBuf {
        entry_dir(data, Store::Trash, &entry.id)
            .join("payload")
            .join(&entry.items[index].payload)
    }

    #[test]
    fn across_volumes_the_tree_is_copied_checked_and_only_then_removed() {
        let (_d, data, src) = world();
        tree(&src);
        let before = files_of(&src);
        let volumes = Volumes::apart();
        let entry = put_all(&volumes, &data, std::slice::from_ref(&src)).unwrap();
        assert_eq!(volumes.copies.get(), 1);
        assert!(!src.exists());
        assert_eq!(files_of(&payload_of(&data, &entry, 0)), before);
        let staged: Vec<_> = fsx::list_dir(payload_of(&data, &entry, 0).parent().unwrap())
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(staged, vec!["memory".to_string()]);
        restore_with(&volumes, &data, Store::Trash, &entry).unwrap();
        assert_eq!(files_of(&src), before);
        assert!(list(&data, Store::Trash, 1_000_001).is_empty());
    }

    #[test]
    fn an_error_that_is_not_a_second_volume_never_falls_back_to_a_copy() {
        let (_d, data, src) = world();
        tree(&src);
        let before = files_of(&src);
        let mut volumes = Volumes::same();
        volumes.deny_rename = Some(Box::new(|_, _| true));
        let err = put_all(&volumes, &data, std::slice::from_ref(&src)).unwrap_err();
        assert!(err.message.contains("could not move"), "{}", err.message);
        assert_eq!(volumes.copies.get(), 0);
        assert_eq!(files_of(&src), before);
        assert!(list(&data, Store::Trash, 1_000_001).is_empty());
        assert!(fsx::list_dir(&base(&data, Store::Trash)).is_empty());
    }

    #[test]
    fn a_copy_that_differs_from_the_original_leaves_the_source_alone() {
        let (_d, data, src) = world();
        tree(&src);
        let before = files_of(&src);
        let mut volumes = Volumes::apart();
        volumes.corrupt_copy = true;
        let err = put_all(&volumes, &data, std::slice::from_ref(&src)).unwrap_err();
        assert!(err.message.contains("differs"), "{}", err.message);
        assert_eq!(files_of(&src), before);
        assert!(list(&data, Store::Trash, 1_000_001).is_empty());
        let leftovers = fsx::list_dir(&base(&data, Store::Trash));
        assert!(leftovers.is_empty());
    }

    #[test]
    fn a_copy_that_fails_midway_leaves_the_source_alone() {
        let (_d, data, src) = world();
        tree(&src);
        let before = files_of(&src);
        let mut volumes = Volumes::apart();
        volumes.fail_copy = true;
        let err = put_all(&volumes, &data, std::slice::from_ref(&src)).unwrap_err();
        assert!(err.message.contains("disk full"), "{}", err.message);
        assert_eq!(files_of(&src), before);
        assert!(fsx::list_dir(&base(&data, Store::Trash)).is_empty());
    }

    #[test]
    fn a_source_that_cannot_be_removed_keeps_the_whole_copy_and_says_so() {
        let (_d, data, src) = world();
        tree(&src);
        let before = files_of(&src);
        let mut volumes = Volumes::apart();
        volumes.fail_remove = true;
        let err = put_all(&volumes, &data, std::slice::from_ref(&src)).unwrap_err();
        assert!(err.message.contains("kept"), "{}", err.message);
        assert!(err.message.contains("busy"), "{}", err.message);
        let kept = list(&data, Store::Trash, 1_000_001);
        assert_eq!(kept.len(), 1);
        assert_eq!(files_of(&payload_of(&data, &kept[0], 0)), before);
        assert!(src.exists());
        assert_eq!(files_of(&src).len(), before.len() - 1);
    }

    #[test]
    fn a_failure_on_the_second_item_puts_the_first_back() {
        let (_d, data, src) = world();
        tree(&src);
        let other = src.parent().unwrap().join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("x.md"), "xray\n").unwrap();
        let (before_src, before_other) = (files_of(&src), files_of(&other));
        let mut volumes = Volumes::same();
        let denied = other.clone();
        volumes.deny_rename = Some(Box::new(move |from, _| from == denied));
        let err = put_all(&volumes, &data, &[src.clone(), other.clone()]).unwrap_err();
        assert!(err.message.contains("could not move"), "{}", err.message);
        assert_eq!(files_of(&src), before_src);
        assert_eq!(files_of(&other), before_other);
        assert!(fsx::list_dir(&base(&data, Store::Trash)).is_empty());
    }

    #[test]
    fn what_cannot_be_put_back_after_a_failure_stays_listed_in_the_trash() {
        let (_d, data, src) = world();
        tree(&src);
        let other = src.parent().unwrap().join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("x.md"), "xray\n").unwrap();
        let before_src = files_of(&src);
        let mut volumes = Volumes::same();
        let (first, second) = (src.clone(), other.clone());
        volumes.deny_rename = Some(Box::new(move |from, to| from == second || to == first));
        let err = put_all(&volumes, &data, &[src.clone(), other.clone()]).unwrap_err();
        assert!(err.message.contains("stay in the trash"), "{}", err.message);
        assert!(!src.exists());
        let kept = list(&data, Store::Trash, 1_000_001);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].items.len(), 1);
        assert_eq!(files_of(&payload_of(&data, &kept[0], 0)), before_src);
    }

    #[test]
    fn a_restore_that_fails_halfway_keeps_what_it_could_not_move_back_where_it_is() {
        let (_d, data, src) = world();
        tree(&src);
        let other = src.parent().unwrap().join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("x.md"), "xray\n").unwrap();
        let entry = put_all(&Volumes::same(), &data, &[src.clone(), other.clone()]).unwrap();
        let mut volumes = Volumes::same();
        let (first, second) = (src.clone(), other.clone());
        volumes.deny_rename = Some(Box::new(move |from, to| to == second || from == first));
        let err = restore_with(&volumes, &data, Store::Trash, &entry).unwrap_err();
        assert!(err.message.contains("stay restored"), "{}", err.message);
        assert!(src.exists());
        assert!(!other.exists());
        assert!(payload_of(&data, &entry, 1).exists());
    }

    #[test]
    fn a_copy_is_only_whole_when_every_file_matches_by_content() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        for side in [&a, &b] {
            fs::create_dir_all(side.join("sub")).unwrap();
            fs::write(side.join("sub/f.md"), "same size\n").unwrap();
        }
        verify_copy(&a, &b).unwrap();
        fs::write(b.join("sub/f.md"), "SAME SIZE\n").unwrap();
        assert!(verify_copy(&a, &b)
            .unwrap_err()
            .to_string()
            .contains("sub/f.md"));
        fs::write(b.join("sub/f.md"), "same size\n").unwrap();
        fs::write(b.join("extra.md"), "x").unwrap();
        assert!(verify_copy(&a, &b)
            .unwrap_err()
            .to_string()
            .contains("extra.md"));
    }
}
