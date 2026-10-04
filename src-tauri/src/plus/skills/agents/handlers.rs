//! `plus.agents.*`: the agent commands over the shared cores in `agents::*` and `skills::ops`.
//! Arguments use snake_case keys, results camelCase; `global_mode` (default true) selects the
//! user-level scope like `plus.skills.sync`. `clean`, `uninstall` and `add` take `repo_path`
//! literally (default: the working directory) as mcpm does, the read commands discover the
//! repository from it. Agent files that fail to parse are skipped and listed as
//! `discoveryWarnings`.

use super::lint::lint_agents;
use super::manage::add_agent;
use super::{
    all_agent_transpilers, discover_agents_report, sync_scoped, Agent, AgentFrontmatter,
};
use crate::plus::args::{flag, flag_or, list, str_nonempty};
use crate::plus::skills::audit::audit_agents;
use crate::plus::skills::clock::SystemClock;
use crate::plus::skills::lock::load_lockfile;
use crate::plus::skills::ops::{
    self, clean_agents, diff_agents, has_drift, lock_output_root, read_lock, Scope,
};
use crate::plus::skills::repo::{find_repo, resolve_path};
use crate::plus::skills::state_handlers::{display, literal_repo, paths, scope_name};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn discovered(args: &Value) -> Result<(PathBuf, Vec<Agent>, Vec<String>), String> {
    let start = str_nonempty(args, "repo_path").map(|p| resolve_path(Path::new(p)));
    let repo = find_repo(start.as_deref()).ok_or("no skills repository found")?;
    let (agents, warnings) = discover_agents_report(&repo);
    Ok((repo, agents, warnings))
}

fn row(agent: &Agent) -> Value {
    let fm: &AgentFrontmatter = &agent.frontmatter;
    json!({
        "name": agent.name(),
        "description": fm.description,
        "model": fm.model,
        "tools": fm.tools,
        "path": display(&agent.source_path),
    })
}

pub fn list_handler(args: Value) -> Result<Value, String> {
    let (repo, agents, warnings) = discovered(&args)?;
    Ok(json!({
        "repo": display(&repo),
        "agents": agents.iter().map(row).collect::<Vec<_>>(),
        "discoveryWarnings": warnings,
    }))
}

pub fn lint_handler(args: Value) -> Result<Value, String> {
    let (repo, agents, warnings) = discovered(&args)?;
    let result = lint_agents(&agents);
    let (errors, warns) = (result.errors().count(), result.warnings().count());
    Ok(json!({
        "repo": display(&repo),
        "agentCount": agents.len(),
        "errors": errors,
        "warnings": warns,
        "infos": result.messages.len() - errors - warns,
        "messages": result
            .messages
            .iter()
            .map(|m| json!({"level": m.level, "name": m.name, "message": m.message}))
            .collect::<Vec<_>>(),
        "discoveryWarnings": warnings,
    }))
}

pub fn audit_handler(args: Value) -> Result<Value, String> {
    let (repo, agents, warnings) = discovered(&args)?;
    let findings = audit_agents(&agents).findings;
    let count = |severity: &str| findings.iter().filter(|f| f.severity == severity).count();
    Ok(json!({
        "repo": display(&repo),
        "agentCount": agents.len(),
        "clean": findings.is_empty(),
        "high": count("high"),
        "medium": count("medium"),
        "low": count("low"),
        "findings": findings
            .iter()
            .map(|f| json!({
                "severity": f.severity,
                "agent": f.skill_name,
                "message": f.message,
                "line": f.line,
            }))
            .collect::<Vec<_>>(),
        "discoveryWarnings": warnings,
    }))
}

pub fn diff_handler(args: Value) -> Result<Value, String> {
    let (repo, agents, warnings) = discovered(&args)?;
    let lock = read_lock(&repo).map(|(lock, _)| lock);
    let report = diff_agents(&agents, lock.as_ref())?;
    Ok(json!({
        "repo": display(&repo),
        "noLockfile": report.no_lockfile,
        "clean": report.is_clean(),
        "new": report.new,
        "modified": report.modified,
        "removed": report.removed,
        "unchanged": report.unchanged,
        "discoveryWarnings": warnings,
    }))
}

