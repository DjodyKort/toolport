//! Everything in a skill folder that is installed next to its SKILL.md, and the content hash that
//! covers it. The whole folder is installed except what a fixed deny-list names, so nothing is
//! dropped without being reported.

use super::parser::Skill;
use crate::hashing::finish_lock_hash;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const DENY_DIRS: &[&str] = &[
    ".git",
    "__pycache__",
    "node_modules",
    ".venv",
    ".pytest_cache",
    "dist",
    "build",
    "target",
];
pub const DENY_FILES: &[&str] = &[".DS_Store"];
pub const DENY_SUFFIXES: &[&str] = &[".pyc", ".log"];

pub const SKILL_FILE: &str = "SKILL.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason {
    Denied(String),
    LinkLeavesFolder,
    BrokenLink,
    Unreadable(String),
}

impl SkipReason {
    pub fn describe(&self) -> String {
        match self {
            Self::Denied(rule) => format!("on the deny-list ({rule})"),
            Self::LinkLeavesFolder => "link leaves the folder".into(),
            Self::BrokenLink => "link target does not exist".into(),
            Self::Unreadable(why) => format!("unreadable: {why}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedFile {
    pub rel: String,
    pub reason: SkipReason,
}

/// A file or link that a sync installs, relative to the skill folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillFile {
    pub rel: String,
    /// The link text when the entry is a relative symlink to something inside the folder.
    pub link: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub files: Vec<SkillFile>,
    pub skipped: Vec<SkippedFile>,
}

pub fn rel_string(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The deny-list rule that excludes a folder entry, if any.
pub fn denied_by(name: &str, is_dir: bool) -> Option<String> {
    if is_dir {
        return DENY_DIRS.contains(&name).then(|| format!("{name}/"));
    }
    if DENY_FILES.contains(&name) {
        return Some(name.to_string());
    }
    DENY_SUFFIXES
        .iter()
        .find(|suffix| name.ends_with(*suffix))
        .map(|suffix| format!("*{suffix}"))
}

fn walk(dir: &Path, base: &Path, real_base: &Path, out: &mut Inventory) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(base) else {
            continue;
        };
        let rel_text = rel_string(rel);
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            classify_link(&path, &rel_text, real_base, out);
            continue;
        }
        if let Some(rule) = denied_by(&name, meta.is_dir()) {
            out.skipped.push(SkippedFile {
                rel: if meta.is_dir() {
                    format!("{rel_text}/")
                } else {
                    rel_text
                },
                reason: SkipReason::Denied(rule),
            });
            continue;
        }
        if meta.is_dir() {
            walk(&path, base, real_base, out);
        } else if meta.is_file() {
            out.files.push(SkillFile {
                rel: rel_text,
                link: None,
            });
        }
    }
}

fn classify_link(path: &Path, rel: &str, real_base: &Path, out: &mut Inventory) {
    let skip = |out: &mut Inventory, reason| {
        out.skipped.push(SkippedFile {
            rel: rel.to_string(),
            reason,
        })
    };
    let Ok(text) = fs::read_link(path) else {
        return skip(out, SkipReason::Unreadable("link".into()));
    };
    let Ok(real) = fs::canonicalize(path) else {
        return skip(out, SkipReason::BrokenLink);
    };
    if text.is_absolute() || !real.starts_with(real_base) {
        return skip(out, SkipReason::LinkLeavesFolder);
    }
    out.files.push(SkillFile {
        rel: rel.to_string(),
        link: Some(text),
    });
}

/// What a sync would install next to `SKILL.md` and what it would leave out, sorted by path.
pub fn inventory(src_dir: &Path) -> Inventory {
    let mut out = Inventory::default();
    let real_base = fs::canonicalize(src_dir).unwrap_or_else(|_| src_dir.to_path_buf());
    walk(src_dir, src_dir, &real_base, &mut out);
    out.files.retain(|f| f.rel != SKILL_FILE);
    out.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    out.skipped.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// `sha256:` plus the first 16 hex digits over `SKILL.md\n<bytes>\n---\n` followed by
/// `<rel>\n<bytes>\n---\n` for each installed file in sorted relative-path order.
pub fn compute_skill_hash(skill: &Skill) -> Result<String, String> {
    let src_dir = skill.source_dir();
    let mut h = Sha256::new();
    h.update(b"SKILL.md\n");
    h.update(
        fs::read(&skill.source_path)
            .map_err(|e| format!("{}: {e}", skill.source_path.display()))?,
    );
    h.update(b"\n---\n");
    for file in inventory(src_dir).files {
        h.update(format!("{}\n", file.rel).as_bytes());
        match &file.link {
            Some(text) => h.update(format!("link:{}", text.display()).as_bytes()),
            None => {
                let full = src_dir.join(&file.rel);
                h.update(fs::read(&full).map_err(|e| format!("{}: {e}", full.display()))?);
            }
        }
        h.update(b"\n---\n");
    }
    Ok(finish_lock_hash(h))
}

/// True when `rel` is a plain relative path inside its folder.
pub fn is_plain_relative(rel: &str) -> bool {
    Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}
