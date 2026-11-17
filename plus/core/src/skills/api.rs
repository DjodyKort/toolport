//! The skills operations as typed functions: `list`, `get`, `lint`, `diff`, `status`, `sync` and
//! `transpilers` take [`Args`] and return the JSON every surface serves. `toolportctl skills`, the
//! `plus.skills.*` handlers and the self-MCP skills tools only translate their input into
//! [`Args`] and render or wrap the result; they share nothing else.

use super::assets::compute_skill_hash;
use super::auto;
use super::client_scope::{self, default_clients, ClientSource};
use super::clock::Clock;
use super::fix::chmod_hooks;
use super::frontmatter::frontmatter_accepted;
use super::lint::{
    lint_description_lengths, lint_files, lint_outputs, lint_parse_failures, lint_skills,
    LintMessage, LintResult, FIX_CHMOD,
};
use super::lock::{get_entry, load_lockfile, lockfile_path, save_lockfile};
use super::ops::{
    check_outputs, diff_skills, find_skills_repo, has_drift, lock_dir, lock_output_root,
    read_lock_in, skills_status as output_rows, LockRead,
};
use super::parser::{
    discover_skills, discover_skills_report, find_skill, Discovery, Skill, SkillType,
    DESCRIPTION_LIMIT, SPEC_DESCRIPTION_LIMIT,
};
use super::snapshot::{self, Journal, Recording};
use super::sync_report::sync_report;
use super::transpiler::TranspilerRegistry;
use super::transpilers::registry;
use super::verify::{verify_clients, verify_installed};
use super::{sync_skills, SyncOptions, SystemClock};
use crate::args::{list, nonempty_strings, str_arg};
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::SettingsStore;

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
    /// `sync`: also apply removals, new hooks and replacements of files that differ.
    pub apply_held: bool,
    /// `sync`: also install the hooks of skills that came from a tap.
    pub allow_tap_hooks: bool,
    /// `sync`: the run a schedule makes. It does nothing while auto-sync is paused and never
    /// applies what waits for a decision.
    pub unattended: bool,
    /// `list`: only the items of this source (`library`, `repo:odh`, `plugin:ecc@ecc`).
    pub source: Option<String>,
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
            apply_held: false,
            allow_tap_hooks: false,
            unattended: false,
            source: None,
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
            dry_run: crate::args::flag(args, "dry_run"),
            global: crate::args::flag_or(args, "global_mode", true),
            migrate: args.get("migrate").and_then(Value::as_bool),
            apply_held: crate::args::flag(args, "apply_held"),
            allow_tap_hooks: crate::args::flag(args, "allow_tap_hooks"),
            unattended: crate::args::flag(args, "unattended"),
            source: str_arg(args, "source").map(String::from),
        }
    }
}

fn backend(message: impl Into<String>) -> OpError {
    OpError::failed("skills", message)
}

