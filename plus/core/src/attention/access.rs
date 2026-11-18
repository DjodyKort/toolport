//! What the Access check reads from the registry, and the ledger that tells a server added after
//! the access sets last changed from one that was always unreached.

use std::collections::BTreeMap;

use toolport_plus_api::Host;

use super::feed_access::{AccessInputs, UnreachedServer};
use super::store::{AccessLedger, Store};
use crate::op::OpError;
use crate::reach::{self, Filter};

struct Registry {
    servers: Vec<String>,
    sets: BTreeMap<String, String>,
}

fn registry(host: &dyn Host) -> Result<Option<Registry>, String> {
    let Some(view) = host.read_view()? else {
        return Ok(None);
    };
    Ok(Some(Registry {
        servers: view.servers()?.into_iter().map(|s| s.id).collect(),
        sets: view
            .access_sets()
            .into_iter()
            .map(|set| {
                let fingerprint = AccessLedger::fingerprint(&set.name, &set.server_ids);
                (set.id, fingerprint)
            })
            .collect(),
    }))
}

pub fn inputs(host: &dyn Host, store: Option<&Store>) -> Result<AccessInputs, String> {
    let report = reach::assess(host, Filter::default()).map_err(|e| e.message)?;
    let sets = registry(host)?.map(|r| r.sets).unwrap_or_default();
    let ledger = store.map(|s| s.access.clone()).unwrap_or_default();
    let now = host.now_unix();
    Ok(AccessInputs {
        unreached: report
            .servers
            .iter()
            .filter(|server| server.unreached)
            .map(|server| UnreachedServer {
                id: server.id.clone(),
                name: server.name.clone(),
                added: ledger.added_after_sets(&server.id, &sets, now),
            })
            .collect(),
        folders: report
            .access_rows
            .into_iter()
            .filter(|row| row.id.starts_with("access:folder:"))
            .collect(),
    })
}

/// Writes down which servers and sets exist now. The notifier pass and the update check call it,
/// so the ledger follows the registry whether or not notifications are on; the list itself never
/// writes.
pub fn observe(host: &dyn Host) -> Result<(), OpError> {
    let failed = |message: String| OpError::failed("attention_store", message);
    let base = host
        .data_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "data directory could not be resolved"))?;
    let Some(current) = registry(host).map_err(failed)? else {
        return Ok(());
    };
    let mut store = Store::read(&base).map_err(failed)?;
    if store
        .access
        .observe(&current.servers, current.sets, host.now_unix())
    {
        store.write(&base).map_err(failed)?;
    }
    Ok(())
}