pub fn status_handler(args: Value) -> Result<Value, String> {
    let start = str_nonempty(&args, "repo_path").map(|p| resolve_path(Path::new(p)));
    let repo = find_repo(start.as_deref()).ok_or("no skills repository found")?;
    let found = read_lock(&repo);
    let mut outputs = Vec::new();
    let mut output_root = Value::Null;
    if let Some((lock, source)) = &found {
        let root = lock_output_root(*source, &repo)?;
        outputs = ops::agents_status(lock, &all_agent_transpilers(), &root);
        output_root = json!(display(&root));
    }
    Ok(json!({
        "repo": display(&repo),
        "lockfilePresent": found.is_some(),
        "lockedCount": found.as_ref().map_or(0, |(lock, _)| lock.agents.len()),
        "outputRoot": output_root,
        "drift": has_drift(&outputs),
        "outputs": outputs
            .iter()
            .map(|r| json!({"name": r.name, "client": r.client, "present": r.present}))
            .collect::<Vec<_>>(),
    }))
}

pub fn clean_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let scope = Scope::new(global, &literal_repo(&args))?;
    let lock = load_lockfile(&scope.lock_dir);
    let out = clean_agents(
        &scope.output_root,
        &all_agent_transpilers(),
        str_nonempty(&args, "client"),
        lock.as_ref(),
        dry_run,
    );
    Ok(json!({
        "scope": scope_name(global),
        "lockDir": display(&scope.lock_dir),
        "cleanRoot": display(&scope.output_root),
        "dryRun": dry_run,
        "lockfilePresent": lock.is_some(),
        "managed": out.managed,
        "removed": paths(&out.removed),
        "skipped": out
            .skipped
            .iter()
            .map(|(client, error)| json!({"client": client, "error": error}))
            .collect::<Vec<_>>(),
        "ignored": out.ignored,
    }))
}

pub fn uninstall_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let name = str_nonempty(&args, "name").ok_or("name is required")?;
    let repo = literal_repo(&args);
    let scope = Scope::new(global, &repo)?;
    let out = ops::uninstall_agent(&repo, name, &scope, &all_agent_transpilers(), dry_run)?;
    Ok(json!({
        "repo": display(&repo),
        "name": name,
        "scope": scope_name(global),
        "lockDir": display(&scope.lock_dir),
        "outputRoot": display(&scope.output_root),
        "dryRun": dry_run,
        "sourcePath": display(&out.source),
        "outputs": paths(&out.outputs),
        "lockUpdated": out.lock_updated,
    }))
}

pub fn add_handler(args: Value) -> Result<Value, String> {
    let dry_run = flag(&args, "dry_run");
    let name = str_nonempty(&args, "name").ok_or("name is required")?;
    let report = add_agent(&literal_repo(&args), name, dry_run)?;
    Ok(json!({
        "repo": display(&report.repo),
        "name": name,
        "path": display(&report.agent_file),
        "dryRun": dry_run,
    }))
}

fn client_keys(args: &Value) -> Option<Vec<String>> {
    let keys: Vec<String> = list(args, "client_keys")?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    (!keys.is_empty()).then_some(keys)
}

pub fn sync_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let (repo, agents, warnings) = discovered(&args)?;
    let scope = Scope::new(global, &repo)?;
    let mut data = json!({
        "repo": display(&repo),
        "scope": scope_name(global),
        "outputRoot": display(&scope.output_root),
        "lockDir": display(&scope.lock_dir),
        "dryRun": dry_run,
        "foundCount": agents.len(),
        "agentCount": 0,
        "clientCount": 0,
        "agents": [],
        "discoveryWarnings": warnings,
    });
    if agents.is_empty() {
        return Ok(data);
    }
    let synced = sync_scoped(
        &repo,
        &agents,
        global,
        dry_run,
        client_keys(&args),
        &SystemClock,
    )?;
    let lock = &synced.lock;
    let clients: BTreeSet<&String> = lock
        .agents
        .iter()
        .flat_map(|(_, e)| e.clients_synced.iter())
        .collect();
    data["syncedAt"] = json!(lock.synced_at);
    data["agentCount"] = json!(lock.agents.len());
    data["clientCount"] = json!(clients.len());
    data["agents"] = lock
        .agents
        .iter()
        .map(|(name, entry)| {
            let source = agents.iter().find(|a| a.name() == name);
            json!({
                "name": name,
                "found": source.is_some(),
                "model": source.and_then(|a| a.frontmatter.model.clone()),
                "clientsSynced": entry.clients_synced,
                "warnings": entry.warnings,
                "outputFiles": entry
                    .output_files
                    .iter()
                    .map(|(client, files)| json!({"client": client, "files": files}))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(data)
}
