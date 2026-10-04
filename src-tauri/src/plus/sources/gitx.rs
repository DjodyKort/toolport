//! Read-only git plumbing against a resolved git directory. Only `rev-parse`, `for-each-ref`,
//! `rev-list`, `ls-tree`, `cat-file` and `config --get` are ever run, always with
//! `--git-dir`, so no worktree is entered, no index is refreshed and nothing is fetched.

use super::fsx;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct GitDir {
    dir: PathBuf,
    timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub oid: String,
    pub size: u64,
}

/// The git directory of a normal checkout. A linked worktree (its `.git` file points into
/// `<repo>/.git/worktrees/`) is never a source of its own, so it yields `None`.
pub fn open(repo_root: &Path, timeout: Duration) -> Option<GitDir> {
    let dot = repo_root.join(".git");
    let dir = if dot.is_dir() {
        dot
    } else if dot.is_file() {
        let text = fsx::read_text(&dot, 4096)?;
        let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
        let target = Path::new(target);
        let abs = if target.is_absolute() {
            target.to_path_buf()
        } else {
            repo_root.join(target)
        };
        if abs.components().any(|c| c.as_os_str() == "worktrees") {
            return None;
        }
        abs
    } else {
        return None;
    };
    fsx::is_dir(&dir).then_some(GitDir {
        dir,
        timeout: timeout.max(Duration::from_millis(250)),
    })
}

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("--git-dir").arg(dir).args(args);
    cmd.env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");
    cmd
}

fn run(cmd: Command, input: Option<Vec<u8>>, timeout: Duration) -> Option<Vec<u8>> {
    let mut cmd = cmd;
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let writer = input.and_then(|bytes| {
        let mut stdin = child.stdin.take()?;
        Some(std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        }))
    });
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(1)),
            Err(_) => break None,
        }
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let out = reader.join().unwrap_or_default();
    status.filter(|s| s.success()).map(|_| out)
}

impl GitDir {
    pub fn path(&self) -> &Path {
        &self.dir
    }

    fn text(&self, args: &[&str]) -> Option<String> {
        let out = run(command(&self.dir, args), None, self.timeout)?;
        Some(String::from_utf8_lossy(&out).trim().to_string())
    }

    pub fn head(&self) -> Option<String> {
        self.text(&["rev-parse", "--verify", "-q", "HEAD"])
            .filter(|s| !s.is_empty())
    }

