//! Contract between the fork logic (`toolport-plus`) and upstream Toolport.
//!
//! This crate depends on nothing from upstream. Conduit implements the traits
//! in `src/plus_adapter.rs`; the fork logic only ever sees these items.

use std::path::PathBuf;

use serde_json::{Map, Value};

pub mod auth;
pub mod context;
pub mod fork_data;
pub mod http;
pub mod memory;
pub mod mirror;
pub mod secrets;
pub mod servers;
pub mod settings;
pub mod tasks;
pub mod view;

pub use auth::{AuthAccess, AuthChild, ConsentSink, LaunchError, LaunchFault};
pub use context::ContextHost;
pub use fork_data::{
    plus_ops, AuthProbe, DirectEntry, ForkServerFields, McpmSource, RegistryPlus, SelfMcpRecord,
    ServerPlus,
};
pub use http::{HttpAccess, HttpBody, HttpError, HttpMethod, HttpRequest, HttpResponse};
pub use memory::{
    MemoryGateway, MemoryHost, MemoryRegistry, MemorySecrets, MemoryState, ScriptedHttp,
};
pub use mirror::{convert, ServerRecord};
pub use secrets::{SecretsHost, TrustInputs, VaultAccess, HTTP_AUTH_KEY};
pub use servers::{
    CallsInFlight, CatalogAccess, CatalogEntryRecord, EnvEntry, GatewayRouter, RestartOutcome,
    ServerRules, ServerSpec,
};
pub use settings::SettingsStore;
pub use tasks::{
    FakeTaskHost, ProcessOutcome, ProcessSpec, RunApproval, RunDecision, SharedClockTaskHost,
    TaskHost,
};
pub use view::{AccessSetInfo, RegistryTx, RegistryView};

/// Written into every binary that forwards upstream code to the fork logic, so
/// `toolportctl doctor` can tell a gateway built with the fork seam from one built without.
pub const SEAM_MARKER: &str = "toolport-plus-seam/1";

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

    /// A read-only view that never recovers, migrates or writes on disk, so
    /// inspection leaves the data dir as it is. `Ok(None)` when no registry file exists.
    fn read_view(&self) -> Result<Option<Box<dyn RegistryView>>, String>;

    /// Runs `edit` inside the registry's lock on a fresh copy and saves the
    /// result. An error from `edit` saves nothing.
    fn transact(
        &self,
        edit: &mut dyn FnMut(&mut dyn RegistryTx) -> Result<(), String>,
    ) -> Result<(), String>;

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

    /// `None` removes the key.
    fn set_server_mode(&self, server_id: &str, mode: Option<&str>) -> Result<(), String>;
}

/// What the command line and the self-MCP server need from the process they run in.
pub trait HostEnv {
    /// Upstream's version, as `toolportctl status` reports it.
    fn app_version(&self) -> &'static str;

    /// Points the process at another data dir. Called before the first data
    /// dir lookup, which upstream memoises.
    fn use_data_dir(&self, dir: &std::path::Path);

    /// Whether upstream code in this process forwards to the fork logic.
    fn seam_live(&self) -> bool;

    /// Seconds since the Unix epoch; a test host moves it by hand.
    fn now_unix(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64)
    }

    /// Whether the desktop app is registered to start at login; `None` when this platform has
    /// no way to tell. Reads only.
    fn launch_at_login(&self) -> Option<bool> {
        None
    }
}

/// Everything a command or tool runs against.
pub trait Host:
    RegistryAccess
    + SecretsHost
    + VaultAccess
    + HostEnv
    + ServerRules
    + CatalogAccess
    + HttpAccess
    + AuthAccess
    + ContextHost
    + SettingsStore
    + TaskHost
{
}

impl<
        T: RegistryAccess
            + SecretsHost
            + VaultAccess
            + HostEnv
            + ServerRules
            + CatalogAccess
            + HttpAccess
            + AuthAccess
            + ContextHost
            + SettingsStore
            + TaskHost,
    > Host for T
{
}
