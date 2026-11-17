//! What skills, agents and styles share: documents laid out as `<dir>/<name>/<FILE>` under the
//! repository, each with YAML frontmatter. Discovery, lookup and the one-line inventory are
//! written once over [`ContentKind`].

use std::path::{Path, PathBuf};

pub trait ContentKind {
    type Item;
    const NOUN: &'static str;
    const DIRS: &'static [&'static str];
    const FILE: &'static str;

    fn parse(path: &Path) -> Result<Self::Item, String>;
}

/// The item directories of the kind, each content directory's children in byte order of their
/// names.
pub fn item_dirs<K: ContentKind>(repo: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for dir in K::DIRS {
        let Ok(read) = std::fs::read_dir(repo.join(dir)) else {
            continue;
        };
        let mut children: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
        children.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        dirs.extend(children.into_iter().filter(|child| child.is_dir()));
    }
    dirs
}

/// A directory whose file exists but does not parse, so it never reaches sync.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseFailure {
    /// The directory's name.
    pub name: String,
    /// The file's path relative to the repository.
    pub path: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct Found<T> {
    pub items: Vec<T>,
    pub warnings: Vec<String>,
    pub failures: Vec<ParseFailure>,
}

/// The items that parse, and a warning for each directory that does not hold one.
pub fn discover_report<K: ContentKind>(repo: &Path) -> Found<K::Item> {
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut failures = Vec::new();
    for dir in item_dirs::<K>(repo) {
        let file = dir.join(K::FILE);
        if !file.exists() {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
            warnings.push(format!(
                "{} directory {} has no {}, skipping",
                K::NOUN,
                name.unwrap_or_default(),
                K::FILE
            ));
            continue;
        }
        match K::parse(&file) {
            Ok(item) => items.push(item),
            Err(e) => {
                warnings.push(format!("Failed to parse {}: {e}", file.display()));
                failures.push(ParseFailure {
                    name: dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    path: file
                        .strip_prefix(repo)
                        .unwrap_or(&file)
                        .to_string_lossy()
                        .into_owned(),
                    reason: e,
                });
            }
        }
    }
    Found {
        items,
        warnings,
        failures,
    }
}

pub fn discover<K: ContentKind>(repo: &Path) -> Vec<K::Item> {
    discover_report::<K>(repo).items
}
