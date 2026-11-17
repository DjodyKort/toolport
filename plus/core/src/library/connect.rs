//! `library connect|remote add|commit`: bring a repository in as the skills library, give a local
//! library a remote and commit what changed in it. None of these push; credentials stay with git
//! and gh, and a secret scan names a rule, a file and a line, never the matched text.

use super::remote::{git_out, is_git, local_state, locate, LocalState, SYNC_CONFIG};
use super::sync::{note_step, Checks};
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use crate::skills::gitexec::{git_command, run_command};
use crate::skills::taps::{redact, valid_source_url};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CLONE_TIMEOUT: Duration = Duration::from_secs(180);
const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const MESSAGE_LIMIT: usize = 500;
const LISTED: usize = 20;

fn refused(message: impl Into<String>) -> OpError {
    OpError::failed("refused", message)
}

fn url_of(raw: &str) -> Result<&str, OpError> {
    valid_source_url(raw).map_err(|why| OpError::usage(why.replacen("tap ", "repository ", 1)))?;
    Ok(raw)
}

fn expand(home: &Path, raw: &str) -> PathBuf {
    match raw.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if raw == "~" => home.to_path_buf(),
        None => PathBuf::from(raw),
    }
}

fn home() -> Result<PathBuf, OpError> {
    crate::fsx::home()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))
}

/// Where a library goes when `--clone-to` is not given: the folder every locator already checks.
fn default_target(home: &Path) -> PathBuf {
    home.join(".config/mcpm/skills_repo")
}

fn empty_or_missing(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => !dir.exists(),
    }
}

fn plan(summary: String, steps: Vec<Value>, warnings: Vec<String>, undo: String) -> Value {
    json!({"summary": summary, "steps": steps, "effects": {}, "warnings": warnings, "undo": undo})
}

fn exec_step(path: &Path, detail: impl Into<String>) -> Value {
    json!({"op": "exec", "path": path.display().to_string(), "detail": detail.into()})
}

fn config_path() -> Result<PathBuf, OpError> {
    crate::fsx::data_dir()
        .map(|dir| dir.join(SYNC_CONFIG))
        .ok_or_else(|| OpError::not_found("data directory unknown"))
}

fn remember(target: &Path) -> Result<(), OpError> {
    let path = config_path()?;
    let mut config: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    config["local_path"] = json!(target.display().to_string());
    let text = serde_json::to_string_pretty(&config).unwrap_or_default() + "\n";
    atomic_write(&path, &text, Mode::KeepOrDefault)
        .map_err(|e| OpError::failed("skills", format!("{}: {e}", path.display())))
}

fn classify_failure(stderr: &str) -> &'static str {
    let text = stderr.to_lowercase();
    if [
        "authentication",
        "could not read username",
        "permission denied",
        "403",
        "401",
    ]
    .iter()
    .any(|k| text.contains(k))
    {
        "authentication failed"
    } else if [
        "could not resolve",
        "unable to access",
        "connection",
        "timed out",
    ]
    .iter()
    .any(|k| text.contains(k))
    {
        "network unreachable"
    } else if [
        "not found",
        "does not exist",
        "does not appear to be a git repository",
    ]
    .iter()
    .any(|k| text.contains(k))
    {
        "repository not found"
    } else {
        "git could not clone the repository"
    }
}

/// Clones `url` into `--clone-to` (default `~/.config/mcpm/skills_repo`) and points the library
/// at the clone. Refuses while a library is already connected, a target that holds files and a
/// url that carries a credential.
pub fn connect(url: &str, clone_to: Option<&str>, dry_run: bool) -> Result<Value, OpError> {
    let url = url_of(url)?;
    let home = home()?;
    if let Ok(existing) = locate() {
        return Err(refused(format!(
            "a library is already connected at {}; connect a repository there only after moving it away",
            existing.display()
        )));
    }
    let target = clone_to
        .map(|raw| expand(&home, raw))
        .unwrap_or_else(|| default_target(&home));
    if !target.is_absolute() {
        return Err(OpError::usage("--clone-to must be an absolute folder"));
    }
    if !empty_or_missing(&target) {
        return Err(refused(format!(
            "{} is not empty; clone into a folder that does not exist yet",
            target.display()
        )));
    }
    let shown = redact(url);
    let undo = format!(
        "remove the folder {} and the local_path entry of {}",
        target.display(),
        config_path()?.display()
    );
    if dry_run {
        return Ok(json!({
            "dryRun": true,
            "repo": target.display().to_string(),
            "remote": shown,
            "plan": plan(
                format!("Clone {shown} into {}", target.display()),
                vec![
                    exec_step(&target, "git clone (network)"),
                    note_step(format!("point the library at {}", target.display())),
                ],
                Vec::new(),
                undo,
            ),
        }));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| OpError::failed("skills", format!("{}: {e}", parent.display())))?;
    }
    let target_arg = target.display().to_string();
    let out = run_command(
        git_command(None, &["clone", "--quiet", "--", url, &target_arg]),
        CLONE_TIMEOUT,
    )
    .map_err(|_| OpError::failed("network", "git did not run"))?;
    if !out.ok() {
        return Err(OpError::failed("network", classify_failure(&out.stderr)));
    }
    remember(&target)?;
    let state = local_state(&target);
    Ok(json!({
        "dryRun": false,
        "repo": target.display().to_string(),
        "remote": shown,
        "branch": state.branch,
        "connected": true,
        "empty": state.head.is_none(),
        "result": {"applied": true, "changed": [target.display().to_string()], "undo": undo, "backups": []},
    }))
}

