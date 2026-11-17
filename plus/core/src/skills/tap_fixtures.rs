//! Local git remotes for the tap tests: the trees under `tests/fixtures/skills-taps/repos`
//! become bare repositories, so no test reaches a network. `Rewrite` points the GitHub URLs a
//! `user/repo` tap expands to at those remotes.

use super::git::{GitRunner, SystemGit};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skills-taps")
}

pub(crate) fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub(crate) fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub(crate) struct Remotes {
    base: PathBuf,
}

impl Remotes {
    pub fn new(base: &Path) -> Self {
        Self {
            base: base.to_path_buf(),
        }
    }

    pub fn work(&self, owner_repo: &str) -> PathBuf {
        self.base.join("work").join(owner_repo)
    }

    pub fn remote(&self, owner_repo: &str) -> PathBuf {
        self.base.join("remotes").join(format!("{owner_repo}.git"))
    }

    pub fn url(&self, owner_repo: &str) -> String {
        format!("file://{}", self.remote(owner_repo).display())
    }

    /// Commits the fixture tree `repos/<owner>/<repo>` and publishes it as a bare repository.
    pub fn publish(&self, owner_repo: &str) {
        let work = self.work(owner_repo);
        copy_tree(&fixture_root().join("repos").join(owner_repo), &work);
        git_in(&work, &["init", "-q"]);
        git_in(&work, &["add", "-A"]);
        git_in(&work, &["commit", "-q", "-m", "init"]);
        let remote = self.remote(owner_repo);
        std::fs::create_dir_all(remote.parent().unwrap()).unwrap();
        git_in(
            &work,
            &["clone", "-q", "--bare", ".", &remote.to_string_lossy()],
        );
    }

    /// A new commit on the remote adding or replacing `rel`.
    pub fn commit_file(&self, owner_repo: &str, rel: &str, text: &str) {
        let work = self.work(owner_repo);
        let path = work.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        git_in(&work, &["add", "-A"]);
        git_in(&work, &["commit", "-q", "-m", "change"]);
        git_in(
            &work,
            &[
                "push",
                "-q",
                &self.remote(owner_repo).to_string_lossy(),
                "main",
            ],
        );
    }

    pub fn rewrite(&self) -> Rewrite {
        Rewrite {
            from: "https://github.com/".into(),
            to: format!("file://{}/", self.base.join("remotes").display()),
        }
    }
}

pub(crate) struct Rewrite {
    from: String,
    to: String,
}

impl Rewrite {
    fn map(&self, url: &str) -> String {
        match url.strip_prefix(&self.from) {
            Some(rest) => format!("{}{rest}", self.to),
            None => url.to_string(),
        }
    }
}

impl GitRunner for Rewrite {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String> {
        SystemGit.clone_repo(&self.map(url), dest)
    }

    fn clone_shallow(&self, url: &str, dest: &Path) -> Result<(), String> {
        SystemGit.clone_shallow(&self.map(url), dest)
    }

    fn pull(&self, repo: &Path) -> Result<String, String> {
        SystemGit.pull(repo)
    }

    fn head(&self, repo: &Path) -> Result<String, String> {
        SystemGit.head(repo)
    }
}
