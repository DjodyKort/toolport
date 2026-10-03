//! `plus.skills.{init,add,audit,bundle,unbundle}`: repository management over `skills::repo` and
//! `skills::bundle`. Arguments use snake_case keys, `repo_path` being the repository root.

use super::bundle::{
    create_bundle, extract_bundle, plan_bundle, skill_files, BundleOptions, BundlePlan,
};
use super::clock::SystemClock;
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

fn names(args: &Value, key: &str) -> Option<Vec<String>> {
    let list: Vec<String> = match args.get(key)? {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        Value::String(s) => s.split(',').map(|p| p.trim().to_string()).collect(),
        _ => return None,
    };
    (!list.is_empty()).then_some(list)
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

pub fn bundle_handler(args: Value) -> Result<Value, String> {
    let start = text(&args, "repo_path").map(|p| repo::resolve_path(Path::new(p)));
    let repo = repo::find_repo(start.as_deref()).ok_or("no skills repository found")?;
    let dry_run = switch(&args, "dry_run");
    let opts = BundleOptions {
        output: text(&args, "output").map(|o| repo::resolve_path(Path::new(o))),
        skill_names: names(&args, "skills"),
        clock: &SystemClock,
    };
    let BundlePlan { output, skills } = plan_bundle(&repo, &opts)?;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut rows = Vec::new();
    for skill in &skills {
        let list = skill_files(skill)?;
        files += list.len();
        bytes += list
            .iter()
            .map(|(_, p)| std::fs::metadata(p).map_or(0, |m| m.len()))
            .sum::<u64>();
        rows.push(json!({
            "name": skill.name(),
            "type": format!("{:?}", skill.skill_type).to_lowercase(),
            "files": list.len(),
        }));
    }
    let size = if dry_run {
        Value::Null
    } else {
        let written = create_bundle(&repo, &opts)?;
        json!(std::fs::metadata(&written)
            .map_err(|e| e.to_string())?
            .len())
    };
    Ok(json!({
        "repo": display(&repo),
        "output": display(&output),
        "skills": rows,
        "fileCount": files,
        "sourceBytes": bytes,
        "bundleBytes": size,
        "dryRun": dry_run,
    }))
}

pub fn unbundle_handler(args: Value) -> Result<Value, String> {
    let bundle = text(&args, "bundle_path").ok_or("bundle_path is required")?;
    let dry_run = switch(&args, "dry_run");
    let target = repo::resolve_path(&dir_arg(&args));
    let report = extract_bundle(Path::new(bundle), &target, dry_run)?;
    Ok(json!({
        "bundle": bundle,
        "target": display(&target),
        "names": report.names,
        "files": report.files,
        "overwritten": report.overwritten,
        "skipped": report.skipped,
        "dryRun": dry_run,
    }))
}