pub(crate) fn home() -> Result<PathBuf, OpError> {
    crate::fsx::home().ok_or_else(|| OpError::not_found("home directory unknown"))
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
    let config = crate::fsx::data_dir().unwrap_or_else(|| cwd.clone());
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

fn message_json(message: &LintMessage) -> Value {
    let mut row = json!({"level": message.level, "name": message.name, "message": message.message});
    let Some(detail) = &message.detail else {
        return row;
    };
    row["code"] = json!(detail.code);
    row["consequence"] = json!(detail.consequence);
    for (key, value) in [
        ("file", detail.file.as_deref()),
        ("target", detail.target.as_deref()),
        ("reason", detail.reason.as_deref()),
        ("fixable", detail.fixable),
    ] {
        if let Some(value) = value {
            row[key] = json!(value);
        }
    }
    if let Some(line) = detail.line {
        row["line"] = json!(line);
    }
    if let Some(meter) = detail.meter {
        row["meter"] =
            json!({"length": meter.length, "specLimit": meter.spec_limit, "limit": meter.limit});
    }
    row
}

pub(crate) fn lint_json(result: &LintResult) -> Value {
    json!({
        "errors": result.errors().count(),
        "warnings": result.warnings().count(),
        "messages": result.messages.iter().map(message_json).collect::<Vec<_>>(),
    })
}

/// The skills lint: the rows plus the two description limits one meter draws and how many rows
/// each consequence has.
fn skills_lint_json(result: &LintResult) -> Value {
    let mut data = lint_json(result);
    let mut consequences = serde_json::Map::new();
    for detail in result.messages.iter().filter_map(|m| m.detail.as_ref()) {
        let key = json!(detail.consequence)
            .as_str()
            .unwrap_or_default()
            .to_string();
        let count = consequences.get(&key).and_then(Value::as_u64).unwrap_or(0);
        consequences.insert(key, json!(count + 1));
    }
    data["limits"] =
        json!({"specDescription": SPEC_DESCRIPTION_LIMIT, "description": DESCRIPTION_LIMIT});
    data["consequences"] = Value::Object(consequences);
    data
}

fn library_origin(repo: &Path) -> Value {
    let name = repo
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    json!({"kind": "library", "name": name})
}

fn library_row(skill: &Skill, origin: &Value, reason: Option<String>) -> Value {
    let mut row = row(skill);
    row["origin"] = origin.clone();
    row["writable"] = json!(true);
    row["visible"] = json!(reason.is_none());
    row["invisibleReason"] = json!(reason);
    row
}

/// The skills, rules and commands of one source from the sources scan, in the shape of a library row.
fn source_rows(id: &str) -> Result<Value, OpError> {
    let report = crate::sources::scan_host(&crate::sources::ScanOptions {
        source: Some(id.to_string()),
        items: true,
        ..Default::default()
    })
    .ok_or_else(|| OpError::not_found("home directory unknown"))?;
    if report.sources.is_empty() {
        return Err(OpError::not_found(format!("unknown source: {id}")));
    }
    let rows: Vec<Value> = report
        .items
        .iter()
        .filter(|i| matches!(i.kind, "skill" | "rule" | "command"))
        .map(|i| {
            json!({
                "name": i.name,
                "description": i.description,
                "activation": i.activation.as_deref().unwrap_or("auto"),
                "type": i.kind,
                "path": i.path,
                "origin": i.origin,
                "writable": i.writable,
                "visible": i.visible,
                "invisibleReason": i.invisible_reason,
            })
        })
        .collect();
    Ok(json!({
        "repo": report.sources.first().and_then(|s| s.root.clone()),
        "skills": rows,
        "partial": report.partial,
        "skipped": report.skipped,
    }))
}

pub fn list_skills(args: &Args) -> Result<Value, OpError> {
    if let Some(id) = args.source.as_deref().filter(|s| *s != "library") {
        return source_rows(id);
    }
    let (repo, skills) = load(args)?;
    let origin = library_origin(&repo);
    let home = home()?;
    let registry = registry();
    let rows: Vec<Value> = skills
        .iter()
        .map(|s| library_row(s, &origin, invisible_reason(&registry, s, &home)))
        .collect();
    Ok(json!({"repo": repo.to_string_lossy(), "skills": rows}))
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
    let repo = resolve_repo(args.repo.as_deref())?;
    let Discovery {
        skills: mut found,
        mut failures,
        ..
    } = discover_skills_report(&repo);
    if let Some(names) = &args.names {
        found.retain(|s| names.iter().any(|n| n == s.name()));
        failures.retain(|f| names.contains(&f.name));
    }
    let skills = found;
    let mut result = lint_skills(&skills);
    let outputs = lint_outputs(&skills, &registry());
    result.messages.extend(outputs.messages);
    result.messages.extend(lint_files(&skills).messages);
    result
        .messages
        .extend(lint_description_lengths(&skills, other_clients_on(args)).messages);
    result
        .messages
        .extend(lint_parse_failures(&repo, &failures).messages);
    Ok(skills_lint_json(&result))
}

/// Applies the one safe fix a lint row offers (`fixable`), to the skills in `args.names` or to
/// all; `dry_run` reports what would change.
pub fn fix(args: &Args, kind: &str) -> Result<Value, OpError> {
    if kind != FIX_CHMOD {
        return Err(OpError::usage(format!(
            "unknown fix '{kind}'; the only safe fix is {FIX_CHMOD}"
        )));
    }
    let (repo, mut skills) = load(args)?;
    if let Some(names) = &args.names {
        if let Some(missing) = names
            .iter()
            .find(|n| !skills.iter().any(|s| s.name() == *n))
        {
            return Err(OpError::not_found(format!("skill not found: {missing}")));
        }
        skills.retain(|s| names.iter().any(|n| n == s.name()));
    }
    let fixed = chmod_hooks(&skills, args.dry_run).map_err(backend)?;
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "fix": kind,
        "dryRun": args.dry_run,
        "count": fixed.len(),
        "fixed": fixed
            .iter()
            .map(|f| json!({"skill": f.skill, "file": f.file, "event": f.event, "from": f.from, "to": f.to}))
            .collect::<Vec<_>>(),
    }))
}

