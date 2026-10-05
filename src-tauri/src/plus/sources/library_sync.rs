//! `library pull|push`: the one core behind `toolportctl library pull|push` and the self-MCP tools
//! `library_pull` and `skills_git_push`. Pull is fast-forward only and refuses over uncommitted
//! changes; push audits the skills, scans what would leave the machine for secrets and never
//! forces. Credentials stay with git and gh: nothing here reads, stores or prints one, and a
//! finding names a rule, a file and a line, never the matched text.

use super::library_remote::{fetch, git_out, is_git, local_state, LocalState};
use crate::plus::exec::{git_command, is_not_found, run_command, CmdOutput};
use crate::plus::op::OpError;
use crate::plus::plan::{Effects, PlanV1, ResultV1, Step};
use crate::plus::skills::repo::audit_repo;
use crate::plus::skills::taps::redact;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

pub const DEFAULT_MESSAGE: &str = "Update skills library";
const GIT_TIMEOUT: Duration = Duration::from_secs(120);
const SCAN_FILE_LIMIT: u64 = 1024 * 1024;
const LISTED: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub rule: String,
    pub file: String,
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

const RULES: &[(&str, &str)] = &[
    ("private-key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
    ("github-token", r"\bgh[pousr]_[A-Za-z0-9]{36,}"),
    ("github-pat", r"\bgithub_pat_[A-Za-z0-9_]{22,}"),
    ("aws-access-key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
    ("slack-token", r"\bxox[abprs]-[A-Za-z0-9-]{10,}"),
    ("anthropic-key", r"\bsk-ant-[A-Za-z0-9_-]{20,}"),
    ("google-api-key", r"\bAIza[0-9A-Za-z_-]{35}"),
];

fn rules() -> &'static [(&'static str, Regex)] {
    static COMPILED: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        RULES
            .iter()
            .map(|(name, pattern)| (*name, Regex::new(pattern).expect("secret rule")))
            .collect()
    })
}

fn scan_line(text: &str) -> Vec<&'static str> {
    rules()
        .iter()
        .filter(|(_, re)| re.is_match(text))
        .map(|(name, _)| *name)
        .collect()
}

/// Added lines of a `git diff` or `git log -p` patch made with `-U0`; `commit:<sha>` lines come
/// from `--format=commit:%h`.
fn scan_patch(patch: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let (mut file, mut commit, mut line) = (String::new(), None::<String>, 0u64);
    for text in patch.lines() {
        if let Some(sha) = text.strip_prefix("commit:") {
            commit = Some(sha.trim().to_string());
        } else if let Some(name) = text.strip_prefix("+++ b/") {
            file = name.to_string();
        } else if text.starts_with("+++ ") || text.starts_with("--- ") || text.starts_with("diff ")
        {
        } else if text.starts_with("@@") {
            line = text
                .split('+')
                .nth(1)
                .and_then(|rest| rest.split([',', ' ']).next())
                .and_then(|n| n.parse::<u64>().ok())
                .map_or(0, |n| n.saturating_sub(1));
        } else if let Some(added) = text.strip_prefix('+') {
            line += 1;
            for rule in scan_line(added) {
                out.push(Finding {
                    rule: rule.to_string(),
                    file: file.clone(),
                    line,
                    commit: commit.clone(),
                });
            }
        }
    }
    out
}

fn git_run(repo: &Path, args: &[&str]) -> Result<CmdOutput, OpError> {
    run_command(git_command(Some(repo), args), GIT_TIMEOUT)
        .map_err(|e| OpError::failed("git_failed", format!("git {}: {e}", args.first().unwrap_or(&""))))
}

fn git_must(repo: &Path, args: &[&str]) -> Result<String, OpError> {
    let out = git_run(repo, args)?;
    if out.ok() {
        Ok(out.stdout)
    } else {
        Err(OpError::failed(
            "git_failed",
            format!(
                "git {} failed: {}",
                args.first().unwrap_or(&""),
                redact(&out.first_error_line())
            ),
        ))
    }
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn commit_list(repo: &Path, range: &str) -> Vec<Value> {
    git_out(repo, &["log", "--format=%h%x09%s", range])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(sha, subject)| json!({"sha": sha, "subject": subject}))
        .collect()
}

fn note(detail: impl Into<String>) -> Step {
    Step {
        op: "note",
        path: None,
        detail: detail.into(),
        keys: None,
        diff: None,
    }
}

fn exec(repo: &Path, detail: impl Into<String>) -> Step {
    Step {
        op: "exec",
        path: Some(repo.display().to_string()),
        ..note(detail)
    }
}

fn applied(changed: Vec<String>, undo: String) -> Value {
    serde_json::to_value(ResultV1 {
        applied: true,
        changed,
        undo,
        backups: Vec::new(),
    })
    .unwrap_or(Value::Null)
}

