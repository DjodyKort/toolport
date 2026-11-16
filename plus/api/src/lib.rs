//! Contract between the fork logic (`toolport-plus`) and upstream Toolport.
//!
//! This crate depends on nothing from upstream. Conduit implements the traits
//! in `src/plus_adapter.rs`; the fork logic only ever sees these items.

use std::path::PathBuf;

use serde_json::{Map, Value};

pub mod fork_data;
pub mod http;
pub mod memory;
pub mod mirror;
pub mod secrets;

pub use fork_data::{
    plus_ops, AuthProbe, DirectEntry, ForkServerFields, McpmSource, RegistryPlus, SelfMcpRecord,
    ServerPlus,
};
pub use http::{HttpAccess, HttpBody, HttpError, HttpMethod, HttpRequest, HttpResponse};
pub use memory::{MemoryRegistry, MemorySecrets, ScriptedHttp};
pub use mirror::{convert, ServerRecord};
pub use secrets::{SecretsHost, TrustInputs};

/// Registry access for the fork logic.
///
/// Reads return mirror types. Writes are operations, never a whole mirrored
/// registry handed back: the adapter mutates upstream's own entry in place, so
/// fields the fork does not know survive. Every write takes the registry's
/// cross-process lock for its whole load, change and save.
pub trait RegistryAccess {
    fn registry_path(&self) -> Option<PathBuf>;

    fn list_servers(&self) -> Result<Vec<ServerRecord>, String>;

    /// `None` when no server has `server_id`.
    fn get_server(&self, server_id: &str) -> Result<Option<ServerRecord>, String>;

    fn profile_count(&self) -> Result<usize, String>;

    /// Fails when no server has `server_id`.
    fn set_server_enabled(&self, server_id: &str, enabled: bool) -> Result<(), String>;

    /// The fork's top-level `plus` data; empty when the registry has none.
    fn plus_data(&self) -> Result<RegistryPlus, String>;

    /// Replaces `plus.selfmcp`, or removes it with `None`.
    fn set_selfmcp_record(&self, record: Option<SelfMcpRecord>) -> Result<(), String>;

    /// Adds profile ids to `plus.selfmcp.enabledIn`, keeping order and skipping
    /// ids already listed.
    fn add_selfmcp_enabled_in(&self, profile_ids: &[String]) -> Result<(), String>;

    fn set_selfmcp_opt_out(&self, opt_out: bool) -> Result<(), String>;

    /// Writes one record under `plus.directEntries.<client>.<entry>`.
    fn set_direct_entry(
        &self,
        client: &str,
        entry: &str,
        record: DirectEntry,
    ) -> Result<(), String>;

    /// True when the record existed.
    fn clear_direct_entry(&self, client: &str, entry: &str) -> Result<bool, String>;

    /// Sets or, with `None`, removes `plus.authProbe` of a server.
    fn set_server_auth_probe(
        &self,
        server_id: &str,
        probe: Option<AuthProbe>,
    ) -> Result<(), String>;

    /// Replaces `mcpmSource` of a server, or removes it with `None`.
    fn set_server_mcpm_source(
        &self,
        server_id: &str,
        source: Option<McpmSource>,
    ) -> Result<(), String>;

    /// Sets the given keys inside `mcpmSource`, creating it when missing and
    /// leaving its other keys alone.
    fn merge_server_mcpm_source(
        &self,
        server_id: &str,
        fields: Map<String, Value>,
    ) -> Result<(), String>;

    fn set_server_mcpm_oauth(&self, server_id: &str, oauth: Option<bool>) -> Result<(), String>;

    fn set_server_mcpm_tags(
        &self,
        server_id: &str,
        tags: Option<Vec<String>>,
    ) -> Result<(), String>;

    /// Off removes the key, as the setting has no value to store when off.
    fn set_server_forward_instructions(&self, server_id: &str, enabled: bool)
        -> Result<(), String>;

    /// Off removes the key.
    fn set_server_declare_client_capabilities(
        &self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String>;

    fn set_server_max_request_timeout_ms(
        &self,
        server_id: &str,
        timeout_ms: Option<u64>,
    ) -> Result<(), String>;
}
