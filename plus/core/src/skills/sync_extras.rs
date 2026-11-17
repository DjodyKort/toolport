//! Agents and output styles in the same run as skills: each output file is planned, held,
//! journaled and recorded in the lock like a skill file, so one Apply writes them and one Undo
//! puts them back. Output styles here are the tier-1 sync (`styles sync`), not an applied style.

use super::agents::transpilers::{AGENT_APPEND_MODE, AGENT_PROJECT_ONLY};
use super::agents::{all_agent_transpilers, Agent};
use super::lock::{get_entry, get_entry_mut, set_entry, LockEntry, LockFile};
use super::pyfs::{text_hash, write_text};
use super::styles::transpilers::{all_style_transpilers, StyleTranspiler, Tier, STYLE_APPEND_MODE};
use super::styles::Style;
use super::sync::{
    compare, installed_hash, place_files, rel_or_abs, Ctx, Desired, Failure, Found, Kind, Placed,
};
use super::transpiler::TranspileResult;
use super::verify::{Finding, Verify};
use std::collections::BTreeSet;
use std::path::Path;

/// The agents and styles of the library, synced next to its skills.
#[derive(Clone, Debug, Default)]
pub struct Extras {
    pub agents: Vec<Agent>,
    pub styles: Vec<Style>,
}

fn wanted(clients: &Option<Vec<String>>, key: &str) -> bool {
    clients
        .as_ref()
        .is_none_or(|keys| keys.is_empty() || keys.iter().any(|c| c == key))
}

fn desired(result: TranspileResult, root: &Path) -> Desired {
    Desired::text(
        result.output_path.clone(),
        rel_or_abs(&result.output_path, root),
        result.content,
    )
}

fn tracked(entry: Option<&LockEntry>, client: &str) -> BTreeSet<String> {
    entry
        .and_then(|e| e.output_files.iter().find(|(k, _)| k == client))
        .map(|(_, rels)| rels.iter().cloned().collect())
        .unwrap_or_default()
}

struct Output {
    client: String,
    result: Result<TranspileResult, String>,
}

/// Plans and writes one item's files for each of its outputs and fills its lock entry; a client
/// that fails is a warning on the entry and a failed step of the run.
fn place_item(
    ctx: &mut Ctx<'_>,
    kind: Kind,
    name: &str,
    previous: Option<&LockEntry>,
    outputs: Vec<Output>,
    entry: &mut LockEntry,
) {
    for Output { client, result } in outputs {
        let placed = result.and_then(|out| {
            entry.clients_synced.push(client.clone());
            entry.warnings.extend(out.warnings.iter().cloned());
            let files = [desired(out, ctx.output_root)];
            place_files(
                ctx,
                Placed {
                    kind,
                    name,
                    client: &client,
                },
                &files,
                &tracked(previous, &client),
                previous,
                entry,
            )
        });
        if let Err(message) = placed {
            entry
                .warnings
                .push(format!("{client}: transpilation failed: {message}"));
            ctx.plan.failed.push(Failure {
                kind,
                name: name.to_string(),
                client,
                step: "files",
                message,
            });
        }
    }
}

fn carry(previous: Option<&LockEntry>, into: &mut Vec<(String, LockEntry)>, name: &str) {
    if let Some(entry) = previous {
        set_entry(into, name, entry.clone());
    }
}

/// An append-mode client holds every item in one file: it is written whole, outside the plan.
fn write_appended(
    ctx: &Ctx<'_>,
    result: Option<Result<TranspileResult, String>>,
    names: &[String],
    entries: &mut Vec<(String, LockEntry)>,
    client: &str,
) -> Result<(), String> {
    let Some(Ok(out)) = result else {
        return Ok(());
    };
    let rel = rel_or_abs(&out.output_path, ctx.output_root);
    for name in names {
        if let Some(entry) = get_entry_mut(entries, name) {
            entry.clients_synced.push(client.to_string());
            entry.warnings.extend(out.warnings.iter().cloned());
            entry.push_output(client, rel.clone());
        }
    }
    if !ctx.dry_run && !out.content.is_empty() {
        ctx.touch(&out.output_path)?;
        write_text(&out.output_path, &out.content)?;
    }
    Ok(())
}

