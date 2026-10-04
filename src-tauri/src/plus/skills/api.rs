//! The skills operations as typed functions: `list`, `get`, `lint`, `diff`, `status`, `sync` and
//! `transpilers` take [`Args`] and return the JSON every surface serves. `toolportctl skills`, the
//! `plus.skills.*` handlers and the self-MCP skills tools only translate their input into
//! [`Args`] and render or wrap the result; they share nothing else.

use super::lint::{lint_skills, LintResult};
use super::ops::{
    diff_skills, find_skills_repo, has_drift, lock_dir, lock_output_root, read_lock,
    skills_status as output_rows,
};
use super::assets::compute_skill_hash;
use super::lock::{get_entry, save_lockfile};
use super::parser::{discover_skills, find_skill, Skill, SkillType};
use super::sync_report::sync_report;
use super::transpiler::TranspilerRegistry;
use super::transpilers::{self, registry_with_home};
use super::{sync_skills, SyncOptions, SystemClock};
use crate::plus::args::{list, nonempty_strings, str_arg};
use crate::plus::op::OpError;
use crate::registry;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_REPO: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// What an operation reads: `repo` is where discovery starts, `name` is the skill `get` reads,
/// `names` narrows `lint`, `clients` narrows `status` and `sync`, and `dry_run`, `global` and
/// `migrate` steer `sync`.
#[derive(Clone, Debug)]
pub struct Args {
    pub repo: Option<PathBuf>,
    pub name: String,
    pub names: Option<Vec<String>>,
    pub clients: Option<Vec<String>>,
    pub dry_run: bool,
    pub global: bool,
    pub migrate: Option<bool>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            repo: None,
            name: String::new(),
            names: None,
            clients: None,
            dry_run: false,
            global: true,
            migrate: None,
        }
    }
}

impl Args {
    /// The snake_case keys of the self-MCP tools and `plus.skills.*`; a value of another type
    /// counts as absent.
    pub fn from_json(args: &Value) -> Self {
        Self {
            repo: str_arg(args, "repo_path").map(PathBuf::from),
            name: str_arg(args, "name").unwrap_or_default().to_string(),
            names: list(args, "names").map(|names| {
                names
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            }),
            clients: nonempty_strings(args, "client_keys"),
            dry_run: crate::plus::args::flag(args, "dry_run"),
            global: crate::plus::args::flag_or(args, "global_mode", true),
            migrate: args.get("migrate").and_then(Value::as_bool),
        }
    }
}

fn backend(message: impl Into<String>) -> OpError {
    OpError::failed("skills", message)
}

pub(crate) fn home() -> Result<PathBuf, OpError> {
    crate::clients::home().ok_or_else(|| OpError::not_found("home directory unknown"))
}

/// The skills repository `start` leads to, or the one the current directory or the git-sync
/// config leads to.
pub(crate) fn resolve_repo(start: Option<&Path>) -> Result<PathBuf, OpError> {
    #[cfg(test)]
    if start.is_none() {
        if let Some(repo) = TEST_REPO.with(|r| r.borrow().clone()) {
            return Ok(repo);
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let config = registry::conduit_dir().unwrap_or_else(|| cwd.clone());
    find_skills_repo(start, &cwd, &config)
        .ok_or_else(|| OpError::not_found("no skills repository found"))
}

fn load(args: &Args) -> Result<(PathBuf, Vec<Skill>), OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let skills = discover_skills(&repo);
    Ok((repo, skills))
}

fn row(skill: &Skill) -> Value {
    json!({
        "name": skill.name(),
        "description": skill.frontmatter.description,
        "activation": skill.frontmatter.activation.as_str(),
        "type": skill.skill_type.as_str(),
        "path": skill.source_path.to_string_lossy(),
    })
}

pub(crate) fn lint_json(result: &LintResult) -> Value {
    json!({
        "errors": result.errors().count(),
        "warnings": result.warnings().count(),
        "messages": result.messages.iter().map(|m| json!({"level": m.level, "name": m.name, "message": m.message})).collect::<Vec<_>>(),
    })
}

pub fn list_skills(args: &Args) -> Result<Value, OpError> {
    let (repo, skills) = load(args)?;
    Ok(json!({"repo": repo.to_string_lossy(), "skills": skills.iter().map(row).collect::<Vec<_>>()}))
}

pub fn get(args: &Args) -> Result<Value, OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let skill = find_skill(&repo, &args.name)
        .ok_or_else(|| OpError::not_found(format!("skill not found: {}", args.name)))?;
    let mut row = row(&skill);
    row["body"] = Value::String(skill.body);
    Ok(row)
}

