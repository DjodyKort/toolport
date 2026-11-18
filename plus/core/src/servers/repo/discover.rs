//! Which servers run from a git checkout on this machine: the working directory, the command or
//! an argument names a path inside a work tree. A server reached by URL, one that only runs a
//! program found on `PATH`, and anything installed into a package manager's cache is not one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use toolport_plus_api::ServerRecord;

use crate::skills::gitexec::git_stdout;

const GIT_TIMEOUT: Duration = Duration::from_secs(20);
const REMOTE_TRANSPORTS: [&str; 3] = ["http", "sse", "streamable-http"];
/// Programs that fetch a package and run it from a cache: only a working directory the server
/// names itself says anything about a checkout.
const PACKAGE_RUNNERS: [&str; 8] = [
    "npx", "uvx", "bunx", "pnpx", "pipx", "docker", "podman", "dlx",
];
const CACHE_PARTS: [&str; 11] = [
    "/node_modules/",
    "/_npx/",
    "/.npm/",
    "/.cache/",
    "/site-packages/",
    "/dist-packages/",
    "/.yarn/",
    "/.pnpm-store/",
    "/.bun/",
    "/.rustup/",
    "/Library/Caches/",
];
const CACHE_TAILS: [&str; 4] = [
    "/.cargo/registry/",
    "/.cargo/git/",
    "/.local/share/uv/",
    "/.local/share/pnpm/",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub server_id: String,
    pub server_name: String,
    pub root: PathBuf,
}

pub fn in_package_cache(path: &Path) -> bool {
    let text = format!("{}/", path.to_string_lossy().replace('\\', "/"));
    CACHE_PARTS
        .iter()
        .chain(CACHE_TAILS.iter())
        .any(|part| text.contains(part))
}

fn expand(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    if raw.contains("${") || raw.contains("$(") {
        return None;
    }
    if raw == "~" {
        return home.map(Path::to_path_buf);
    }
    match raw.strip_prefix("~/") {
        Some(rest) => home.map(|home| home.join(rest)),
        None => Some(PathBuf::from(raw)),
    }
}

fn resolved(raw: &str, cwd: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let raw = match raw.strip_prefix('-') {
        Some(flag) => flag.split_once('=')?.1,
        None => raw,
    };
    if raw.contains("://") || raw.starts_with('@') {
        return None;
    }
    let path = expand(raw, home)?;
    if path.is_absolute() {
        return Some(path);
    }
    let relative = raw.starts_with("./") || raw.starts_with("../") || raw.contains('/');
    match (relative, cwd) {
        (true, Some(cwd)) => Some(cwd.join(path)),
        _ => None,
    }
}

/// The paths a stdio server's configuration points at, working directory first. Nothing here
/// touches the disk.
pub fn candidates(server: &ServerRecord, home: Option<&Path>) -> Vec<PathBuf> {
    let transport = server.text("transport").unwrap_or("stdio");
    if REMOTE_TRANSPORTS.contains(&transport) {
        return Vec::new();
    }
    let Some(command) = server.text("command") else {
        return Vec::new();
    };
    let cwd = server
        .text("cwd")
        .and_then(|cwd| expand(cwd, home))
        .filter(|cwd| cwd.is_absolute());
    let mut out: Vec<PathBuf> = cwd.iter().cloned().collect();
    let program = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);
    if !PACKAGE_RUNNERS.contains(&program) {
        for raw in std::iter::once(command.to_string()).chain(server.strings("args")) {
            out.extend(resolved(&raw, cwd.as_deref(), home));
        }
    }
    out.retain(|path| !in_package_cache(path));
    out
}

fn nearest_directory(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn has_git_marker(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join(".git").exists())
}

/// The work tree root git reports for `path`, `None` outside one or where git refuses.
pub fn work_tree_of(path: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let dir = nearest_directory(path)?;
    if !has_git_marker(&dir) {
        return None;
    }
    let top = git_stdout(Some(&dir), &["rev-parse", "--show-toplevel"], GIT_TIMEOUT).ok()?;
    let root = std::fs::canonicalize(top.trim()).ok()?;
    let is_home = home.is_some_and(|home| std::fs::canonicalize(home).is_ok_and(|h| h == root));
    (root.parent().is_some() && !is_home && !in_package_cache(&root)).then_some(root)
}

/// The servers that run from a work tree, each with that tree's root. One server counts once,
/// for the first path that lands in a work tree.
pub fn discover(servers: &[ServerRecord], home: Option<&Path>) -> Vec<Found> {
    let mut known: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    let mut found = Vec::new();
    for server in servers {
        for path in candidates(server, home) {
            let root = known
                .entry(path.clone())
                .or_insert_with(|| work_tree_of(&path, home))
                .clone();
            if let Some(root) = root {
                found.push(Found {
                    server_id: server.id.clone(),
                    server_name: server.name.clone(),
                    root,
                });
                break;
            }
        }
    }
    found
}
