use super::transpilers::{
    all_agent_transpilers, AgentTranspiler, AGENT_APPEND_MODE, AGENT_PROJECT_ONLY,
};
use super::Agent;
use crate::plus::skills::clock::Clock;
use crate::plus::skills::lock::{
    get_entry_mut, load_lockfile, save_lockfile, set_entry, LockEntry, LockFile,
};
use crate::plus::skills::ops::Scope;
use crate::plus::skills::pyfs::{text_hash, write_text};
use crate::plus::skills::sync::rel_or_abs;
use std::path::{Path, PathBuf};

pub struct AgentSyncOptions<'a> {
    /// Project root, or the user's home directory in global mode.
    pub output_root: PathBuf,
    pub global_mode: bool,
    pub dry_run: bool,
    pub client_keys: Option<Vec<String>>,
    pub clock: &'a dyn Clock,
}

/// Extends `lockfile` (or a fresh one) with the agents section. Per-agent transpile failures
/// become lock warnings; nothing else in the lock is touched.
pub fn sync_agents(
    agents: &[Agent],
    lockfile: Option<LockFile>,
    opts: &AgentSyncOptions<'_>,
) -> Result<LockFile, String> {
    let mut lock = lockfile.unwrap_or_else(|| LockFile::new(opts.clock.now().isoformat()));
    let root = opts.output_root.as_path();
    if opts.global_mode {
        lock.scope = "global".into();
        lock.output_root = root.to_string_lossy().into_owned();
    }

    let selected: Vec<Box<dyn AgentTranspiler>> = all_agent_transpilers()
        .into_iter()
        .filter(|t| {
            opts.client_keys
                .as_ref()
                .is_none_or(|k| k.is_empty() || k.iter().any(|c| c == t.client_key()))
        })
        .filter(|t| !(opts.global_mode && AGENT_PROJECT_ONLY.contains(&t.client_key())))
        .collect();
    let (append, per_file): (Vec<_>, Vec<_>) = selected
        .iter()
        .partition(|t| AGENT_APPEND_MODE.contains(&t.client_key()));

    for agent in agents {
        let mut entry = LockEntry::new(agent.version(), text_hash(&agent.source_path)?);
        for t in &per_file {
            let key = t.client_key();
            match t.transpile(agent, root) {
                Ok(result) => {
                    entry.clients_synced.push(key.to_string());
                    entry.warnings.extend(result.warnings.iter().cloned());
                    if !opts.dry_run {
                        if let Err(e) = write_text(&result.output_path, &result.content) {
                            entry
                                .warnings
                                .push(format!("{key}: transpilation failed: {e}"));
                            continue;
                        }
                    }
                    entry.push_output(key, rel_or_abs(&result.output_path, root));
                }
                Err(e) => entry
                    .warnings
                    .push(format!("{key}: transpilation failed: {e}")),
            }
        }
        set_entry(&mut lock.agents, agent.name(), entry);
    }

    for t in &append {
        let Some(Ok(result)) = t.transpile_all(agents, root) else {
            continue;
        };
        let rel = rel_or_abs(&result.output_path, root);
        for agent in agents {
            if let Some(entry) = get_entry_mut(&mut lock.agents, agent.name()) {
                entry.clients_synced.push(t.client_key().to_string());
                entry.warnings.extend(result.warnings.iter().cloned());
                entry.push_output(t.client_key(), rel.clone());
            }
        }
        if !opts.dry_run {
            write_text(&result.output_path, &result.content)?;
        }
    }
    Ok(lock)
}

pub struct ScopedSync {
    pub lock: LockFile,
    pub scope: Scope,
}

/// Syncs `agents` under the user-level scope (outputs under `~/`, the lock beside the registry)
/// or the project one (both in the repository), extending the lock that scope already has and
/// saving it unless this is a dry run.
pub fn sync_scoped(
    repo: &Path,
    agents: &[Agent],
    global: bool,
    dry_run: bool,
    client_keys: Option<Vec<String>>,
    clock: &dyn Clock,
) -> Result<ScopedSync, String> {
    let scope = Scope::new(global, repo)?;
    let opts = AgentSyncOptions {
        output_root: scope.output_root.clone(),
        global_mode: global,
        dry_run,
        client_keys,
        clock,
    };
    let lock = sync_agents(agents, load_lockfile(&scope.lock_dir), &opts)?;
    if !dry_run {
        save_lockfile(&scope.lock_dir, &lock)?;
    }
    Ok(ScopedSync { lock, scope })
}
