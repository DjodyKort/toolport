//! Reading and copying folders that someone else wrote: a tap clone, a plugin checkout, a skill
//! folder. Every call looks at a path with `symlink_metadata` and never follows a link, so a
//! checkout cannot point Toolport at a file outside of it. Names that could leave a folder
//! (separators, `..`, absolute paths, NUL) are refused, writes are checked to stay under their
//! destination, and a folder's file count and size are capped.
//!
//! Content that belongs to the user (their own `~/.claude`, a skills library they keep) is not
//! read through here: a link in it is the user's own and works as before.

use crate::fsx::{atomic_write_bytes, Mode};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const MAX_ITEM_FILES: usize = 2_000;
pub const MAX_ITEM_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Link,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Links {
    /// A link anywhere in the tree is an error and names the link.
    Reject,
    /// Links are left behind and listed in [`Tree::skipped`].
    Skip,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub files: usize,
    pub bytes: u64,
    pub file_bytes: u64,
    pub depth: usize,
}

impl Limits {
    /// What one skill, rule, agent or hook folder may hold.
    pub const ITEM: Limits = Limits {
        files: MAX_ITEM_FILES,
        bytes: MAX_ITEM_BYTES,
        file_bytes: MAX_FILE_BYTES,
        depth: MAX_DEPTH,
    };
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Dir {
        rel: String,
    },
    File {
        rel: String,
        path: PathBuf,
        len: u64,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// Depth-first in byte order of the names, a folder before its children.
    pub nodes: Vec<Node>,
    pub skipped: Vec<String>,
}

impl Tree {
    pub fn files(&self) -> impl Iterator<Item = (&str, &Path, u64)> {
        self.nodes.iter().filter_map(|n| match n {
            Node::File { rel, path, len } => Some((rel.as_str(), path.as_path(), *len)),
            Node::Dir { .. } => None,
        })
    }
}

fn kind_of(meta: &fs::Metadata) -> Kind {
    let t = meta.file_type();
    if t.is_symlink() {
        Kind::Link
    } else if t.is_dir() {
        Kind::Dir
    } else if t.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

/// What `path` itself is, without following it; `None` when it does not exist.
pub fn kind(path: &Path) -> Option<Kind> {
    fs::symlink_metadata(path).ok().map(|m| kind_of(&m))
}

pub fn is_file(path: &Path) -> bool {
    kind(path) == Some(Kind::File)
}

pub fn is_dir(path: &Path) -> bool {
    kind(path) == Some(Kind::Dir)
}

/// One path component: no separator, no `..`, not absolute, no NUL.
pub fn check_name(name: &str) -> Result<(), String> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || Path::new(name).is_absolute()
        || !matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        );
    if bad {
        return Err(format!("{name:?} is not a plain file or folder name"));
    }
    Ok(())
}

/// A relative path written with `/`: every part passes [`check_name`].
pub fn check_rel(rel: &str) -> Result<(), String> {
    if rel.is_empty() {
        return Err("an empty path".to_string());
    }
    rel.split('/').try_for_each(check_name)
}

/// `root/rel` after checking every part of `rel`: each is a plain name and none that exists is a
/// link, so the result cannot lead out of `root`. The last part may be missing.
pub fn below(root: &Path, rel: &str) -> Result<PathBuf, String> {
    check_rel(rel).map_err(|why| format!("{rel}: {why}"))?;
    let mut path = root.to_path_buf();
    for part in rel.split('/') {
        path.push(part);
        if kind(&path) == Some(Kind::Link) {
            return Err(format!(
                "{rel} goes through a symbolic link, which is never followed"
            ));
        }
    }
    Ok(path)
}