/// A lock that syncs to any client besides Claude Code makes the spec's description limit binding.
fn other_clients_on(args: &Args) -> bool {
    let Ok(repo) = resolve_repo(args.repo.as_deref()) else {
        return false;
    };
    let lock = read_lock_in(&repo, args.global).map(|r| r.lock);
    let registry = registry();
    let (clients, _) = default_clients(lock.as_ref(), &registry);
    clients.iter().any(|c| c != client_scope::FALLBACK_CLIENT)
}

pub fn transpilers() -> Value {
    let reg = registry();
    json!({"transpilers": reg.all().map(|t| t.client_key().to_string()).collect::<Vec<_>>()})
}

fn lock_warnings(read: Option<&LockRead>) -> Vec<&str> {
    read.and_then(|r| r.warning.as_deref())
        .into_iter()
        .collect()
}

pub fn diff(args: &Args) -> Result<Value, OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let read = read_lock_in(&repo, args.global);
    let skills = discover_skills(&repo);
    let report = diff_skills(&skills, read.as_ref().map(|r| &r.lock)).map_err(backend)?;
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "lockfile": read.as_ref().map(|r| r.path.to_string_lossy()),
        "warnings": lock_warnings(read.as_ref()),
        "noLockfile": report.no_lockfile,
        "clean": report.is_clean(),
        "new": report.new,
        "modified": report.modified,
        "removed": report.removed,
        "unchanged": report.unchanged,
    }))
}

fn finding_rows(rows: &[super::verify::Finding]) -> Vec<Value> {
    rows.iter()
        .map(|f| json!({"skill": f.skill, "client": f.client, "path": f.path}))
        .collect()
}

pub fn status(args: &Args) -> Result<Value, OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let read = read_lock_in(&repo, args.global);
    let lock = read.as_ref().map(|r| &r.lock);
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
    let transpilers = registry();
    check_clients(args, &transpilers)?;
    let targeted = args.clients.clone().unwrap_or_else(|| {
        transpilers
            .all()
            .map(|t| t.client_key().to_string())
            .collect()
    });
    let mut outputs = Vec::new();
    let mut rejected = Vec::new();
    let mut output_root = Value::Null;
    let mut verify = super::verify::Verify::default();
    if let Some(read) = &read {
        let root = lock_output_root(read.source, &repo).map_err(OpError::not_found)?;
        outputs = output_rows(&read.lock, &transpilers, &root);
        outputs.retain(|row| targeted.contains(&row.client));
        rejected = check_outputs(&read.lock, &transpilers, &root).rejected;
        rejected.retain(|row| targeted.contains(&row.client));
        verify = super::verify::verify_installed(&skills, &read.lock, &transpilers, &root);
        verify.missing.retain(|f| targeted.contains(&f.client));
        verify.differing.retain(|f| targeted.contains(&f.client));
        verify.mode.retain(|f| targeted.contains(&f.client));
        verify.edited.retain(|f| targeted.contains(&f.client));
        output_root = json!(root.to_string_lossy());
    }
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "lockfile": read.as_ref().map(|r| r.path.to_string_lossy()),
        "warnings": lock_warnings(read.as_ref()),
        "lockfilePresent": lock.is_some(),
        "lockfileSyncedAt": lock.map(|l| l.synced_at.clone()),
        "lockedCount": lock.map_or(0, |l| l.skills.len() + l.rules.len()),
        "targetedClients": targeted,
        "entries": entries,
        "outputRoot": output_root,
        "drift": has_drift(&outputs) || !verify.clean(),
        "verify": {
            "checked": verify.checked,
            "clean": verify.clean(),
            "missing": finding_rows(&verify.missing),
            "differing": finding_rows(&verify.differing),
            "mode": finding_rows(&verify.mode),
            "edited": finding_rows(&verify.edited),
        },
        "outputs": outputs
            .iter()
            .map(|row| json!({"name": row.name, "client": row.client, "present": row.present}))
            .collect::<Vec<_>>(),
        "rejected": rejected
            .iter()
            .map(|row| json!({
                "name": row.name,
                "client": row.client,
                "path": row.path.to_string_lossy(),
                "code": row.reason.code(),
                "reason": row.reason.to_string(),
            }))
            .collect::<Vec<_>>(),
    }))
}

