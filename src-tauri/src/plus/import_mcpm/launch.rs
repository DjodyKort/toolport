use crate::registry::ServerEntry;
use std::path::{Path, PathBuf};

pub const SCRIPTS_SUBDIR: &str = "imported-scripts";

const SCRIPT_SOURCE_DIRS: &[&str] = &[".config/mcpm/bin/", ".claude/mcp-servers/"];

const ENV_DIRS: &[&str] = &[
    ".venv",
    "venv",
    ".virtualenv",
    "virtualenv",
    "site-packages",
    "node_modules",
    "__pypackages__",
];

const PACKAGE_MARKERS: &[&str] = &[
    "pyvenv.cfg",
    ".venv",
    "venv",
    "node_modules",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "setup.cfg",
    "Pipfile",
    "poetry.lock",
    "uv.lock",
    "go.mod",
    "Cargo.toml",
    "Gemfile",
    "__init__.py",
];

pub fn scripts_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(SCRIPTS_SUBDIR)
}

fn source_rel<'a>(home: &str, path: &'a str) -> Option<(&'static str, &'a str)> {
    let home = home.trim_end_matches('/');
    SCRIPT_SOURCE_DIRS.iter().find_map(|dir| {
        let rel = path.strip_prefix(&format!("{home}/{dir}"))?;
        let rel = rel.trim_start_matches('/');
        let clean = !rel.is_empty()
            && rel
                .split('/')
                .all(|seg| !seg.is_empty() && seg != "." && seg != "..");
        clean.then_some((*dir, rel))
    })
}

pub fn relocate_script(home: &str, data_dir: &Path, path: &str) -> Option<PathBuf> {
    source_rel(home, path).map(|(dir, rel)| {
        let tag = dir.trim_end_matches('/').replace('/', "_");
        scripts_dir(data_dir)
            .join(tag.trim_start_matches('.'))
            .join(rel)
    })
}

/// The first directory under a script source dir (`<home>/.claude/mcp-servers/<name>`): a loose
/// file directly in the source dir has none.
fn package_root(home: &str, path: &str) -> Option<String> {
    let (dir, rel) = source_rel(home, path)?;
    let (first, _) = rel.split_once('/')?;
    Some(format!("{}/{dir}{first}", home.trim_end_matches('/')))
}

fn env_dir_segment(rel: &str) -> Option<&str> {
    let mut parts: Vec<&str> = rel.split('/').collect();
    parts.pop();
    parts.into_iter().find(|seg| ENV_DIRS.contains(seg))
}

/// Why `path` cannot move on its own: it sits in a virtualenv (named or marked by a
/// `pyvenv.cfg`) or in a package directory that keeps its dependencies beside it. Reads the
/// directories it names; a path that does not exist is judged by its name alone.
fn pinned_reason(home: &str, path: &str) -> Option<String> {
    let (_, rel) = source_rel(home, path)?;
    if let Some(seg) = env_dir_segment(rel) {
        return Some(format!(
            "it lives inside a virtualenv or dependency tree ({seg})"
        ));
    }
    let root = package_root(home, path)?;
    let mut dir = Path::new(path).parent();
    while let Some(d) = dir {
        if d.join("pyvenv.cfg").exists() {
            return Some(format!("it lives inside the virtualenv {}", d.display()));
        }
        if let Some(marker) = PACKAGE_MARKERS.iter().find(|m| d.join(m).exists()) {
            return Some(format!(
                "its package directory {} keeps dependencies beside it ({marker})",
                d.display()
            ));
        }
        if d == Path::new(&root) {
            break;
        }
        dir = d.parent();
    }
    None
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Relocation {
    /// Where each path moved: the original, then its place under the data dir.
    pub moves: Vec<(String, PathBuf)>,
    /// A path that would have moved but has to stay where it is, with the reason.
    pub kept: Vec<(String, String)>,
}

pub fn relocate_entry(entry: &mut ServerEntry, home: &str, data_dir: &Path) -> Relocation {
    let paths: Vec<String> = entry
        .command
        .iter()
        .chain(entry.args.iter())
        .cloned()
        .collect();
    let pinned: Vec<(String, String)> = paths
        .iter()
        .filter_map(|p| Some((package_root(home, p)?, pinned_reason(home, p)?)))
        .collect();
    let mut out = Relocation::default();
    let mut map = |s: &mut String| {
        let Some(to) = relocate_script(home, data_dir, s) else {
            return;
        };
        let own = pinned_reason(home, s);
        let shared = || {
            let root = package_root(home, s)?;
            pinned
                .iter()
                .find(|(r, _)| *r == root)
                .map(|_| "it shares its package directory with a path that has to stay".to_string())
        };
        match own.or_else(shared) {
            Some(reason) => out.kept.push((s.clone(), reason)),
            None => {
                out.moves.push((s.clone(), to.clone()));
                *s = to.to_string_lossy().into_owned();
            }
        }
    };
    if let Some(c) = entry.command.as_mut() {
        map(c);
    }
    for a in entry.args.iter_mut() {
        map(a);
    }
    out
}

pub fn screen_entry(entry: &ServerEntry) -> Result<(), String> {
    match entry.command.as_deref() {
        Some(c) => crate::downstream::screen_spawn_command(c, &entry.args),
        None => Ok(()),
    }
}
