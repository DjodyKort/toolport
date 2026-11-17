//! The clients a skills sync writes to when none was asked for. mcpm only ever synced the
//! clients it was pointed at, so a bare sync must not fan out to every transpiler: it repeats
//! the clients the scope's lock already holds, else it starts with claude-code. The lock the
//! sync writes lists exactly those clients, which is what keeps the choice for the next run.

use super::lock::{LockEntry, LockFile};
use super::transpiler::TranspilerRegistry;
use std::collections::BTreeSet;

pub const FALLBACK_CLIENT: &str = "claude-code";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientSource {
    Requested,
    Lock,
    Default,
}

impl ClientSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Lock => "lock",
            Self::Default => "default",
        }
    }
}

/// The registry's clients that some skill or rule of `lock` was synced to, in registry order;
/// agents and styles have their own sync paths and do not count. A lock that names no known
/// client is as good as none.
pub fn default_clients(
    lock: Option<&LockFile>,
    registry: &TranspilerRegistry,
) -> (Vec<String>, ClientSource) {
    let held: BTreeSet<&str> = lock
        .into_iter()
        .flat_map(|l| l.skills.iter().chain(l.rules.iter()))
        .flat_map(|(_, entry)| entry.clients_synced.iter().map(String::as_str))
        .collect();
    let known: Vec<String> = registry
        .all()
        .map(|t| t.client_key().to_string())
        .filter(|key| held.contains(key.as_str()))
        .collect();
    if known.is_empty() {
        (vec![FALLBACK_CLIENT.to_string()], ClientSource::Default)
    } else {
        (known, ClientSource::Lock)
    }
}

/// Fails when `keys` names a client that skills are not synced to.
pub fn ensure_supported<'a>(
    keys: impl IntoIterator<Item = &'a str>,
    registry: &TranspilerRegistry,
) -> Result<(), String> {
    let unknown: Vec<&str> = keys
        .into_iter()
        .filter(|key| registry.get(key).is_none())
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(format!(
        "skills are not synced to {}; supported clients: {}",
        unknown.join(", "),
        registry
            .all()
            .map(|t| t.client_key())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn recorded_clients(entry: &LockEntry) -> impl Iterator<Item = &str> {
    entry
        .output_files
        .iter()
        .map(|(client, _)| client.as_str())
        .chain(entry.file_hashes.iter().map(|(client, _)| client.as_str()))
}

/// Does some skill or rule of `lock` record outputs for `client`?
pub fn lock_records_outputs(lock: &LockFile, client: &str) -> bool {
    lock.skills
        .iter()
        .chain(lock.rules.iter())
        .any(|(_, entry)| recorded_clients(entry).any(|c| c == client))
}

/// Carries what `previous` recorded for clients the registry no longer has into `lock`, so a
/// later clean can still delete those files. A skill that left the library keeps an entry that
/// lists only those outputs.
pub fn carry_retired_clients(
    previous: &LockFile,
    lock: &mut LockFile,
    registry: &TranspilerRegistry,
) {
    for (prev_map, new_map) in [
        (&previous.skills, &mut lock.skills),
        (&previous.rules, &mut lock.rules),
    ] {
        for (name, prev) in prev_map {
            let retired = |client: &str| registry.get(client).is_none();
            let outputs: Vec<_> = prev
                .output_files
                .iter()
                .filter(|(client, _)| retired(client))
                .cloned()
                .collect();
            let hashes: Vec<_> = prev
                .file_hashes
                .iter()
                .filter(|(client, _)| retired(client))
                .cloned()
                .collect();
            if outputs.is_empty() && hashes.is_empty() {
                continue;
            }
            if super::lock::get_entry(new_map, name).is_none() {
                let mut stub = LockEntry::new(prev.version.clone(), prev.hash.clone());
                stub.source = prev.source.clone();
                stub.warnings =
                    vec!["no longer in the library; only retired clients' outputs remain".into()];
                super::lock::set_entry(new_map, name, stub);
            }
            if let Some(entry) = super::lock::get_entry_mut(new_map, name) {
                entry.output_files.retain(|(client, _)| !retired(client));
                entry.file_hashes.retain(|(client, _)| !retired(client));
                entry.output_files.extend(outputs);
                entry.file_hashes.extend(hashes);
            }
        }
    }
}

/// Forgets what `lock` recorded for `client`.
pub fn drop_client(lock: &mut LockFile, client: &str) {
    for (_, entry) in lock.skills.iter_mut().chain(lock.rules.iter_mut()) {
        entry.clients_synced.retain(|c| c != client);
        entry.output_files.retain(|(c, _)| c != client);
        entry.file_hashes.retain(|(c, _)| c != client);
        entry.hooks_installed.retain(|(c, _)| c != client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::lock::{set_entry, LockEntry};
    use crate::skills::transpilers::registry;

    fn synced(clients: &[&str]) -> LockEntry {
        let mut entry = LockEntry::new(None, "h".into());
        entry.clients_synced = clients.iter().map(|c| c.to_string()).collect();
        entry
    }

    fn pick(lock: Option<&LockFile>) -> (Vec<String>, ClientSource) {
        default_clients(lock, &registry())
    }

    #[test]
    fn only_the_registered_clients_are_supported() {
        let registry = registry();
        assert_eq!(
            ensure_supported(["claude-code", "agents-md"], &registry),
            Ok(())
        );
        let err = ensure_supported(["claude-code", "cursor", "zed"], &registry).unwrap_err();
        assert_eq!(
            err,
            "skills are not synced to cursor, zed; supported clients: agents-md, claude-code"
        );
    }

    #[test]
    fn no_lock_means_claude_code() {
        assert_eq!(
            pick(None),
            (vec!["claude-code".to_string()], ClientSource::Default)
        );
        let empty = LockFile::new("t".into());
        assert_eq!(pick(Some(&empty)).1, ClientSource::Default);
    }

    #[test]
    fn skills_and_rules_decide_in_registry_order() {
        let mut lock = LockFile::new("t".into());
        set_entry(&mut lock.skills, "a", synced(&["agents-md"]));
        set_entry(&mut lock.rules, "r", synced(&["claude-code", "agents-md"]));
        let (clients, source) = pick(Some(&lock));
        assert_eq!(source, ClientSource::Lock);
        let registry = registry();
        let order: Vec<String> = registry
            .all()
            .map(|t| t.client_key().to_string())
            .filter(|k| k == "claude-code" || k == "agents-md")
            .collect();
        assert_eq!(clients, order);
    }

    #[test]
    fn agents_styles_and_unknown_keys_do_not_count() {
        let mut lock = LockFile::new("t".into());
        set_entry(&mut lock.agents, "a", synced(&["cursor"]));
        set_entry(&mut lock.styles, "s", synced(&["cursor"]));
        set_entry(&mut lock.skills, "k", synced(&["no-such-client"]));
        assert_eq!(
            pick(Some(&lock)),
            (vec!["claude-code".to_string()], ClientSource::Default)
        );
    }
}