fn check_clients(args: &Args, registry: &TranspilerRegistry) -> Result<(), OpError> {
    let keys = args.clients.iter().flatten().map(String::as_str);
    client_scope::ensure_supported(keys, registry).map_err(OpError::usage)
}

fn paused_report(args: &Args, state: &auto::State) -> Value {
    json!({
        "dryRun": args.dry_run,
        "unattended": true,
        "paused": true,
        "skipped": true,
        "since": state.since,
        "reason": state.reason,
        "held": [],
        "snapshot": null,
        "verify": null,
        "message": state.describe(),
    })
}

/// An unattended run applies what is safe and leaves the rest held: it refuses the flags that
/// apply held changes and never replaces a file that shadows a synced skill.
pub fn check_unattended(args: &Args) -> Result<(), OpError> {
    if !args.unattended {
        return Ok(());
    }
    if args.apply_held || args.allow_tap_hooks {
        return Err(OpError::usage(
            "an unattended sync never applies held changes: drop apply_held and allow_tap_hooks",
        ));
    }
    if args.migrate == Some(true) {
        return Err(OpError::usage(
            "an unattended sync never replaces files that shadow a synced skill: drop migrate",
        ));
    }
    Ok(())
}

fn paused_by_policy(args: &Args) -> Result<Option<Value>, OpError> {
    let Some(base) = crate::fsx::data_dir() else {
        return Ok(None);
    };
    let state = auto::read(&base)?;
    Ok(state.paused.then(|| paused_report(args, &state)))
}

fn held_items(data: &Value) -> Vec<auto::HeldItem> {
    serde_json::from_value(data["held"].clone()).unwrap_or_default()
}

fn count(data: &Value, key: &str) -> u64 {
    data["plan"][key]
        .as_array()
        .map_or(0, |rows| rows.len() as u64)
}

