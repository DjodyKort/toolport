//! mcpm to Toolport registry mapping (MCP-1): pure functions, no vault or file side effects.

mod clients;
mod ids;
mod launch;
#[cfg(test)]
mod launch_tests;
#[cfg(test)]
mod launch_venv_tests;
mod name_map;
#[cfg(test)] mod name_map_prop_tests;
#[cfg(test)]
mod name_map_tests;
#[cfg(test)]
mod odh_tests;
mod rename_refs;
#[cfg(test)] mod rename_refs_prop_tests;
#[cfg(test)]
mod rename_refs_tests;
#[cfg(test)]
mod rename_refs_wildcard_tests;
mod run;
#[cfg(test)]
mod run_tests;
mod servers;
#[cfg(test)]
mod tests;

pub use clients::{map_clients, ClientConfig, ClientEntries, ClientMapping, ClientSkip};
pub use ids::{exposed_prefix, MAX_TOOL_NAME_LEN, TOOL_NAME_PREFIX};
pub use launch::{relocate_entry, screen_entry};
pub use name_map::{name_map, name_map_handler, NameMap};
pub use rename_refs::{rename_refs, rename_refs_handler};
pub use run::{load_input, run, run_handler, RunOptions};
pub use servers::{map_servers, MappedServer, McpmInput, SecretWrite, Warning, IMPORT_SOURCE};

#[cfg(test)]
pub use {
    ids::{short_id, tool_name_fits},
    name_map::{build_name_map, ToolManifest},
    rename_refs::rewrite_text,
    run::{Action, Plan},
};

use crate::registry::Profile;
use serde::Serialize;
#[cfg(test)]
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub servers: Vec<MappedServer>,
    pub profiles: Vec<Profile>,
    pub client_scopes: BTreeMap<String, String>,
    pub client_discovery: BTreeMap<String, String>,
    pub client_entries: Vec<ClientEntries>,
    pub skipped_clients: Vec<ClientSkip>,
    pub warnings: Vec<Warning>,
}

impl Mapping {
    pub fn secret_writes(&self) -> Vec<&SecretWrite> {
        self.servers.iter().flat_map(|s| &s.secrets).collect()
    }

    #[cfg(test)]
    pub fn golden(&self) -> Value {
        let servers: Vec<Value> = self
            .servers
            .iter()
            .map(|s| {
                json!({
                    "entry": s.entry,
                    "vaultKeys": s.secrets.iter().map(SecretWrite::vault_key).collect::<Vec<_>>(),
                    "oauthViaGateway": s.oauth_via_gateway,
                })
            })
            .collect();
        json!({
            "servers": servers,
            "profiles": self.profiles,
            "clientScopes": self.client_scopes,
            "clientDiscovery": self.client_discovery,
            "clientEntries": self.client_entries,
            "skippedClients": self.skipped_clients,
            "warnings": self.warnings,
        })
    }
}

pub fn map_all(input: &McpmInput, clients: &[ClientConfig]) -> Mapping {
    let (servers, mut warnings) = map_servers(input);
    let ClientMapping {
        entries,
        profiles,
        client_scopes,
        client_discovery,
        skipped,
        warnings: client_warnings,
    } = map_clients(&servers, input, clients);
    warnings.extend(client_warnings);
    Mapping {
        servers,
        profiles,
        client_scopes,
        client_discovery,
        client_entries: entries,
        skipped_clients: skipped,
        warnings,
    }
}