fn plan_value(plan: PlanV1) -> Value {
    serde_json::to_value(plan).unwrap_or(Value::Null)
}

fn require_repo(repo: &Path) -> Result<(), OpError> {
    if is_git(repo) {
        Ok(())
    } else {
        Err(OpError::not_found(format!(
            "{} is not a git repository",
            repo.display()
        )))
    }
}

pub fn pull(repo: &Path, dry_run: bool) -> Result<Value, OpError> {
    require_repo(repo)?;
    let before = local_state(repo);
    if before.uncommitted > 0 {
        return Err(OpError::failed(
            "refused",
            format!(
                "{} uncommitted change(s) in {}; commit or stash them first",
                before.uncommitted,
                repo.display()
            ),
        ));
    }
    let upstream = before
        .upstream
        .clone()
        .ok_or_else(|| OpError::failed("refused", "no upstream branch to pull from"))?;
    let old = before.head.clone().unwrap_or_default();
    let undo = format!("git -C {} reset --hard {old}", quote(repo));
    let fetched = fetch(repo);
    let before = if fetched.is_ok() { local_state(repo) } else { before };
    if dry_run {
        let range = format!("HEAD..{upstream}");
        let mut steps = vec![
            exec(repo, "git fetch origin (refs only, network)"),
            exec(repo, format!("git merge --ff-only {upstream}")),
        ];
        let commits = commit_list(repo, &range);
        steps.extend(commits.iter().take(LISTED).map(|c| {
            note(format!(
                "{} {}",
                c["sha"].as_str().unwrap_or(""),
                c["subject"].as_str().unwrap_or("")
            ))
        }));
        let mut warnings = Vec::new();
        if let Err(reason) = &fetched {
            warnings.push(format!(
                "could not fetch ({reason}); the counts come from the last fetch"
            ));
        }
        if before.ahead > 0 && before.behind > 0 {
            warnings.push(format!(
                "diverged: {} local and {} remote commit(s); a fast-forward is not possible",
                before.ahead, before.behind
            ));
        }
        let summary = if before.behind == 0 {
            format!("Already up to date with {upstream}")
        } else {
            format!("Fast-forward {} commit(s) from {upstream}", before.behind)
        };
        return Ok(json!({
            "dryRun": true,
            "repo": repo.display().to_string(),
            "plan": plan_value(PlanV1 {
                summary,
                steps,
                effects: Effects::default(),
                warnings,
                undo,
            }),
        }));
    }
    fetched.map_err(|e| OpError::failed("network", format!("git fetch failed: {e}")))?;
    let after = local_state(repo);
    if after.ahead > 0 && after.behind > 0 {
        return Err(OpError::failed(
            "refused",
            format!(
                "diverged: {} local and {} remote commit(s); a fast-forward is not possible",
                after.ahead, after.behind
            ),
        ));
    }
    if after.behind == 0 {
        return Ok(json!({
            "dryRun": false,
            "repo": repo.display().to_string(),
            "pulled": false,
            "result": applied(Vec::new(), String::new()),
        }));
    }
    git_must(repo, &["merge", "--ff-only", &upstream])?;
    let new = git_out(repo, &["rev-parse", "HEAD"]).unwrap_or_default();
    let changed = git_out(repo, &["diff", "--name-only", &format!("{old}..{new}")])
        .unwrap_or_default()
        .lines()
        .map(|f| repo.join(f).display().to_string())
        .collect();
    Ok(json!({
        "dryRun": false,
        "repo": repo.display().to_string(),
        "pulled": true,
        "commits": after.behind,
        "result": applied(changed, undo),
    }))
}

static REPORTS: AtomicUsize = AtomicUsize::new(0);

enum Mode<'a> {
    Worktree,
    Commits(&'a str),
}

