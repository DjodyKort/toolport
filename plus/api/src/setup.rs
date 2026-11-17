//! What setup export and restore need of upstream's registry beyond the mirror types: the servers
//! in upstream's own per-server export rules, access sets and client routing as one document, the
//! whitelisted registry settings and the vaulted static values.
//!
//! Every method has a default that reports the capability as missing, so a test host only
//! implements what its test exercises.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const MISSING: &str = "setup export is not available in this host";

/// A server a restore added, held disabled until it is reviewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedServer {
    pub name: String,
    pub id: String,
}

/// One value the vault holds that was typed in Toolport. `kind` is `env` (a secret environment
/// entry), `launch` (a launch argument input) or `token` (the access token of a remote server).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticSecret {
    pub server: String,
    pub key: String,
    pub kind: String,
    pub value: String,
}

impl std::fmt::Debug for StaticSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticSecret")
            .field("server", &self.server)
            .field("key", &self.key)
            .field("kind", &self.kind)
            .field("value", &"<hidden>")
            .finish()
    }
}

pub trait SetupHost {
    /// Every server except the gateway entry and those kept local, as upstream's per-server
    /// export writes it (`personal_sync::export`), plus a `forkSwitches` object with the fork's
    /// own per-server settings. Environment values are blank unless marked portable, and entries
    /// that override the execution environment are dropped.
    fn setup_servers(&self) -> Result<Vec<Value>, String> {
        Err(MISSING.into())
    }

    /// Adds the given documents (the shape of [`SetupHost::setup_servers`]) through upstream's
    /// import: servers whose name exists are skipped, secrets are blanked and the new ones are
    /// held disabled. Fork switches are applied to the new ones. Returns what was added.
    fn setup_add_servers(&self, _servers: &[Value]) -> Result<Vec<AddedServer>, String> {
        Err(MISSING.into())
    }

    /// Access sets (servers by name), default access, the access set of each client, the discovery
    /// mode per client, folder routing and the clients Toolport has connected.
    fn setup_access(&self) -> Result<Value, String> {
        Err(MISSING.into())
    }

    /// Merges a document of [`SetupHost::setup_access`]: adds sets that do not exist (by name),
    /// scopes and discovery modes of clients that have none, and folder routes that are absent.
    /// Nothing existing is replaced. Returns what was added, for [`SetupHost::setup_unmerge_access`].
    fn setup_merge_access(&self, _doc: &Value) -> Result<Value, String> {
        Err(MISSING.into())
    }

    /// Removes what a merge added, as far as it is still as the merge left it.
    fn setup_unmerge_access(&self, _added: &Value) -> Result<(), String> {
        Err(MISSING.into())
    }

    /// The registry settings that travel between machines (safety, discovery, Code Mode, ...).
    fn setup_settings(&self) -> Result<Map<String, Value>, String> {
        Err(MISSING.into())
    }

    /// Sets those registry settings; returns the previous value of each key that was set, `null`
    /// where there was none.
    fn setup_apply_settings(
        &self,
        _changes: &Map<String, Value>,
    ) -> Result<Map<String, Value>, String> {
        Err(MISSING.into())
    }

    /// The vaulted values typed in Toolport. Plain text: callers seal them before they leave.
    fn setup_secrets(&self) -> Result<Vec<StaticSecret>, String> {
        Err(MISSING.into())
    }

    /// Stores one value back through the vault, on the server with this name.
    fn setup_store_secret(&self, _secret: &StaticSecret) -> Result<(), String> {
        Err(MISSING.into())
    }
}
