//! Which folders the repo-like detectors look at: the ODH root and the clients root from the
//! config (D-040), the `sourceRoots` of `context.json`, `--root` flags and the repo around `--cwd`.

use super::budget::Budget;
use super::fsx::{self, Kind};
use super::ScanOptions;
use crate::plus::context::{ContextConfig, Roots};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct RootSet {
    pub clients_root: PathBuf,
    /// Git checkouts the `repo` and `vendored` detectors read.
    pub repos: Vec<RepoRef>,
    /// Every configured folder, including the clients root: where `inert` looks for names.
    pub folders: Vec<PathBuf>,
    /// The folders that were configured, with where each came from.
    pub listing: Vec<RootRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoRef {
    pub path: PathBuf,
    /// Named as a root itself, so it is shown even without content; a child found under a
    /// container root is shown only when it has some.
    pub explicit: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootRow {
    pub path: PathBuf,
    pub origin: &'static str,
    pub exists: bool,
    pub repo: bool,
}

pub fn has_dot_git(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// A checkout that is not a linked worktree.
pub fn is_checkout(dir: &Path) -> bool {
    has_dot_git(dir) && super::gitx::open(dir, std::time::Duration::from_millis(250)).is_some()
}

/// The defaults of `sources root ls`: the repo that holds the clients root, and the clients root.
pub fn default_roots(roots: &Roots, config: &ContextConfig) -> Vec<PathBuf> {
    let clients = roots.resolve_clients_root(config);
    let mut out = Vec::new();
    if let Some(parent) = clients.parent().filter(|p| has_dot_git(p)) {
        out.push(parent.to_path_buf());
    }
    out.push(clients);
    out
}

pub fn config_roots(roots: &Roots, config: &ContextConfig) -> Vec<PathBuf> {
    config
        .source_roots
        .iter()
        .map(|raw| roots.expand_user(raw))
        .collect()
}

fn toplevel(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .take(24)
        .find(|d| has_dot_git(d))
        .map(Path::to_path_buf)
}

pub fn compute(
    roots: &Roots,
    config: &ContextConfig,
    opts: &ScanOptions,
    budget: &Budget,
) -> RootSet {
    let clients = roots.resolve_clients_root(config);
    let clients_canon = fsx::canonical(&clients);
    let mut inputs: Vec<(PathBuf, &'static str)> = default_roots(roots, config)
        .into_iter()
        .map(|p| (p, "default"))
        .collect();
    inputs.extend(
        config_roots(roots, config)
            .into_iter()
            .map(|p| (p, "config")),
    );
    inputs.extend(opts.roots.iter().cloned().map(|p| (p, "flag")));
    if let Some(top) = opts.cwd.as_deref().and_then(toplevel) {
        if !fsx::canonical(&top).starts_with(&clients_canon) {
            inputs.push((top, "cwd"));
        }
    }
    let mut set = RootSet {
        clients_root: clients,
        ..RootSet::default()
    };
    let mut seen: Vec<PathBuf> = Vec::new();
    for (path, origin) in inputs {
        let canon = fsx::canonical(&path);
        if seen.contains(&canon) {
            continue;
        }
        seen.push(canon.clone());
        let exists = fsx::is_dir(&path);
        let repo = exists && has_dot_git(&path);
        set.listing.push(RootRow {
            path: path.clone(),
            origin,
            exists,
            repo,
        });
        if !exists {
            continue;
        }
        set.folders.push(path.clone());
        if canon == clients_canon {
            continue;
        }
        if repo {
            if is_checkout(&path) {
                set.repos.push(RepoRef {
                    path,
                    explicit: true,
                });
            }
            continue;
        }
        for child in fsx::list_dir(&path) {
            if !budget.tick() {
                break;
            }
            if child.kind == Kind::Dir
                && !child.symlink
                && has_dot_git(&child.path)
                && is_checkout(&child.path)
            {
                set.repos.push(RepoRef {
                    path: child.path,
                    explicit: false,
                });
            }
        }
    }
    set
}