fn gitleaks(repo: &Path, mode: Mode) -> Result<Vec<Finding>, String> {
    let bin = std::env::var("TOOLPORT_GITLEAKS_BIN").unwrap_or_else(|_| "gitleaks".into());
    let report = std::env::temp_dir().join(format!(
        "toolport-gitleaks-{}-{}.json",
        std::process::id(),
        REPORTS.fetch_add(1, Ordering::Relaxed)
    ));
    let mut cmd = Command::new(&bin);
    cmd.args(["detect", "--no-banner", "--redact", "--exit-code", "2"])
        .args(["--report-format", "json", "--report-path"])
        .arg(&report)
        .arg("--source")
        .arg(repo);
    match mode {
        Mode::Worktree => cmd.arg("--no-git"),
        Mode::Commits(range) => cmd.arg("--log-opts").arg(range),
    };
    let result = run_command(cmd, GIT_TIMEOUT);
    let text = std::fs::read_to_string(&report).unwrap_or_default();
    let _ = std::fs::remove_file(&report);
    match result {
        Err(e) if is_not_found(&e) => Err("not installed".into()),
        Err(_) => Err("did not run".into()),
        Ok(out) if out.code == 0 => Ok(Vec::new()),
        Ok(out) if out.code == 2 => {
            let rows: Vec<Value> = serde_json::from_str(&text).unwrap_or_default();
            Ok(rows
                .iter()
                .map(|r| Finding {
                    rule: r["RuleID"].as_str().unwrap_or("unknown").to_string(),
                    file: r["File"].as_str().unwrap_or("").to_string(),
                    line: r["StartLine"].as_u64().unwrap_or(0),
                    commit: r["Commit"]
                        .as_str()
                        .filter(|c| !c.is_empty())
                        .map(|c| c.chars().take(7).collect()),
                })
                .collect())
        }
        Ok(_) => Err("failed".into()),
    }
}

fn scan_untracked(repo: &Path) -> Vec<Finding> {
    let listing = git_out(repo, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
    let mut out = Vec::new();
    for name in listing.lines() {
        let path = repo.join(name);
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > SCAN_FILE_LIMIT {
            continue;
        }
        let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()).into_owned();
        for (index, line) in text.lines().enumerate() {
            for rule in scan_line(line) {
                out.push(Finding {
                    rule: rule.to_string(),
                    file: name.to_string(),
                    line: index as u64 + 1,
                    commit: None,
                });
            }
        }
    }
    out
}

struct Checks {
    value: Value,
    findings: Vec<Finding>,
}

fn findings_json(rows: &[Finding]) -> Value {
    serde_json::to_value(rows).unwrap_or(Value::Null)
}

fn run_checks(repo: &Path, state: &LocalState) -> Checks {
    let audit = match audit_repo(Some(repo)) {
        Ok(report) => json!({
            "ran": true,
            "skills": report.skill_count,
            "high": report.result.findings.iter().filter(|f| f.severity == "high").count(),
            "findings": report.result.findings.iter().map(|f| json!({
                "severity": f.severity,
                "skill": f.skill_name,
                "message": f.message,
                "line": f.line,
            })).collect::<Vec<_>>(),
        }),
        Err(error) => json!({"ran": false, "error": error}),
    };
    let range = state.upstream.as_ref().map(|u| format!("{u}..HEAD"));
    let mut builtin = Vec::new();
    if state.uncommitted > 0 {
        let tracked = git_out(repo, &["diff", "HEAD", "-U0", "--no-color", "--no-ext-diff"]);
        builtin.extend(scan_patch(&tracked.unwrap_or_default()));
        builtin.extend(scan_untracked(repo));
    }
    if let Some(range) = &range {
        let patch = git_out(
            repo,
            &["log", "-p", "-U0", "--no-color", "--no-ext-diff", "--format=commit:%h", range],
        );
        builtin.extend(scan_patch(&patch.unwrap_or_default()));
    }
    let mut leaks = Vec::new();
    let mut gitleaks_state = json!({"available": true, "ran": true, "error": null});
    let mut runs = Vec::new();
    if state.uncommitted > 0 {
        runs.push(Mode::Worktree);
    }
    if state.ahead > 0 {
        if let Some(range) = &range {
            runs.push(Mode::Commits(range));
        }
    }
    if runs.is_empty() {
        gitleaks_state["ran"] = json!(false);
    }
    for mode in runs {
        match gitleaks(repo, mode) {
            Ok(found) => leaks.extend(found),
            Err(reason) => {
                gitleaks_state = json!({
                    "available": reason != "not installed",
                    "ran": false,
                    "error": reason,
                });
                break;
            }
        }
    }
    gitleaks_state["count"] = json!(leaks.len());
    gitleaks_state["findings"] = findings_json(&leaks);
    let mut findings = builtin.clone();
    findings.extend(leaks);
    findings.sort_by(|a, b| (&a.file, a.line, &a.rule).cmp(&(&b.file, b.line, &b.rule)));
    findings.dedup();
    Checks {
        value: json!({
            "audit": audit,
            "gitleaks": gitleaks_state,
            "builtinScan": {"count": builtin.len(), "findings": findings_json(&builtin)},
            "blocked": !findings.is_empty(),
        }),
        findings,
    }
}

fn describe(rows: &[Finding]) -> String {
    let listed: Vec<String> = rows
        .iter()
        .take(5)
        .map(|f| format!("{} in {}:{}", f.rule, f.file, f.line))
        .collect();
    let more = rows.len().saturating_sub(5);
    format!(
        "{}{}",
        listed.join(", "),
        if more > 0 { format!(" and {more} more") } else { String::new() }
    )
}