    /// The remote default branch as the clone last saw it: `refs/remotes/origin/HEAD`, else
    /// `origin/main`, else `origin/master`.
    pub fn remote_default(&self) -> Option<Remote> {
        let listing = self.text(&[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)%09%(symref)",
            "refs/remotes/origin",
        ])?;
        let refs: Vec<(String, String, String)> = listing
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                Some((
                    parts.next()?.to_string(),
                    parts.next()?.to_string(),
                    parts.next().unwrap_or("").to_string(),
                ))
            })
            .collect();
        let named = |full: &str| {
            refs.iter()
                .find(|(name, _, _)| name == full)
                .map(|(name, sha, _)| Remote {
                    name: name.trim_start_matches("refs/remotes/").to_string(),
                    sha: sha.clone(),
                })
        };
        if let Some((_, sha, symref)) = refs
            .iter()
            .find(|(n, _, _)| n == "refs/remotes/origin/HEAD")
        {
            if let Some(target) = symref.strip_prefix("refs/remotes/") {
                return Some(Remote {
                    name: target.to_string(),
                    sha: sha.clone(),
                });
            }
        }
        named("refs/remotes/origin/main").or_else(|| named("refs/remotes/origin/master"))
    }

    /// `(ahead, behind)` of HEAD against `remote_sha`.
    pub fn counts(&self, remote_sha: &str) -> Option<(u64, u64)> {
        let range = format!("HEAD...{remote_sha}");
        let text = self.text(&["rev-list", "--left-right", "--count", &range])?;
        let mut parts = text.split_whitespace();
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }

    pub fn tree(&self, commit: &str, specs: &[&str]) -> Vec<TreeEntry> {
        let mut args = vec!["ls-tree", "-r", "-l", "-z", "--full-tree", commit, "--"];
        args.extend_from_slice(specs);
        let Some(out) = run(command(&self.dir, &args), None, self.timeout) else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out)
            .split('\0')
            .filter_map(|record| {
                let (meta, path) = record.split_once('\t')?;
                let mut fields = meta.split_whitespace();
                let _mode = fields.next()?;
                if fields.next()? != "blob" {
                    return None;
                }
                Some(TreeEntry {
                    path: path.to_string(),
                    oid: fields.next()?.to_string(),
                    size: fields.next()?.parse().ok()?,
                })
            })
            .collect()
    }

    pub fn blobs(&self, oids: &[String]) -> HashMap<String, Vec<u8>> {
        let mut found = HashMap::new();
        if oids.is_empty() {
            return found;
        }
        let input: Vec<u8> = oids
            .iter()
            .flat_map(|o| format!("{o}\n").into_bytes())
            .collect();
        let Some(out) = run(
            command(&self.dir, &["cat-file", "--batch"]),
            Some(input),
            self.timeout,
        ) else {
            return found;
        };
        let mut at = 0;
        while at < out.len() {
            let Some(nl) = out[at..].iter().position(|b| *b == b'\n') else {
                break;
            };
            let header = String::from_utf8_lossy(&out[at..at + nl]).into_owned();
            at += nl + 1;
            let mut fields = header.split_whitespace();
            let (Some(oid), Some(kind)) = (fields.next(), fields.next()) else {
                continue;
            };
            if kind == "missing" {
                continue;
            }
            let Some(size) = fields.next().and_then(|s| s.parse::<usize>().ok()) else {
                break;
            };
            if at + size > out.len() {
                break;
            }
            found.insert(oid.to_string(), out[at..at + size].to_vec());
            at += size + 1;
        }
        found
    }

    pub fn remote_url(&self) -> Option<String> {
        self.text(&["config", "--get", "remote.origin.url"])
            .filter(|s| !s.is_empty())
            .map(|url| crate::plus::skills::taps::redact(&url))
    }

    pub fn last_fetch_secs(&self) -> Option<i64> {
        fsx::mtime_secs(&self.dir.join("FETCH_HEAD"))
    }
}

/// The same remote spelled as `git@host:owner/repo.git`, `https://host/owner/repo` or with a
/// trailing slash compares equal.
pub fn normalize_url(url: &str) -> String {
    let mut text = url.trim().to_lowercase();
    if let Some((_, rest)) = text.split_once("://") {
        text = rest.to_string();
        if let Some((_, after)) = text.split_once('@') {
            text = after.to_string();
        }
    } else if let Some((_, rest)) = text.split_once('@') {
        text = rest.replacen(':', "/", 1);
    }
    text.trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_spellings_normalize_to_one_form() {
        let forms = [
            "git@github.com:Owner/Skills.git",
            "https://github.com/owner/skills",
            "https://user:secret@github.com/owner/skills.git/",
            "ssh://git@github.com/owner/skills.git",
        ];
        for form in forms {
            assert_eq!(normalize_url(form), "github.com/owner/skills", "{form}");
        }
    }

    #[test]
    fn a_linked_worktree_is_not_opened() {
        let dir = std::env::temp_dir().join(format!("sources-gitx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("main/.git/worktrees/w1")).unwrap();
        std::fs::create_dir_all(dir.join("w1")).unwrap();
        let pointer = dir.join("main/.git/worktrees/w1");
        std::fs::write(
            dir.join("w1/.git"),
            format!("gitdir: {}\n", pointer.display()),
        )
        .unwrap();
        assert!(open(&dir.join("w1"), Duration::from_secs(1)).is_none());
        assert!(open(&dir.join("main"), Duration::from_secs(1)).is_some());
        assert!(open(&dir.join("absent"), Duration::from_secs(1)).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
