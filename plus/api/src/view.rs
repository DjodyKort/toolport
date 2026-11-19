//! The registry as the fork logic reads and edits it: plain data and two
//! traits, so the logic never sees upstream's `Registry`.
//!
//! An access set is what upstream calls a profile since registry version 3: a
//! named list of server ids a client can be scoped to. A server is reachable
//! through a set when its global switch is on and it is listed in the set;
//! with no explicit default set, the default is every enabled server.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

    /// The default access, every client scope and every folder mapping, each resolved the way the
    /// gateway resolves it and with the enabled servers it reaches. The default access comes
    /// first, then clients by id, then folders in the registry's order. A client with no entry
    /// follows the default access and is not listed.
    fn scope_reach(&self) -> Vec<ScopeReach>;

    /// The folder mapping the gateway applies to the root a client reports, as upstream's
    /// `profile_for_root` picks it: the reported text is matched as a string on a path boundary,
    /// separators and a trailing slash are ignored, the longest mapped folder wins, and the disk is
    /// never consulted, so a symlinked spelling of a mapped folder matches nothing. `None` when no
    /// mapping covers the root.
    fn folder_route_for(&self, root: &str) -> Option<ScopeReach>;

    /// One access set as the registry file stores it: id, name, servers, per-server tool lists,
    /// instructions and the fields this build does not know. `None` when no set has the id.
    fn access_set_record(&self, access_set_id: &str) -> Option<Value>;

    /// What `plus.directEntries` records: client id, then entry name.
    fn direct_entries(&self) -> Result<DirectEntries, String>;

    /// A client's own discovery-mode override (`full`, `lazy` or `grouped`); `None` inherits the
    /// global mode.
    fn client_discovery(&self, client_id: &str) -> Option<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeKind {
    /// Clients without a scope of their own.
    Default,
    /// A client id with an entry in `client_scopes`.
    Client,
    /// A folder mapping in `folder_profiles`.
    Folder,
}

/// Where one scope resolves to by upstream's own rules, and the enabled servers it reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeReach {
    pub kind: ScopeKind,
    /// Empty for the default access, else the client id or the folder path.
    pub key: String,
    /// The stored set reference (id or name); empty when the scope follows the default access.
    pub reference: String,
    /// The access set it resolves to; `None` when it reaches every enabled server (no explicit
    /// default access) or when the reference names no set.
    pub set_id: Option<String>,
    /// The reference names no existing set, so the scope reaches nothing.
    pub dangling: bool,
    /// Ids of the enabled servers reached, in registry order.
    pub server_ids: Vec<String>,
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

    /// Creates an empty access set through upstream's own create (name rules included) and
    /// returns its id.
    fn create_access_set(&mut self, name: &str) -> Result<String, String>;

    /// Deletes a set through upstream's own removal, which refuses the default access and the
    /// last set. Clients and folders that still name it are left dangling and reach nothing.
    fn delete_access_set(&mut self, access_set_id: &str) -> Result<(), String>;

    /// Puts a set back exactly as [`RegistryView::access_set_record`] returned it, keeping its id.
    /// Refused when a set with that id or name exists. Returns the id.
    fn restore_access_set(&mut self, record: Value) -> Result<String, String>;

    /// Makes a set the explicit default access; `None` returns to "every enabled server".
    fn set_default_access(&mut self, access_set_id: Option<&str>) -> Result<(), String>;

    /// Narrows the tools a set exposes from one server; `None` restores all tools.
    fn set_access_tools(
        &mut self,
        access_set_id: &str,
        server_id: &str,
        tools: Option<Vec<String>>,
    ) -> Result<(), String>;

    /// Maps a project folder to a set, replacing an earlier mapping of the same folder.
    fn set_folder_route(&mut self, path: &str, access_set_id: &str) -> Result<(), String>;

    fn remove_folder_route(&mut self, path: &str) -> Result<(), String>;

    fn set_selfmcp_record(&mut self, record: Option<SelfMcpRecord>) -> Result<(), String>;

    /// Binds a client id to an access set through upstream's `set_client_scope`; `None` removes
    /// the binding, so the client falls back to `TOOLPORT_PROFILE` and then the default access.
    fn set_client_scope(
        &mut self,
        client_id: &str,
        access_set_id: Option<&str>,
    ) -> Result<(), String>;

    /// Pins a client's discovery mode (`full`, `lazy`, `grouped`); `None` clears the override.
    fn set_client_discovery(&mut self, client_id: &str, mode: Option<&str>) -> Result<(), String>;

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
    fn set_serial(&mut self, server_id: &str, enabled: bool) -> Result<(), String>;

    /// Off removes the key.
    fn set_declare_client_capabilities(
        &mut self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String>;
}
