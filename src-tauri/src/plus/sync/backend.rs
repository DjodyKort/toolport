use super::bundle::{read_manifest, Manifest, SyncError, MANIFEST_FILE, SALT_FILE};
use super::exec::Exec;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::fs;
use std::path::{Path, PathBuf};

pub struct GitBackend<'a> {
    pub exec: &'a dyn Exec,
    pub repo_url: String,
    pub local: PathBuf,
    pub branch: String,
}

fn git_err(e: String) -> SyncError {
    SyncError::Git(e)
}

fn check_ref(value: &str, what: &str) -> Result<(), SyncError> {
    if value.trim().is_empty() || value.starts_with('-') || value.contains(char::is_whitespace) {
        return Err(SyncError::Config(format!("refusing {what} {value:?}")));
    }
    Ok(())
}

impl<'a> GitBackend<'a> {
    pub fn new(exec: &'a dyn Exec, repo_url: &str, local: &Path, branch: &str) -> Self {
        Self {
            exec,
            repo_url: repo_url.to_string(),
            local: local.to_path_buf(),
            branch: branch.to_string(),
        }
    }

    fn git(&self, args: &[&str]) -> Result<String, SyncError> {
        self.exec.git(Some(&self.local), args).map_err(git_err)
    }

    pub fn is_cloned(&self) -> bool {
        self.local.join(".git").exists()
    }

    pub fn init(&self) -> Result<(), SyncError> {
        check_ref(&self.repo_url, "repository url")?;
        check_ref(&self.branch, "branch")?;
        if self.is_cloned() {
            self.pull();
            return Ok(());
        }
        let dest = self.local.to_string_lossy().into_owned();
        if let Some(parent) = self.local.parent() {
            fs::create_dir_all(parent).map_err(|e| super::bundle::io(parent, e))?;
        }
        let branched = self.exec.git(
            None,
            &[
                "clone",
                "--branch",
                &self.branch,
                "--",
                &self.repo_url,
                &dest,
            ],
        );
        if branched.is_ok() {
            return Ok(());
        }
        let _ = fs::remove_dir_all(&self.local);
        self.exec
            .git(None, &["clone", "--", &self.repo_url, &dest])
            .map_err(git_err)?;
        self.git(&["checkout", "-B", &self.branch])?;
        Ok(())
    }

    pub fn pull(&self) -> bool {
        if self.git(&["pull", "--ff-only"]).is_ok() {
            return true;
        }
        self.git(&["pull", "--rebase"]).is_ok()
    }

    pub fn head(&self) -> Option<String> {
        self.git(&["rev-parse", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
    }

    pub fn manifest(&self) -> Option<Manifest> {
        read_manifest(&self.local).ok()
    }

    pub fn fetch_manifest(&self) -> Option<Manifest> {
        let _ = self.git(&["fetch", "origin", &self.branch]);
        let spec = format!("origin/{}:{MANIFEST_FILE}", self.branch);
        let text = self.git(&["show", &spec]).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn load_salt(&self) -> Option<Vec<u8>> {
        let text = fs::read_to_string(self.local.join(SALT_FILE)).ok()?;
        STANDARD.decode(text.trim()).ok()
    }

    pub fn commit_and_push(&self, message: &str) -> Result<bool, SyncError> {
        self.git(&["add", "."])?;
        if self.git(&["status", "--porcelain"])?.trim().is_empty() {
            return Ok(false);
        }
        let identity = self
            .git(&["config", "user.email"])
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        let mut args = Vec::new();
        if !identity {
            args.extend([
                "-c",
                "user.name=toolport-sync",
                "-c",
                "user.email=toolport-sync@localhost",
            ]);
        }
        args.extend(["commit", "--quiet", "-m", message]);
        self.git(&args)?;
        self.git(&["push", "origin", &self.branch])?;
        Ok(true)
    }

    pub fn discard_to(&self, sha: &str) {
        let _ = self.git(&["reset", "--hard", sha]);
        let _ = self.git(&["clean", "-fdq"]);
    }
}