fn last_run_of(data: &Value) -> auto::LastRun {
    let applied = data["plan"]["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|change| change["held"] != json!(true))
        .count() as u64;
    auto::LastRun {
        at: SystemClock.now().zulu(),
        applied,
        held: held_items(data),
        skipped: count(data, "skipped"),
        error: None,
    }
}

/// An unattended run leaves its outcome in the auto-sync record; a manual run that no longer
/// holds what that run held clears it from there. A paused run did nothing and records nothing.
fn record_outcome(args: &Args, result: &Result<Value, OpError>) -> Result<(), OpError> {
    let Some(base) = crate::fsx::data_dir() else {
        return Ok(());
    };
    match result {
        Ok(data) if data["paused"] == json!(true) => Ok(()),
        Ok(data) if args.unattended => auto::record_run(&base, last_run_of(data)),
        Ok(_) if args.clients.is_some() || !args.global => Ok(()),
        Ok(data) => auto::settle_held(&base, &held_items(data)),
        Err(error) if args.unattended => auto::record_error(&base, &error.message, &SystemClock),
        Err(_) => Ok(()),
    }
}

/// `settings` is where hooks are installed; `None` only for a dry run, which never writes them.
/// A run that writes first keeps a snapshot of what it is about to change (`skills undo`).
pub fn sync(args: &Args, settings: Option<&dyn SettingsStore>) -> Result<Value, OpError> {
    check_unattended(args)?;
    let result = run_sync(args, settings);
    if !args.dry_run {
        record_outcome(args, &result)?;
    }
    result
}

fn run_sync(args: &Args, settings: Option<&dyn SettingsStore>) -> Result<Value, OpError> {
    let mut args = args.clone();
    if args.unattended {
        check_unattended(&args)?;
        if let Some(paused) = paused_by_policy(&args)? {
            return Ok(paused);
        }
        args.migrate = Some(false);
    }
    let repo = resolve_repo(args.repo.as_deref())?;
    let skills = discover_skills(&repo);
    let global = args.global;
    let output_root = if global { home()? } else { repo.clone() };
    let dir = lock_dir(global, &repo);
    let registry = registry();
    check_clients(&args, &registry)?;
    let (targeted, source) = match &args.clients {
        Some(keys) => (keys.clone(), ClientSource::Requested),
        None => default_clients(load_lockfile(&dir).as_ref(), &registry),
    };
    let journal = (!args.dry_run)
        .then(crate::fsx::data_dir)
        .flatten()
        .map(|base| Journal::begin(&base, &SystemClock));
    let lock_before = std::fs::read(lockfile_path(&dir)).ok();
    let recording = settings
        .zip(journal.as_ref())
        .map(|(inner, journal)| Recording { inner, journal });
    let (outcome, saved) = {
        let opts = SyncOptions {
            output_root: output_root.clone(),
            lock_dir: dir.clone(),
            global_mode: global,
            dry_run: args.dry_run,
            migrate: args.migrate,
            client_keys: Some(targeted.clone()),
            apply_held: args.apply_held,
            allow_tap_hooks: args.allow_tap_hooks,
            clock: &SystemClock,
            settings: match &recording {
                Some(recording) => Some(recording as &dyn SettingsStore),
                None => settings,
            },
            unattended: args.unattended,
            journal: journal.as_ref(),
        };
        let outcome = sync_skills(&skills, &registry, &opts);
        let saved = match &outcome {
            Ok(result) if !args.dry_run => save_lockfile(&dir, &result.lockfile).map_err(backend),
            _ => Ok(()),
        };
        (outcome, saved)
    };
    let snapshot = match journal {
        Some(journal) => journal
            .finish(
                settings,
                snapshot::Context {
                    global,
                    lock_dir: &dir,
                    output_root: &output_root,
                    lock_before,
                },
            )
            .map_err(backend)?,
        None => None,
    };
    let result = outcome.map_err(backend)?;
    saved?;
    let verify = if args.dry_run {
        Value::Null
    } else {
        verify_installed(&skills, &result.lockfile, &registry, &result.output_root).summary()
    };
    let mut data = json!({
        "repo": repo.to_string_lossy(),
        "dryRun": args.dry_run,
        "unattended": args.unattended,
        "paused": false,
        "globalMode": global,
        "outputRoot": result.output_root.to_string_lossy(),
        "syncedAt": result.lockfile.synced_at,
        "skillCount": result.lockfile.skills.len(),
        "ruleCount": result.lockfile.rules.len(),
        "cleaned": result.cleaned.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "targetedClients": targeted,
        "clientSource": source.as_str(),
        "snapshot": snapshot.map(|info| info.to_json()),
        "verify": verify,
    });
    if let Some(fields) = data.as_object_mut() {
        fields.extend(sync_report(&result));
    }
    Ok(data)
}

/// Every file the lock records against its library copy, with the same findings `status` lists.
/// Without a lock nothing is checked, which is not a clean result.
pub fn verify(args: &Args) -> Result<Value, OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let registry = registry();
    check_clients(args, &registry)?;
    let Some(read) = read_lock_in(&repo, args.global) else {
        return Ok(json!({
            "repo": repo.to_string_lossy(),
            "lockfilePresent": false,
            "matched": 0,
            "total": 0,
            "clean": false,
            "mismatched": [],
            "warnings": [],
        }));
    };
    let root = lock_output_root(read.source, &repo).map_err(OpError::not_found)?;
    let skills = discover_skills(&repo);
    let found = verify_clients(
        &skills,
        &read.lock,
        &registry,
        &root,
        args.clients.as_deref(),
    );
    let mut data = found.summary();
    data["repo"] = json!(repo.to_string_lossy());
    data["lockfilePresent"] = json!(true);
    data["lockfile"] = json!(read.path.to_string_lossy());
    data["outputRoot"] = json!(root.to_string_lossy());
    data["warnings"] = json!(lock_warnings(Some(&read)));
    Ok(data)
}

/// Puts the newest snapshot back (`dry_run` only reports) and pauses auto-sync.
pub fn undo(args: &Args, settings: Option<&dyn SettingsStore>) -> Result<Value, OpError> {
    let base =
        crate::fsx::data_dir().ok_or_else(|| OpError::not_found("data directory unknown"))?;
    let lock_dir = if args.global {
        base.clone()
    } else {
        resolve_repo(args.repo.as_deref())?
    };
    snapshot::undo(&base, &lock_dir, args.dry_run, settings, &SystemClock)
}

/// Why Claude Code would not list `skill`: what Toolport emits for it, or the copy already
/// deployed under `home`, fails the strict frontmatter check. `None` when it is accepted.
pub fn invisible_reason(
    registry: &TranspilerRegistry,
    skill: &Skill,
    home: &Path,
) -> Option<String> {
    let claude = registry.get("claude-code")?;
    let out = claude.transpile(skill, home).ok()?;
    if let Err(why) = frontmatter_accepted(&out.content) {
        return Some(why.to_string());
    }
    let deployed = std::fs::read_to_string(&out.output_path).ok()?;
    frontmatter_accepted(&deployed)
        .err()
        .map(|why| format!("the deployed copy is rejected: {why}"))
}