/// Adds `origin` to the library when it has none. Nothing is fetched or pushed.
pub fn remote_add(repo: &Path, url: &str, dry_run: bool) -> Result<Value, OpError> {
    let url = url_of(url)?;
    if !is_git(repo) {
        return Err(OpError::not_found(format!(
            "{} is not a git repository",
            repo.display()
        )));
    }
    if let Some(existing) = local_state(repo).url {
        return Err(refused(format!(
            "the library already has a remote: {existing}"
        )));
    }
    let shown = redact(url);
    let undo = format!("git -C '{}' remote remove origin", repo.display());
    if dry_run {
        return Ok(json!({
            "dryRun": true,
            "repo": repo.display().to_string(),
            "remote": shown,
            "plan": plan(
                format!("Add {shown} as origin"),
                vec![exec_step(repo, "git remote add origin (nothing is fetched or pushed)")],
                Vec::new(),
                undo,
            ),
        }));
    }
    let out = run_command(
        git_command(Some(repo), &["remote", "add", "origin", "--", url]),
        GIT_TIMEOUT,
    )
    .map_err(|e| OpError::failed("git_failed", format!("git remote: {e}")))?;
    if !out.ok() {
        return Err(OpError::failed(
            "git_failed",
            format!("git remote add failed: {}", redact(&out.first_error_line())),
        ));
    }
    Ok(json!({
        "dryRun": false,
        "repo": repo.display().to_string(),
        "remote": shown,
        "branch": local_state(repo).branch,
        "result": {"applied": true, "changed": [], "undo": undo, "backups": []},
    }))
}

fn changed_files(repo: &Path) -> Vec<String> {
    git_out(repo, &["status", "--porcelain", "--untracked-files=all"])
        .unwrap_or_default()
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| l[3..].trim().trim_matches('"').to_string())
        .collect()
}

/// Commits everything that changed in the library with `message`, after the same secret scan a
/// push runs over the uncommitted work. A finding refuses the commit by rule, file and line.
pub fn commit(repo: &Path, message: &str, dry_run: bool) -> Result<Value, OpError> {
    if !is_git(repo) {
        return Err(OpError::not_found(format!(
            "{} is not a git repository",
            repo.display()
        )));
    }
    let message = message.trim();
    if message.is_empty() {
        return Err(OpError::usage("the commit needs a message"));
    }
    if message.chars().count() > MESSAGE_LIMIT || message.contains('\0') {
        return Err(OpError::usage(format!(
            "the commit message is longer than {MESSAGE_LIMIT} characters"
        )));
    }
    let state = local_state(repo);
    let files = changed_files(repo);
    if state.uncommitted == 0 {
        return Ok(json!({
            "dryRun": dry_run,
            "repo": repo.display().to_string(),
            "committed": false,
            "message": "working tree clean; nothing to commit",
            "files": files,
        }));
    }
    let scan = LocalState {
        ahead: 0,
        upstream: None,
        url: None,
        ..state.clone()
    };
    let checks = Checks::run(repo, &scan);
    if !dry_run && !checks.findings.is_empty() {
        return Err(refused(format!(
            "secret scan blocked the commit: {} (nothing was committed)",
            checks.describe()
        )));
    }
    let undo = format!(
        "git -C '{}' reset --soft HEAD~1",
        repo.display().to_string().replace('\'', "'\\''")
    );
    if dry_run {
        let mut warnings = Vec::new();
        if !checks.findings.is_empty() {
            warnings.push(format!(
                "the commit would be refused: {}",
                checks.describe()
            ));
        }
        let mut steps = vec![
            exec_step(repo, "git add -A"),
            exec_step(repo, format!("git commit -m \"{message}\"")),
        ];
        steps.extend(
            files
                .iter()
                .take(LISTED)
                .map(|f| note_step(format!("commit {f}"))),
        );
        return Ok(json!({
            "dryRun": true,
            "repo": repo.display().to_string(),
            "committed": false,
            "files": files,
            "checks": checks.value,
            "plan": plan(
                format!("Commit {} changed file(s)", files.len()),
                steps,
                warnings,
                undo,
            ),
        }));
    }
    super::sync::git_must(repo, &["add", "-A"])?;
    super::sync::git_must(repo, &["commit", "-q", "-m", message])?;
    let sha = git_out(repo, &["rev-parse", "HEAD"]).unwrap_or_default();
    Ok(json!({
        "dryRun": false,
        "repo": repo.display().to_string(),
        "committed": true,
        "commitSha": sha,
        "files": files,
        "checks": checks.value,
        "result": {
            "applied": true,
            "changed": files.iter().map(|f| repo.join(f).display().to_string()).collect::<Vec<_>>(),
            "undo": undo,
            "backups": [],
        },
    }))
}
