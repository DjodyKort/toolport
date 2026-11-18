//! Where a folder's git repository keeps its files, read from the files themselves: `.git` is a
//! folder in a normal checkout, a file pointing at the real folder in a linked worktree or a
//! submodule. Claude Code keys auto memory by the repository, so every worktree and subfolder
//! resolves to the main checkout.

use super::fsx;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The folder that holds `.git`.
    pub work_root: PathBuf,
    pub git_dir: PathBuf,
    /// Shared by every worktree of the repository (`info/exclude` lives here).
    pub common_dir: PathBuf,
    pub linked: bool,
}

impl Layout {
    pub fn exclude_file(&self) -> PathBuf {
        self.common_dir.join("info").join("exclude")
    }

    /// The main checkout: where Claude Code keeps the local settings file and derives the
    /// project key from. A bare repository or a submodule is its own root.
    pub fn main_root(&self) -> PathBuf {
        if self.linked && self.common_dir.file_name().is_some_and(|n| n == ".git") {
            if let Some(parent) = self.common_dir.parent() {
                return parent.to_path_buf();
            }
        }
        self.work_root.clone()
    }

    /// The main checkout and every worktree git has registered, whether or not it still exists.
    pub fn worktrees(&self) -> Vec<PathBuf> {
        let main = self.main_root();
        let mut out = vec![main.clone()];
        let mut linked: Vec<PathBuf> = fsx::list_dir(&self.common_dir.join("worktrees"))
            .into_iter()
            .filter(|e| e.kind == fsx::Kind::Dir)
            .filter_map(|e| {
                let text = fsx::read_text(&e.path.join("gitdir"), 4096)?;
                let dotgit = PathBuf::from(text.trim());
                Some(fsx::canonical(dotgit.parent()?))
            })
            .filter(|p| *p != main)
            .collect();
        linked.sort();
        linked.dedup();
        out.extend(linked);
        out
    }
}

fn resolve(base: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw.trim());
    fsx::canonical(&if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

/// The layout when `.git` sits directly in `dir`.
pub fn at(dir: &Path) -> Option<Layout> {
    let work_root = fsx::canonical(dir);
    let dot = work_root.join(".git");
    if dot.is_dir() {
        return Some(Layout {
            work_root,
            git_dir: dot.clone(),
            common_dir: dot,
            linked: false,
        });
    }
    let text = fsx::read_text(&dot, 4096)?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?;
    let git_dir = resolve(&work_root, target);
    let common = fsx::read_text(&git_dir.join("commondir"), 4096)
        .map(|raw| resolve(&git_dir, raw.lines().next().unwrap_or("")));
    Some(Layout {
        work_root,
        linked: common.is_some(),
        common_dir: common.unwrap_or_else(|| git_dir.clone()),
        git_dir,
    })
}

/// The layout of the nearest folder at or above `start` that holds `.git`.
pub fn locate(start: &Path) -> Option<Layout> {
    fsx::canonical(start).ancestors().find_map(at)
}

/// Where Claude Code reads and writes the local settings file for a session in `cwd`.
pub fn settings_root(cwd: &Path) -> PathBuf {
    locate(cwd).map_or_else(|| fsx::canonical(cwd), |l| l.main_root())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn linked_worktree(base: &Path) -> (PathBuf, PathBuf) {
        let main = base.join("main");
        fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        let wt = base.join("wt");
        fs::create_dir_all(&wt).unwrap();
        let git_dir = main.join(".git/worktrees/wt");
        fs::write(wt.join(".git"), format!("gitdir: {}\n", git_dir.display())).unwrap();
        fs::write(git_dir.join("commondir"), "../..\n").unwrap();
        fs::write(
            git_dir.join("gitdir"),
            format!("{}\n", wt.join(".git").display()),
        )
        .unwrap();
        (fsx::canonical(&main), fsx::canonical(&wt))
    }

    #[test]
    fn a_linked_worktree_resolves_to_the_main_checkout_and_its_shared_exclude() {
        let dir = tempfile::tempdir().unwrap();
        let (main, wt) = linked_worktree(&fsx::canonical(dir.path()));
        let layout = locate(&wt).unwrap();
        assert!(layout.linked);
        assert_eq!(layout.work_root, wt);
        assert_eq!(layout.main_root(), main);
        assert_eq!(layout.exclude_file(), main.join(".git/info/exclude"));
        assert_eq!(layout.worktrees(), [main, wt]);
    }

    #[test]
    fn a_subfolder_resolves_to_the_repository_root_and_a_plain_folder_to_itself() {
        let dir = tempfile::tempdir().unwrap();
        let base = fsx::canonical(dir.path());
        fs::create_dir_all(base.join("repo/.git")).unwrap();
        fs::create_dir_all(base.join("repo/src/deep")).unwrap();
        fs::create_dir_all(base.join("plain")).unwrap();
        assert_eq!(
            settings_root(&base.join("repo/src/deep")),
            base.join("repo")
        );
        assert_eq!(settings_root(&base.join("plain")), base.join("plain"));
    }

    #[test]
    fn a_submodule_is_its_own_root() {
        let dir = tempfile::tempdir().unwrap();
        let base = fsx::canonical(dir.path());
        fs::create_dir_all(base.join("super/.git/modules/sub")).unwrap();
        fs::create_dir_all(base.join("super/sub")).unwrap();
        fs::write(base.join("super/sub/.git"), "gitdir: ../.git/modules/sub\n").unwrap();
        let layout = locate(&base.join("super/sub")).unwrap();
        assert!(!layout.linked);
        assert_eq!(layout.main_root(), base.join("super/sub"));
        assert_eq!(layout.common_dir, base.join("super/.git/modules/sub"));
    }
}
