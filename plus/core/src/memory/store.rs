//! Toolport's own trash and archive for memory and sessions. Both live in Toolport's data folder,
//! never inside `~/.claude/projects`: Claude Code reads what is there as memory and sessions, and
//! a session found twice makes `--resume <id>` report not-found. Everything is moved, never
//! copied, so a clear and its undo leave exactly one copy.

use crate::context::fsx;
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
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
        fs::copy(from, to).map(|_| ())
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

/// A rename; across volumes a copy that is verified by the rename of its last step, then the
/// source is removed, so the end state is still one copy.
pub fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            if let Err(e) = copy_tree(from, to) {
                let _ = remove_tree(to);
                return Err(e);
            }
            remove_tree(from)
        }
    }
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

/// Moves `froms` into a new entry. A failure puts back what was already moved.
pub fn put(data_dir: &Path, put: &Put) -> Result<Entry, OpError> {
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
        if let Err(e) = move_path(from, &dir.join("payload").join(&payload)) {
            for done in items.iter().rev() {
                let _ = move_path(
                    &dir.join("payload").join(&done.payload),
                    Path::new(&done.from),
                );
            }
            let _ = remove_tree(&dir);
            return Err(failed(format!("could not move {}: {e}", from.display())));
        }
        items.push(Item {
            from: fsx::display(from),
            payload,
            bytes,
        });
    }
    let entry = Entry {
        id,
        kind: put.kind.to_string(),
        created: put.now,
        repo_root: fsx::display(put.repo_root),
        repo_name: put.repo_name.to_string(),
        items,
        switch_before: put.switch_before.clone(),
    };
    write_manifest(&dir, &entry)?;
    Ok(entry)
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
        if let Err(e) = move_path(
            &dir.join("payload").join(&item.payload),
            Path::new(&item.from),
        ) {
            for back in done.iter().rev() {
                let _ = move_path(
                    Path::new(&back.from),
                    &dir.join("payload").join(&back.payload),
                );
            }
            return Err(failed(format!("could not restore {}: {e}", item.from)));
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
}