pub(crate) fn sync_extras(
    ctx: &mut Ctx<'_>,
    lock: &mut LockFile,
    extras: &Extras,
    global: bool,
    clients: &Option<Vec<String>>,
) -> Result<(), String> {
    let previous_lock = ctx.previous;
    let root = ctx.output_root;

    let agent_clients: Vec<_> = all_agent_transpilers()
        .into_iter()
        .filter(|t| wanted(clients, t.client_key()))
        .filter(|t| !(global && AGENT_PROJECT_ONLY.contains(&t.client_key())))
        .collect();
    for agent in &extras.agents {
        let previous = previous_lock.and_then(|p| get_entry(&p.agents, agent.name()));
        if ctx.frozen(&Kind::Agent.scoped(agent.name())) {
            carry(previous, &mut lock.agents, agent.name());
            continue;
        }
        let mut entry = LockEntry::new(agent.version(), text_hash(&agent.source_path)?);
        let outputs = agent_clients
            .iter()
            .filter(|t| !AGENT_APPEND_MODE.contains(&t.client_key()))
            .map(|t| Output {
                client: t.client_key().to_string(),
                result: t.transpile(agent, root),
            })
            .collect();
        place_item(
            ctx,
            Kind::Agent,
            agent.name(),
            previous,
            outputs,
            &mut entry,
        );
        set_entry(&mut lock.agents, agent.name(), entry);
    }
    if ctx.only.is_none() {
        let names: Vec<String> = extras.agents.iter().map(|a| a.name().to_string()).collect();
        for t in agent_clients
            .iter()
            .filter(|t| AGENT_APPEND_MODE.contains(&t.client_key()))
        {
            let result = t.transpile_all(&extras.agents, root);
            write_appended(ctx, result, &names, &mut lock.agents, t.client_key())?;
        }
    }

    let style_clients: Vec<Box<dyn StyleTranspiler>> = all_style_transpilers()
        .into_iter()
        .filter(|t| t.tier() == Tier::Native && wanted(clients, t.client_key()))
        .collect();
    for style in &extras.styles {
        let previous = previous_lock.and_then(|p| get_entry(&p.styles, style.name()));
        if ctx.frozen(&Kind::Style.scoped(style.name())) {
            carry(previous, &mut lock.styles, style.name());
            continue;
        }
        let mut entry = LockEntry::new(style.version(), text_hash(&style.source_path)?);
        let outputs = style_clients
            .iter()
            .filter(|t| !STYLE_APPEND_MODE.contains(&t.client_key()))
            .map(|t| Output {
                client: t.client_key().to_string(),
                result: t.transpile(style, root),
            })
            .collect();
        place_item(
            ctx,
            Kind::Style,
            style.name(),
            previous,
            outputs,
            &mut entry,
        );
        set_entry(&mut lock.styles, style.name(), entry);
    }
    if ctx.only.is_none() {
        let names: Vec<String> = extras.styles.iter().map(|s| s.name().to_string()).collect();
        for t in style_clients
            .iter()
            .filter(|t| STYLE_APPEND_MODE.contains(&t.client_key()))
        {
            let result = t.transpile_all(&extras.styles, root);
            write_appended(ctx, result, &names, &mut lock.styles, t.client_key())?;
        }
    }
    Ok(())
}

/// Every output file the lock records for an agent or a style against what the library would
/// install, added to `out` the way the skills are checked.
pub fn verify_extras(
    out: &mut Verify,
    extras: &Extras,
    lock: &LockFile,
    root: &Path,
    only: Option<&[String]>,
) {
    let mut check = |name: &str, client: &str, entry: &LockEntry, file: Desired| {
        out.checked += 1;
        let finding = || Finding {
            skill: name.to_string(),
            client: client.to_string(),
            path: file.rel.clone(),
        };
        match compare(&file) {
            Found::Same | Found::Mode => {}
            Found::Missing => out.missing.push(finding()),
            Found::Different => {
                out.differing.push(finding());
                if entry
                    .file_hash(client, &file.rel)
                    .is_some_and(|hash| installed_hash(&file.abs).as_deref() != Some(hash))
                {
                    out.edited.push(finding());
                }
            }
        }
    };
    let transpilers = all_agent_transpilers();
    for agent in &extras.agents {
        let Some(entry) = get_entry(&lock.agents, agent.name()) else {
            continue;
        };
        for client in &entry.clients_synced {
            if only.is_some_and(|clients| !clients.contains(client)) {
                continue;
            }
            let Some(t) = transpilers.iter().find(|t| t.client_key() == client) else {
                continue;
            };
            if AGENT_APPEND_MODE.contains(&t.client_key()) {
                continue;
            }
            if let Ok(result) = t.transpile(agent, root) {
                check(agent.name(), client, entry, desired(result, root));
            }
        }
    }
    let transpilers = all_style_transpilers();
    for style in &extras.styles {
        let Some(entry) = get_entry(&lock.styles, style.name()) else {
            continue;
        };
        for client in &entry.clients_synced {
            if only.is_some_and(|clients| !clients.contains(client)) {
                continue;
            }
            let Some(t) = transpilers
                .iter()
                .find(|t| t.client_key() == client && t.tier() == Tier::Native)
            else {
                continue;
            };
            if STYLE_APPEND_MODE.contains(&t.client_key()) {
                continue;
            }
            if let Ok(result) = t.transpile(style, root) {
                check(style.name(), client, entry, desired(result, root));
            }
        }
    }
}
