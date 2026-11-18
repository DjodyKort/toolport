//! What a repository's auto memory folder holds: files, sizes, kinds, and what the index costs.

use crate::context::fsx;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const INDEX_LINES: usize = 200;
pub const INDEX_BYTES: usize = 25 * 1024;
const FRONT_MATTER_BYTES: usize = 2048;
const MAX_FILES: usize = 2000;

#[derive(Clone, Debug)]
pub struct MemFile {
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: i64,
    pub kind: Option<String>,
    pub description: Option<String>,
    pub index: bool,
}

impl MemFile {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "path": fsx::display(&self.path),
            "bytes": self.bytes,
            "modified": fsx::zulu(self.modified),
            "type": self.kind,
            "description": self.description,
            "index": self.index,
        })
    }
}

fn front_matter(text: &str, key: &str) -> Option<String> {
    let rest = text.strip_prefix("---")?;
    rest.lines()
        .skip(1)
        .take_while(|l| l.trim_end() != "---")
        .find_map(|l| {
            let value = l.strip_prefix(key)?.strip_prefix(':')?;
            let value = value.trim().trim_matches(['"', '\'']);
            (!value.is_empty()).then(|| value.to_string())
        })
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 3 || out.len() >= MAX_FILES {
        return;
    }
    for entry in fsx::list_dir(dir) {
        match entry.kind {
            fsx::Kind::Dir if !entry.symlink => collect(&entry.path, depth + 1, out),
            fsx::Kind::File => out.push(entry.path),
            _ => {}
        }
    }
}

/// The index first, then the topic files by name.
pub fn files(dir: &Path) -> Vec<MemFile> {
    let mut paths = Vec::new();
    collect(dir, 0, &mut paths);
    let mut out: Vec<MemFile> = paths
        .into_iter()
        .filter_map(|path| {
            let stamp = fsx::stamp(&path)?;
            let head = fsx::read_text(&path, FRONT_MATTER_BYTES).unwrap_or_default();
            let name = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            Some(MemFile {
                index: name == "MEMORY.md",
                kind: front_matter(&head, "type"),
                description: front_matter(&head, "description"),
                name,
                bytes: stamp.len,
                modified: (stamp.mtime_ns / 1_000_000_000) as i64,
                path,
            })
        })
        .collect();
    out.sort_by(|a, b| b.index.cmp(&a.index).then_with(|| a.name.cmp(&b.name)));
    out
}

pub struct Index {
    pub lines: usize,
    pub bytes: u64,
    pub loaded_lines: usize,
    pub truncated: bool,
    pub tokens: u64,
}

/// The first 200 lines or 25 KB of `MEMORY.md` are what every session loads.
pub fn index(dir: &Path, count_tokens: &dyn Fn(&str) -> u64) -> Option<Index> {
    let path = dir.join("MEMORY.md");
    let stamp = fsx::stamp(&path)?;
    let text = fsx::read_text(&path, INDEX_BYTES)?;
    let lines = text.lines().count();
    let kept: Vec<&str> = text.lines().take(INDEX_LINES).collect();
    Some(Index {
        lines,
        bytes: stamp.len,
        loaded_lines: kept.len(),
        truncated: lines > INDEX_LINES || stamp.len > INDEX_BYTES as u64,
        tokens: count_tokens(&kept.join("\n")),
    })
}

pub struct Summary {
    pub count: usize,
    pub topic_files: usize,
    pub bytes: u64,
    pub last_written: Option<i64>,
}

pub fn summary(files: &[MemFile]) -> Summary {
    Summary {
        count: files.len(),
        topic_files: files.iter().filter(|f| !f.index).count(),
        bytes: files.iter().map(|f| f.bytes).sum(),
        last_written: files.iter().map(|f| f.modified).max(),
    }
}

pub fn summary_json(s: &Summary) -> Value {
    json!({
        "count": s.count,
        "topicFiles": s.topic_files,
        "bytes": s.bytes,
        "lastWritten": s.last_written.map(fsx::zulu),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_list_the_index_first_and_read_the_type_from_the_front_matter() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("MEMORY.md"), "- [a](a.md)\n- [b](b.md)\n").unwrap();
        std::fs::write(
            root.join("b.md"),
            "---\nname: b\ntype: feedback\ndescription: \"keep it short\"\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(root.join("a.md"), "no front matter\n").unwrap();
        let listed = files(root);
        let names: Vec<&str> = listed.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["MEMORY.md", "a.md", "b.md"]);
        assert_eq!(listed[2].kind.as_deref(), Some("feedback"));
        assert_eq!(listed[2].description.as_deref(), Some("keep it short"));
        assert_eq!(listed[1].kind, None);
        let s = summary(&listed);
        assert_eq!((s.count, s.topic_files), (3, 2));
    }

    #[test]
    fn only_the_first_200_lines_of_the_index_count() {
        let dir = tempfile::tempdir().unwrap();
        let long: String = (0..250).map(|i| format!("- line {i}\n")).collect();
        std::fs::write(dir.path().join("MEMORY.md"), long).unwrap();
        let count = |t: &str| t.lines().count() as u64;
        let info = index(dir.path(), &count).unwrap();
        assert_eq!(
            (info.lines, info.loaded_lines, info.tokens),
            (250, 200, 200)
        );
        assert!(info.truncated);
        assert!(index(&dir.path().join("none"), &count).is_none());
    }
}
