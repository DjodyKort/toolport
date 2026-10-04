//! The clients a skills sync writes to when none was asked for. mcpm only ever synced the
//! clients it was pointed at, so a bare sync must not fan out to every transpiler: it repeats
//! the clients the scope's lock already holds, else it starts with claude-code. The lock the
//! sync writes lists exactly those clients, which is what keeps the choice for the next run.

use super::lock::LockFile;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::skills::lock::{set_entry, LockEntry};
    use crate::plus::skills::transpilers::registry_with_home;

    fn synced(clients: &[&str]) -> LockEntry {
        let mut entry = LockEntry::new(None, "h".into());
        entry.clients_synced = clients.iter().map(|c| c.to_string()).collect();
        entry
    }

    fn pick(lock: Option<&LockFile>) -> (Vec<String>, ClientSource) {
        default_clients(lock, &registry_with_home(None))
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
        set_entry(&mut lock.skills, "a", synced(&["cursor"]));
        set_entry(&mut lock.rules, "r", synced(&["claude-code", "cursor"]));
        let (clients, source) = pick(Some(&lock));
        assert_eq!(source, ClientSource::Lock);
        let registry = registry_with_home(None);
        let order: Vec<String> = registry
            .all()
            .map(|t| t.client_key().to_string())
            .filter(|k| k == "claude-code" || k == "cursor")
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
