//! Where the sealed file travels. A transport moves one opaque file and names its revision; it
//! never reads it. Git keeps a working clone as a cache of the remote and uses the user's own
//! git credentials; a folder holds the file for whatever tool syncs that folder.

use std::path::{Path, PathBuf};

use super::config::{TransportCfg, FILE_NAME};
use crate::skills::git::GitRunner;
use crate::skills::taps::redact;

/// The remote holds a different revision than the caller expected.
pub const MOVED: &str = "the remote changed since you last pulled: pull first";

pub trait Transport {
    /// Copies the remote file to `dest` and returns its revision; `None` when the remote holds
    /// no file yet.
    fn fetch(&self, dest: &Path) -> Result<Option<String>, String>;

    /// Replaces the remote file with `sealed`, provided the remote is still at `expect` (`None`:
    /// still empty). Returns the new revision.
    fn publish(&self, sealed: &Path, expect: Option<&str>, message: &str)
        -> Result<String, String>;
}

pub struct Folder {
    pub dir: PathBuf,
}

fn digest(path: &Path) -> Result<String, String> {
    crate::setup::sha256_file(path).map_err(|e| format!("{}: {e}", path.display()))
}

impl Folder {
    fn file(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }
}

impl Transport for Folder {
    fn fetch(&self, dest: &Path) -> Result<Option<String>, String> {
        if !self.dir.is_dir() {
            return Err(format!("{} is not a folder", self.dir.display()));
        }
        let file = self.file();
        if !file.is_file() {
            return Ok(None);
        }
        std::fs::copy(&file, dest).map_err(|e| format!("{}: {e}", file.display()))?;
        digest(dest).map(Some)
    }

    fn publish(&self, sealed: &Path, expect: Option<&str>, _: &str) -> Result<String, String> {
        let here = self.file();
        let now = if here.is_file() {
            Some(digest(&here)?)
        } else {
            None
        };
        if now.as_deref() != expect {
            return Err(MOVED.into());
        }
        let partial = self.dir.join(format!("{FILE_NAME}.part"));
        std::fs::copy(sealed, &partial).map_err(|e| format!("{}: {e}", partial.display()))?;
        std::fs::rename(&partial, &here).map_err(|e| {
            let _ = std::fs::remove_file(&partial);
            format!("{}: {e}", here.display())
        })?;
        digest(&here)
    }
}

pub struct Git<'a> {
    pub url: String,
    pub branch: String,
    pub work: PathBuf,
    pub runner: &'a dyn GitRunner,
}

/// The host a git URL reaches, for the egress check; `None` for a path on this machine.
pub fn url_host(url: &str) -> Option<String> {
    let rest = match url.split_once("://") {
        Some(("file", _)) => return None,
        Some((_, rest)) => rest,
        None if url.contains('@') && url.contains(':') => url,
        None => return None,
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    Some(host.split(':').next().unwrap_or(host).to_string())
}

/// A git failure as one classified phrase; git's own text can carry the remote's address.
fn classified(error: String) -> String {
    let text = redact(&error);
    let kind = crate::library::remote::classify(&text);
    if error.contains("non-fast-forward") || error.contains("fetch first") {
        MOVED.to_string()
    } else {
        kind.to_string()
    }
}

impl Git<'_> {
    fn sync_tree(&self) -> Result<(), String> {
        crate::skills::taps::valid_source_url(&self.url).map_err(|e| {
            e.replace("tap url", "sync url")
                .replace("tap source", "sync source")
        })?;
        if let Some(host) = url_host(&self.url) {
            crate::egress::check(&host)?;
        }
        if !self.runner.is_repo(&self.work) {
            if let Some(parent) = self.work.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            self.runner
                .clone_repo(&self.url, &self.work)
                .map_err(classified)?;
        } else {
            self.runner.fetch(&self.work).map_err(classified)?;
        }
        self.runner
            .checkout_remote(&self.work, &self.branch)
            .map_err(classified)
    }

    fn revision(&self) -> Option<String> {
        self.runner
            .head(&self.work)
            .ok()
            .filter(|_| self.work.join(FILE_NAME).is_file())
    }
}

impl Transport for Git<'_> {
    fn fetch(&self, dest: &Path) -> Result<Option<String>, String> {
        self.sync_tree()?;
        let file = self.work.join(FILE_NAME);
        let Some(revision) = self.revision() else {
            return Ok(None);
        };
        std::fs::copy(&file, dest).map_err(|e| format!("{}: {e}", file.display()))?;
        Ok(Some(revision))
    }

    fn publish(
        &self,
        sealed: &Path,
        expect: Option<&str>,
        message: &str,
    ) -> Result<String, String> {
        self.sync_tree()?;
        if self.revision().as_deref() != expect {
            return Err(MOVED.into());
        }
        let file = self.work.join(FILE_NAME);
        std::fs::copy(sealed, &file).map_err(|e| format!("{}: {e}", file.display()))?;
        let committed = self
            .runner
            .commit_all(&self.work, message)
            .map_err(classified)?;
        if committed.is_some() {
            self.runner
                .push(&self.work, &self.branch)
                .map_err(classified)?;
        }
        self.runner.head(&self.work).map_err(classified)
    }
}

pub fn open<'a>(
    cfg: &TransportCfg,
    work: PathBuf,
    runner: &'a dyn GitRunner,
) -> Box<dyn Transport + 'a> {
    match cfg {
        TransportCfg::Git { url, branch } => Box::new(Git {
            url: url.clone(),
            branch: branch.clone(),
            work,
            runner,
        }),
        TransportCfg::Folder { path } => Box::new(Folder {
            dir: PathBuf::from(path),
        }),
    }
}