/// The children of `dir` in byte order of their names, links reported as links. Empty when `dir`
/// is not a real folder (a link to a folder is not entered).
pub fn list_dir(dir: &Path) -> Vec<Entry> {
    if !is_dir(dir) {
        return Vec::new();
    }
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            check_name(&name).ok()?;
            let path = e.path();
            let kind = kind(&path)?;
            Some(Entry { name, path, kind })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

#[cfg(unix)]
fn same_file(meta: &fs::Metadata, file: &fs::File) -> bool {
    use std::os::unix::fs::MetadataExt;
    file.metadata()
        .is_ok_and(|open| open.dev() == meta.dev() && open.ino() == meta.ino())
}

#[cfg(not(unix))]
fn same_file(_: &fs::Metadata, _: &fs::File) -> bool {
    true
}

/// The bytes of a regular file of at most `cap` bytes. A link, a folder and anything bigger than
/// `cap` is an error; the opened file must be the one that was looked at.
pub fn read_bytes(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match kind_of(&meta) {
        Kind::File => {}
        Kind::Link => {
            return Err(format!(
                "{} is a symbolic link, which is never followed",
                path.display()
            ))
        }
        _ => return Err(format!("{} is not a file", path.display())),
    }
    if meta.len() > cap {
        return Err(format!("{} is larger than {cap} bytes", path.display()));
    }
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !same_file(&meta, &file) {
        return Err(format!("{} changed while it was read", path.display()));
    }
    let mut buf = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if buf.len() as u64 > cap {
        return Err(format!("{} is larger than {cap} bytes", path.display()));
    }
    Ok(buf)
}

/// [`read_bytes`] as lossy text, `None` on any refusal.
pub fn read_text(path: &Path, cap: u64) -> Option<String> {
    read_bytes(path, cap)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// A JSON file, `None` when it is a link, too big, missing or not JSON.
pub fn read_json(path: &Path, cap: u64) -> Option<serde_json::Value> {
    serde_json::from_str(&read_text(path, cap)?).ok()
}

struct Walk<'a> {
    links: Links,
    limits: &'a Limits,
    skip_dirs: &'a [&'a str],
    tree: Tree,
    bytes: u64,
    files: usize,
}

impl Walk<'_> {
    fn dir(&mut self, base: &Path, rel: &str, depth: usize) -> Result<(), String> {
        if depth > self.limits.depth {
            return Err(format!(
                "{rel}: nested deeper than {} folders",
                self.limits.depth
            ));
        }
        for entry in list_dir(&base.join(rel)) {
            let child = if rel.is_empty() {
                entry.name.clone()
            } else {
                format!("{rel}/{}", entry.name)
            };
            match entry.kind {
                Kind::Link => match self.links {
                    Links::Reject => {
                        return Err(format!(
                            "{child} is a symbolic link, which is never followed"
                        ))
                    }
                    Links::Skip => self.tree.skipped.push(child),
                },
                Kind::Dir if self.skip_dirs.contains(&entry.name.as_str()) => {}
                Kind::Dir => {
                    self.tree.nodes.push(Node::Dir { rel: child.clone() });
                    self.dir(base, &child, depth + 1)?;
                }
                Kind::File => {
                    let len = fs::symlink_metadata(&entry.path)
                        .map_err(|e| format!("{child}: {e}"))?
                        .len();
                    self.files += 1;
                    self.bytes += len;
                    if self.files > self.limits.files {
                        return Err(format!(
                            "{child}: more than {} files, which is too many to copy",
                            self.limits.files
                        ));
                    }
                    if len > self.limits.file_bytes {
                        return Err(format!(
                            "{child}: larger than {} bytes, which is too large to copy",
                            self.limits.file_bytes
                        ));
                    }
                    if self.bytes > self.limits.bytes {
                        return Err(format!(
                            "{child}: the folder holds more than {} bytes, which is too large to copy",
                            self.limits.bytes
                        ));
                    }
                    self.tree.nodes.push(Node::File {
                        rel: child,
                        path: entry.path,
                        len,
                    });
                }
                Kind::Other => {}
            }
        }
        Ok(())
    }
}