pub fn push(repo: &Path, message: Option<&str>, dry_run: bool) -> Result<Value, OpError> {
    require_repo(repo)?;
    let state = local_state(repo);
    let message = message
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(DEFAULT_MESSAGE);
    let checks = run_checks(repo, &state);
    let range = state.upstream.as_ref().map(|u| format!("{u}..HEAD"));
    let commits = range.as_deref().map(|r| commit_list(repo, r)).unwrap_or_default();
    let remote_head = state
        .upstream
        .as_deref()
        .and_then(|u| git_out(repo, &["rev-parse", "--verify", "-q", u]))
        .unwrap_or_default();
    let undo = format!("git -C {} revert --no-edit {remote_head}..HEAD", quote(repo));
    let nothing = state.uncommitted == 0 && state.ahead == 0;
    if !dry_run && !checks.findings.is_empty() {
        return Err(OpError::failed(
            "refused",
            format!(
                "secret scan blocked the push: {} (nothing was pushed)",
                describe(&checks.findings)
            ),
        ));
    }
    if dry_run {
        let mut steps = vec![note(format!(
            "audit: {} skill(s), {} finding(s), {} high",
            checks.value["audit"]["skills"],
            checks.value["audit"]["findings"].as_array().map_or(0, Vec::len),
            checks.value["audit"]["high"]
        ))];
        steps.push(note(match checks.value["gitleaks"]["error"].as_str() {
            Some(reason) => format!(
                "gitleaks {reason}; built-in scan: {} finding(s)",
                checks.value["builtinScan"]["count"]
            ),
            None if checks.value["gitleaks"]["ran"] == Value::Bool(false) => format!(
                "gitleaks: nothing to scan; built-in scan: {} finding(s)",
                checks.value["builtinScan"]["count"]
            ),
            None => format!(
                "gitleaks: {} finding(s); built-in scan: {} finding(s)",
                checks.value["gitleaks"]["count"], checks.value["builtinScan"]["count"]
            ),
        }));
        if state.uncommitted > 0 {
            steps.push(exec(
                repo,
                format!(
                    "git add -A and git commit -m \"{message}\" ({} changed file(s))",
                    state.uncommitted
                ),
            ));
        }
        steps.extend(commits.iter().take(LISTED).map(|c| {
            note(format!(
                "commit {} {}",
                c["sha"].as_str().unwrap_or(""),
                c["subject"].as_str().unwrap_or("")
            ))
        }));
        steps.push(exec(repo, "git push (never forced)"));
        let mut warnings = Vec::new();
        if !checks.findings.is_empty() {
            warnings.push(format!(
                "the push would be refused: {}",
                describe(&checks.findings)
            ));
        }
        if checks.value["audit"]["high"].as_u64().unwrap_or(0) > 0 {
            warnings.push("the skills audit reports high severity findings".to_string());
        }
        if state.upstream.is_none() {
            warnings.push("no upstream branch: the push would be refused".to_string());
        }
        let total = commits.len() + usize::from(state.uncommitted > 0);
        return Ok(json!({
            "dryRun": true,
            "repo": repo.display().to_string(),
            "branch": state.branch,
            "pushed": false,
            "plan": plan_value(PlanV1 {
                summary: if nothing {
                    "Nothing to push".to_string()
                } else {
                    format!(
                        "Push {total} commit(s) to {}",
                        state.upstream.as_deref().unwrap_or("the remote")
                    )
                },
                steps,
                effects: Effects::default(),
                warnings,
                undo,
            }),
            "commits": commits,
            "checks": checks.value,
        }));
    }
    if nothing {
        return Ok(json!({
            "dryRun": false,
            "repo": repo.display().to_string(),
            "pushed": false,
            "message": "working tree clean; nothing to commit",
            "checks": checks.value,
            "result": applied(Vec::new(), String::new()),
        }));
    }
    if state.upstream.is_none() {
        return Err(OpError::failed(
            "refused",
            "no upstream branch; run git push -u origin <branch> once from the repository",
        ));
    }
    let mut changed = Vec::new();
    if state.uncommitted > 0 {
        git_must(repo, &["add", "-A"])?;
        changed = git_out(repo, &["diff", "--cached", "--name-only"])
            .unwrap_or_default()
            .lines()
            .map(|f| repo.join(f).display().to_string())
            .collect();
        git_must(repo, &["commit", "-q", "-m", message])?;
    }
    let sha = git_out(repo, &["rev-parse", "HEAD"]).unwrap_or_default();
    git_must(repo, &["push"])?;
    Ok(json!({
        "dryRun": false,
        "repo": repo.display().to_string(),
        "pushed": true,
        "commitSha": sha,
        "checks": checks.value,
        "result": applied(changed, undo),
    }))
}
