//! The registry as the fork logic reads and edits it: plain data and two
//! traits, so the logic never sees upstream's `Registry`.
//!
//! An access set is what upstream calls a profile since registry version 3: a
//! named list of server ids a client can be scoped to. A server is reachable
//! through a set when its global switch is on and it is listed in the set;
//! with no explicit default set, the default is every enabled server.

use serde::{Deserialize, Serialize};

use crate::fork_data::{DirectEntry, SelfMcpRecord};
use crate::{CatalogEntryRecord, EnvEntry, ServerRecord, ServerSpec};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessSetInfo {
    pub id: String,
    pub name: String,
    pub server_ids: Vec<String>,
}

/// Read side. Implemented over a read-only load (nothing recovers or migrates
/// on disk) and over the registry inside a transaction.
pub trait RegistryView {
    /// The registry file's schema version; 3 or more has global switches.
    fn version(&self) -> u32;

    /// `enabled` is the global switch on version 3 and up, and membership in
    /// any profile below.
    fn servers(&self) -> Result<Vec<ServerRecord>, String>;

    fn access_sets(&self) -> Vec<AccessSetInfo>;

    /// The explicit set new connections get; `None` means every enabled server, which is also what
    /// a registry below version 3 means.
    fn default_access(&self) -> Option<String>;

    /// Ids of the existing access sets clients are scoped to, sorted, once each.
    fn client_access_sets(&self) -> Vec<String>;

    fn selfmcp_record(&self) -> Result<Option<SelfMcpRecord>, String>;

    /// What `plus.directEntries` records: client id, then entry name.
    fn direct_entries(&self) -> Result<DirectEntries, String>;
}

pub type DirectEntries =
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, DirectEntry>>;

/// Write side, run inside one lock, load and save. The edit is dropped when the
/// closure returns an error.
pub trait RegistryTx: RegistryView {
    /// Adds a disabled stdio server and returns its id.
    fn add_stdio_server(
        &mut self,
        name: &str,
        command: &str,
        source: &str,
    ) -> Result<String, String>;

    /// Makes the server a stdio one running `command`; other fields stay.
    fn repoint_stdio_server(&mut self, server_id: &str, command: &str) -> Result<(), String>;

    fn remove_server(&mut self, server_id: &str) -> Result<(), String>;

    /// The global switch.
    fn set_server_enabled(&mut self, server_id: &str, enabled: bool) -> Result<(), String>;

    /// Lists or unlists a server in an access set.
    fn set_access_server(
        &mut self,
        access_set_id: &str,
        server_id: &str,
        included: bool,
    ) -> Result<(), String>;

    fn set_selfmcp_record(&mut self, record: Option<SelfMcpRecord>) -> Result<(), String>;

    /// Adds a server through upstream's own add and returns its id. Upstream switches the new
    /// server on and lists it in the default access set when it needs no credential.
    fn add_server(&mut self, spec: &ServerSpec) -> Result<String, String>;

    /// Adds a curated catalog entry as upstream's catalog add does: switched on only when the
    /// entry needs no credential.
    fn add_catalog_server(&mut self, entry: &CatalogEntryRecord) -> Result<String, String>;

    /// Edits a server through upstream's own edit, which also drops a launch
    /// recipe that no longer matches the command.
    fn update_server(&mut self, server_id: &str, spec: &ServerSpec) -> Result<(), String>;

    /// Replaces the environment entries of a server.
    fn set_server_env(&mut self, server_id: &str, env: Vec<EnvEntry>) -> Result<(), String>;

    /// Off removes the key.
    fn set_forward_instructions(&mut self, server_id: &str, enabled: bool) -> Result<(), String>;

    /// Off removes the key.
    fn set_declare_client_capabilities(
        &mut self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String>;
}