pub fn lint(args: &Args) -> Result<Value, OpError> {
    let (_, mut skills) = load(args)?;
    if let Some(names) = &args.names {
        skills.retain(|s| names.iter().any(|n| n == s.name()));
    }
    Ok(lint_json(&lint_skills(&skills)))
}

pub fn transpilers() -> Value {
    let mut reg = TranspilerRegistry::new();
    transpilers::register_all(&mut reg);
    json!({"transpilers": reg.all().map(|t| t.client_key().to_string()).collect::<Vec<_>>()})
}

pub fn diff(args: &Args) -> Result<Value, OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let lock = read_lock(&repo).map(|(lock, _)| lock);
    let skills = discover_skills(&repo);
    let report = diff_skills(&skills, lock.as_ref()).map_err(backend)?;
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "noLockfile": report.no_lockfile,
        "clean": report.is_clean(),
        "new": report.new,
        "modified": report.modified,
        "removed": report.removed,
        "unchanged": report.unchanged,
    }))
}

pub fn status(args: &Args) -> Result<Value, OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let found = read_lock(&repo);
    let lock = found.as_ref().map(|(lock, _)| lock);
    let skills = discover_skills(&repo);
    let mut entries = Vec::new();
    for skill in &skills {
        let bucket = lock.map(|l| match skill.skill_type {
            SkillType::Rule => &l.rules,
            SkillType::Skill => &l.skills,
        });
        let entry = bucket.and_then(|b| get_entry(b, skill.name()));
        let current = compute_skill_hash(skill).ok();
        entries.push(json!({
            "name": skill.name(),
            "type": skill.skill_type.as_str(),
            "knownToLockfile": entry.is_some(),
            "currentHash": current,
            "lockfileHash": entry.map(|e| e.hash.clone()),
            "drifted": entry.is_some_and(|e| Some(&e.hash) != current.as_ref()),
            "clientsSynced": entry.map(|e| e.clients_synced.clone()).unwrap_or_default(),
        }));
    }
    let transpilers = registry_with_home(crate::clients::home());
    let targeted = args.clients.clone().unwrap_or_else(|| {
        transpilers
            .all()
            .map(|t| t.client_key().to_string())
            .collect()
    });
    let mut outputs = Vec::new();
    let mut output_root = Value::Null;
    if let Some((lock, source)) = &found {
        let root = lock_output_root(*source, &repo).map_err(OpError::not_found)?;
        outputs = output_rows(lock, &transpilers, &root);
        outputs.retain(|row| targeted.contains(&row.client));
        output_root = json!(root.to_string_lossy());
    }
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "lockfilePresent": lock.is_some(),
        "lockfileSyncedAt": lock.map(|l| l.synced_at.clone()),
        "lockedCount": lock.map_or(0, |l| l.skills.len() + l.rules.len()),
        "targetedClients": targeted,
        "entries": entries,
        "outputRoot": output_root,
        "drift": has_drift(&outputs),
        "outputs": outputs
            .iter()
            .map(|row| json!({"name": row.name, "client": row.client, "present": row.present}))
            .collect::<Vec<_>>(),
    }))
}

pub fn sync(args: &Args) -> Result<Value, OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let skills = discover_skills(&repo);
    let global = args.global;
    let output_root = if global { home()? } else { repo.clone() };
    let dir = lock_dir(global, &repo);
    let opts = SyncOptions {
        output_root,
        lock_dir: dir.clone(),
        global_mode: global,
        dry_run: args.dry_run,
        migrate: args.migrate,
        client_keys: args.clients.clone(),
        clock: &SystemClock,
    };
    let result = sync_skills(&skills, &registry_with_home(Some(home()?)), &opts)
        .map_err(backend)?;
    if !opts.dry_run {
        save_lockfile(&dir, &result.lockfile).map_err(backend)?;
    }
    let mut data = json!({
        "repo": repo.to_string_lossy(),
        "dryRun": opts.dry_run,
        "globalMode": global,
        "outputRoot": result.output_root.to_string_lossy(),
        "syncedAt": result.lockfile.synced_at,
        "skillCount": result.lockfile.skills.len(),
        "ruleCount": result.lockfile.rules.len(),
        "cleaned": result.cleaned.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
    });
    if let Some(fields) = data.as_object_mut() {
        fields.extend(sync_report(&result));
    }
    Ok(data)
}
