//! Git access behind a trait so taps can be tested without a network: `SystemGit` shells out to
//! the `git` binary, and tests use `MockGit`, which records calls and replays canned results.

use super::gitexec::git_stdout;
#[cfg(test)]
use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(300);

pub trait GitRunner {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String>;

    /// A `--depth 1` clone, which is all a tap needs; the default falls back to a full clone.
    fn clone_shallow(&self, url: &str, dest: &Path) -> Result<(), String> {
        self.clone_repo(url, dest)
    }

    /// Fast-forward pull; returns the new HEAD commit.
    fn pull(&self, repo: &Path) -> Result<String, String>;

    fn head(&self, repo: &Path) -> Result<String, String>;

    fn is_repo(&self, path: &Path) -> bool {
        path.join(".git").exists()
    }

    fn fetch(&self, _repo: &Path) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    /// Puts the working tree on `branch`: the remote's copy when it has one, else a new branch.
    /// Local changes of the tree are dropped; it is a cache of the remote.
    fn checkout_remote(&self, _repo: &Path, _branch: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    /// Commits everything in the working tree; `None` when there was nothing to commit.
    fn commit_all(&self, _repo: &Path, _message: &str) -> Result<Option<String>, String> {
        Err(UNSUPPORTED.into())
    }

    /// A plain push of HEAD to `branch`, never forced.
    fn push(&self, _repo: &Path, _branch: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
}

const UNSUPPORTED: &str = "this git runner cannot do that";
const COMMITTER: [&str; 6] = [
    "-c",
    "user.name=Toolport",
    "-c",
    "commit.gpgsign=false",
    "-c",
    "user.email=toolport@localhost.invalid",
];

pub struct SystemGit;

impl SystemGit {
    fn run(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
        git_stdout(dir, args, GIT_TIMEOUT).map(|out| out.trim().to_string())
    }
}

/// A URL that starts with `-` would be parsed by git as an option.
fn safe_url(url: &str) -> Result<(), String> {
    if url.trim().is_empty() || url.starts_with('-') {
        return Err(format!("refusing git url {url:?}"));
    }
    Ok(())
}

impl GitRunner for SystemGit {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String> {
        safe_url(url)?;
        let dest = dest.to_string_lossy();
        self.run(None, &["clone", "--quiet", "--", url, &dest])
            .map(|_| ())
    }

    fn clone_shallow(&self, url: &str, dest: &Path) -> Result<(), String> {
        safe_url(url)?;
        let dest = dest.to_string_lossy();
        self.run(
            None,
            &["clone", "--quiet", "--depth", "1", "--", url, &dest],
        )
        .map(|_| ())
    }

    fn pull(&self, repo: &Path) -> Result<String, String> {
        self.run(Some(repo), &["pull", "--quiet", "--ff-only"])?;
        self.head(repo)
    }

    fn head(&self, repo: &Path) -> Result<String, String> {
        self.run(Some(repo), &["rev-parse", "HEAD"])
    }

    fn fetch(&self, repo: &Path) -> Result<(), String> {
        self.run(Some(repo), &["fetch", "--quiet", "origin"])
            .map(|_| ())
    }

    fn checkout_remote(&self, repo: &Path, branch: &str) -> Result<(), String> {
        safe_branch(branch)?;
        let remote = format!("refs/remotes/origin/{branch}");
        if self
            .run(Some(repo), &["rev-parse", "--verify", "-q", &remote])
            .is_ok()
        {
            let start = format!("origin/{branch}");
            self.run(
                Some(repo),
                &["checkout", "--quiet", "--force", "-B", branch, &start],
            )
            .map(|_| ())
        } else {
            let head = format!("refs/heads/{branch}");
            self.run(Some(repo), &["symbolic-ref", "HEAD", &head])
                .map(|_| ())
        }
    }

    fn commit_all(&self, repo: &Path, message: &str) -> Result<Option<String>, String> {
        self.run(Some(repo), &["add", "-A"])?;
        if self.run(Some(repo), &["status", "--porcelain"])?.is_empty() {
            return Ok(None);
        }
        let mut args: Vec<&str> = COMMITTER.to_vec();
        args.extend(["commit", "--quiet", "-m", message]);
        self.run(Some(repo), &args)?;
        self.head(repo).map(Some)
    }

    fn push(&self, repo: &Path, branch: &str) -> Result<(), String> {
        safe_branch(branch)?;
        let spec = format!("HEAD:refs/heads/{branch}");
        self.run(Some(repo), &["push", "--quiet", "origin", &spec])
            .map(|_| ())
    }
}

fn safe_branch(branch: &str) -> Result<(), String> {
    let plain = !branch.is_empty()
        && !branch.starts_with(['-', '/', '.'])
        && !branch.contains("..")
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'));
    if plain {
        Ok(())
    } else {
        Err(format!("refusing branch name {branch:?}"))
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MockGit {
    pub calls: RefCell<Vec<String>>,
    pub fail_with: RefCell<Option<String>>,
    pub head_sha: RefCell<String>,
}

#[cfg(test)]
impl MockGit {
    pub fn with_head(sha: &str) -> Self {
        Self {
            head_sha: RefCell::new(sha.to_string()),
            ..Self::default()
        }
    }

    fn record(&self, call: String) -> Result<(), String> {
        self.calls.borrow_mut().push(call);
        match self.fail_with.borrow().clone() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
impl GitRunner for MockGit {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String> {
        self.record(format!("clone {url} {}", dest.display()))?;
        std::fs::create_dir_all(dest.join(".git")).map_err(|e| e.to_string())
    }

    fn clone_shallow(&self, url: &str, dest: &Path) -> Result<(), String> {
        self.record(format!("clone --depth 1 {url} {}", dest.display()))?;
        std::fs::create_dir_all(dest.join(".git")).map_err(|e| e.to_string())
    }

    fn pull(&self, repo: &Path) -> Result<String, String> {
        self.record(format!("pull {}", repo.display()))?;
        Ok(self.head_sha.borrow().clone())
    }

    fn head(&self, repo: &Path) -> Result<String, String> {
        self.record(format!("head {}", repo.display()))?;
        Ok(self.head_sha.borrow().clone())
    }

    fn fetch(&self, repo: &Path) -> Result<(), String> {
        self.record(format!("fetch {}", repo.display()))
    }

    fn checkout_remote(&self, repo: &Path, branch: &str) -> Result<(), String> {
        self.record(format!("checkout {branch} {}", repo.display()))
    }

    fn commit_all(&self, repo: &Path, message: &str) -> Result<Option<String>, String> {
        self.record(format!("commit {message} {}", repo.display()))?;
        Ok(Some(self.head_sha.borrow().clone()))
    }

    fn push(&self, repo: &Path, branch: &str) -> Result<(), String> {
        self.record(format!("push {branch} {}", repo.display()))
    }
}
