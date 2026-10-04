//! The only filesystem calls a detector may make: stat, list and read. There is no handle that can
//! write, create, rename or delete, so "read-only" holds by construction.

use crate::plus::skills::clock::{Instant, SystemClock};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const TEXT_CAP: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    Other,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    pub symlink: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub mtime_ns: u128,
    pub len: u64,
}

impl Stamp {
    pub fn key(&self) -> String {
        format!("{}:{}", self.mtime_ns, self.len)
    }
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(Stamp {
        mtime_ns: mtime.as_nanos(),
        len: meta.len(),
    })
}

pub fn mtime_secs(path: &Path) -> Option<i64> {
    stamp(path).map(|s| (s.mtime_ns / 1_000_000_000) as i64)
}

pub fn is_dir(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_dir())
}

pub fn is_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// Children in byte order of their names; symlinks are classified by their target.
pub fn list_dir(path: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(path) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            let symlink = e.file_type().is_ok_and(|t| t.is_symlink());
            let kind = match fs::metadata(&p) {
                Ok(m) if m.is_dir() => Kind::Dir,
                Ok(m) if m.is_file() => Kind::File,
                _ => Kind::Other,
            };
            Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: p,
                kind,
                symlink,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// The first `cap` bytes as text; `None` when the file cannot be opened.
pub fn read_text(path: &Path, cap: usize) -> Option<String> {
    let mut buf = Vec::new();
    File::open(path)
        .ok()?
        .take(cap as u64)
        .read_to_end(&mut buf)
        .ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

pub fn read_bytes(path: &Path, cap: usize) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    File::open(path)
        .ok()?
        .take(cap as u64)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

pub fn zulu(unix_secs: i64) -> String {
    Instant {
        unix_secs,
        micros: 0,
    }
    .zulu()
}

pub fn now_zulu() -> String {
    use crate::plus::skills::clock::Clock;
    SystemClock.now().zulu()
}

pub fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_sorted_classifies_and_reads_capped() {
        let dir = std::env::temp_dir().join(format!("sources-fsx-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("b")).unwrap();
        fs::write(dir.join("a.md"), "hello world").unwrap();
        let names: Vec<(String, Kind)> = list_dir(&dir)
            .into_iter()
            .map(|e| (e.name, e.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("a.md".to_string(), Kind::File),
                ("b".to_string(), Kind::Dir)
            ]
        );
        assert_eq!(read_text(&dir.join("a.md"), 5).as_deref(), Some("hello"));
        assert_eq!(stamp(&dir.join("a.md")).unwrap().len, 11);
        assert!(list_dir(&dir.join("missing")).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn zulu_formats_utc() {
        assert_eq!(zulu(0), "1970-01-01T00:00:00Z");
        assert_eq!(zulu(1_709_210_096), "2024-02-29T12:34:56Z");
    }
}
