//! `skills init|add|audit`: repository management over `skills::repo`. Arguments use snake_case keys, `repo_path` being the repository root.

use super::repo::{self, AddRequest};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn switch(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn dir_arg(args: &Value) -> PathBuf {
    PathBuf::from(text(args, "repo_path").unwrap_or("."))
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn init_handler(args: Value) -> Result<Value, String> {
    let dry_run = switch(&args, "dry_run");
    let report = repo::init_repo(&dir_arg(&args), text(&args, "name"), dry_run)?;
    Ok(json!({
        "repo": display(&report.repo),
        "name": report.name,
        "alreadyExists": report.already_exists,
        "created": report.created,
        "dryRun": dry_run,
    }))
}

pub fn add_handler(args: Value) -> Result<Value, String> {
    let dry_run = switch(&args, "dry_run");
    let with_progressive = switch(&args, "with_progressive");
    let report = repo::add_skill(&AddRequest {
        repo: &dir_arg(&args),
        name: text(&args, "name").unwrap_or_default(),
        skill_type: text(&args, "skill_type").unwrap_or("skill"),
        with_progressive,
        dry_run,
    })?;
    Ok(json!({
        "repo": display(&report.repo),
        "name": text(&args, "name"),
        "type": report.skill_type,
        "path": display(&report.skill_file),
        "files": report.files.iter().map(|f| display(f)).collect::<Vec<_>>(),
        "progressive": with_progressive,
        "dryRun": dry_run,
    }))
}

pub fn audit_handler(args: Value) -> Result<Value, String> {
    let start = text(&args, "repo_path").map(PathBuf::from);
    let report = repo::audit_repo(start.as_deref())?;
    let findings = &report.result.findings;
    let count = |severity: &str| findings.iter().filter(|f| f.severity == severity).count();
    Ok(json!({
        "repo": display(&report.repo),
        "skillCount": report.skill_count,
        "clean": findings.is_empty(),
        "high": count("high"),
        "medium": count("medium"),
        "low": count("low"),
        "findings": findings
            .iter()
            .map(|f| json!({
                "severity": f.severity,
                "skill": f.skill_name,
                "message": f.message,
                "line": f.line,
            }))
            .collect::<Vec<_>>(),
    }))
}