/// Every file and folder below the folder `root`, links handled as `links` says. `skip_dirs` are
/// folder names that are left out at every level (`.git`).
pub fn walk(
    root: &Path,
    links: Links,
    limits: &Limits,
    skip_dirs: &[&str],
) -> Result<Tree, String> {
    match kind(root) {
        Some(Kind::Dir) => {}
        Some(Kind::Link) => {
            return Err(format!(
                "{} is a symbolic link, which is never followed",
                root.display()
            ))
        }
        _ => return Err(format!("{} is not a folder", root.display())),
    }
    let mut walk = Walk {
        links,
        limits,
        skip_dirs,
        tree: Tree::default(),
        bytes: 0,
        files: 0,
    };
    walk.dir(root, "", 0)?;
    Ok(walk.tree)
}

/// Fails unless `path` lies below the real location of `root` and is not itself a link. The
/// nearest existing ancestor is resolved, so a link anywhere between the two is caught before
/// anything is written.
pub fn check_write(root: &Path, path: &Path) -> Result<(), String> {
    let real_root = fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if kind(path) == Some(Kind::Link) {
        return Err(format!(
            "{} is a symbolic link, so nothing is written through it",
            path.display()
        ));
    }
    let mut ancestor = path;
    let mut tail = Vec::new();
    let real = loop {
        if let Ok(real) = fs::canonicalize(ancestor) {
            break real;
        }
        match (ancestor.parent(), ancestor.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_owned());
                ancestor = parent;
            }
            _ => return Err(format!("{} cannot be resolved", path.display())),
        }
    };
    let resolved = tail.iter().rev().fold(real, |p, part| p.join(part));
    if !resolved.starts_with(&real_root) {
        return Err(format!(
            "{} would be written outside {}",
            path.display(),
            root.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn mode_of(path: &Path) -> Mode {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path)
        .map(|m| Mode::Exact(m.permissions().mode() & 0o777))
        .unwrap_or(Mode::Private)
}

#[cfg(not(unix))]
fn mode_of(_: &Path) -> Mode {
    Mode::Private
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Copied {
    pub files: usize,
    pub skipped: Vec<String>,
}

fn write_file(root: &Path, from: &Path, to: &Path, len_cap: u64) -> Result<(), String> {
    let bytes = read_bytes(from, len_cap)?;
    check_write(root, to)?;
    atomic_write_bytes(to, &bytes, mode_of(from)).map_err(|e| format!("{}: {e}", to.display()))
}

/// Copies a file, or a folder's files and folders, to `to`. Nothing is followed: `from` itself
/// may not be a link, and a link inside it is an error or is left behind as `links` says. Every
/// destination is checked to stay below `to` before it is written.
pub fn copy_tree(
    from: &Path,
    to: &Path,
    links: Links,
    limits: &Limits,
    skip_dirs: &[&str],
) -> Result<Copied, String> {
    match kind(from) {
        Some(Kind::File) => {
            let parent = to.parent().unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            write_file(parent, from, to, limits.file_bytes)?;
            return Ok(Copied {
                files: 1,
                skipped: Vec::new(),
            });
        }
        Some(Kind::Dir) => {}
        Some(Kind::Link) => {
            return Err(format!(
                "{} is a symbolic link, which is never followed",
                from.display()
            ))
        }
        Some(Kind::Other) => return Err(format!("{} is not a file or folder", from.display())),
        None => return Err(format!("{} does not exist", from.display())),
    }
    let tree = walk(from, links, limits, skip_dirs)?;
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut copied = Copied {
        files: 0,
        skipped: tree.skipped.clone(),
    };
    for node in &tree.nodes {
        match node {
            Node::Dir { rel } => {
                check_rel(rel)?;
                let dir = to.join(rel);
                check_write(to, &dir)?;
                fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            Node::File { rel, path, .. } => {
                check_rel(rel)?;
                write_file(to, path, &to.join(rel), limits.file_bytes)?;
                copied.files += 1;
            }
        }
    }
    Ok(copied)
}

#[cfg(all(test, unix))]
pub(crate) mod fixture;
#[cfg(all(test, unix))]
mod tests;
