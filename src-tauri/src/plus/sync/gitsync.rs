use super::bundle::SyncError;
use super::engine::SyncContext;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

const CONFIG_FILE: &str = "skills_sync.json";

#[derive(Debug, Default, Clone)]
pub struct GitSyncOptions {
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub auto: bool,
    pub status: bool,
    pub clear: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitSyncReport {
    pub configured: bool,
    pub cleared: bool,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub auto_sync: bool,
    pub local_path: Option<String>,
    pub cloned: bool,
    pub pulled: bool,
    pub head: Option<String>,
}

fn load(ctx: &SyncContext<'_>) -> Value {
    crate::plus::jsonfs::read_json(&ctx.config_dir.join(CONFIG_FILE)).unwrap_or(Value::Null)
}

fn bad(value: &str) -> bool {
    value.trim().is_empty() || value.starts_with('-') || value.contains(char::is_whitespace)
}

pub fn git_sync(ctx: &SyncContext<'_>, opts: &GitSyncOptions) -> Result<GitSyncReport, SyncError> {
    let path = ctx.config_dir.join(CONFIG_FILE);
    if opts.clear {
        if path.exists() {
            fs::remove_file(&path).map_err(|e| super::bundle::io(&path, e))?;
        }
        return Ok(GitSyncReport {
            cleared: true,
            ..GitSyncReport::default()
        });
    }
    if let Some(repo) = &opts.repo {
        let branch = opts.branch.clone().unwrap_or_else(|| "main".into());
        if bad(repo) || bad(&branch) {
            return Err(SyncError::Config(
                "refusing repository url or branch".into(),
            ));
        }
        let doc = json!({
            "repo": repo,
            "remote": repo,
            "branch": branch,
            "auto_sync": opts.auto,
            "local_path": ctx.skills_repo_path().to_string_lossy(),
        });
        fs::create_dir_all(&ctx.config_dir).map_err(|e| super::bundle::io(&ctx.config_dir, e))?;
        let text =
            serde_json::to_string_pretty(&doc).map_err(|e| SyncError::Format(e.to_string()))?;
        fs::write(&path, text).map_err(|e| super::bundle::io(&path, e))?;
    }
    let doc = load(ctx);
    let Some(repo) = doc.get("repo").and_then(Value::as_str).map(str::to_string) else {
        return Ok(GitSyncReport::default());
    };
    let branch = doc
        .get("branch")
        .and_then(Value::as_str)
        .unwrap_or("main")
        .to_string();
    let local = doc
        .get("local_path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.skills_repo_path());
    let mut report = GitSyncReport {
        configured: true,
        repo: Some(repo.clone()),
        branch: Some(branch.clone()),
        auto_sync: doc
            .get("auto_sync")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        local_path: Some(local.to_string_lossy().into_owned()),
        cloned: local.join(".git").exists(),
        ..GitSyncReport::default()
    };
    if opts.status {
        return Ok(report);
    }
    let dest = local.to_string_lossy().into_owned();
    if local.join(".git").exists() {
        report.pulled = ctx.exec.git(Some(&local), &["pull", "--ff-only"]).is_ok();
    } else if !local.exists() {
        if let Some(parent) = local.parent() {
            fs::create_dir_all(parent).map_err(|e| super::bundle::io(parent, e))?;
        }
        ctx.exec
            .git(
                None,
                &[
                    "clone", "--branch", &branch, "--depth", "1", "--", &repo, &dest,
                ],
            )
            .map_err(SyncError::Git)?;
        report.pulled = true;
    }
    report.cloned = local.join(".git").exists();
    report.head = ctx
        .exec
        .git(Some(&local), &["rev-parse", "HEAD"])
        .ok()
        .map(|h| h.trim().to_string());
    Ok(report)
}
