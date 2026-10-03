//! Git access behind a trait so taps and the skills-repo sync can be tested without a network:
//! `SystemGit` shells out to the `git` binary, `MockGit` records calls and replays canned results.

use std::cell::RefCell;
use std::path::Path;
use std::process::Command;

pub trait GitRunner {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String>;

    /// Fast-forward pull; returns the new HEAD commit.
    fn pull(&self, repo: &Path) -> Result<String, String>;

    fn head(&self, repo: &Path) -> Result<String, String>;

    fn is_repo(&self, path: &Path) -> bool {
        path.join(".git").exists()
    }
}

pub struct SystemGit;

impl SystemGit {
    fn run(&self, dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
        let mut cmd = Command::new("git");
        if let Some(dir) = dir {
            cmd.arg("-C").arg(dir);
        }
        let out = cmd
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or(""),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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

    fn pull(&self, repo: &Path) -> Result<String, String> {
        self.run(Some(repo), &["pull", "--quiet", "--ff-only"])?;
        self.head(repo)
    }

    fn head(&self, repo: &Path) -> Result<String, String> {
        self.run(Some(repo), &["rev-parse", "HEAD"])
    }
}

#[derive(Default)]
pub struct MockGit {
    pub calls: RefCell<Vec<String>>,
    pub fail_with: RefCell<Option<String>>,
    pub head_sha: RefCell<String>,
}

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

impl GitRunner for MockGit {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String> {
        self.record(format!("clone {url} {}", dest.display()))?;
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
}
