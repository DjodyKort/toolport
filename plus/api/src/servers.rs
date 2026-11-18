//! What the server commands need of upstream's server records, its catalog and the screening it
//! keeps for environment entries.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ServerRecord;

/// One environment entry of a stdio server. Mirror of upstream's `EnvVar`: a secret entry holds no
/// value (it lives in the vault), a plain one keeps it inline.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default)]
    pub secret: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The fields a server is added or edited with: the shape upstream's own editor submits.
/// `inherit_env: None` keeps the saved choice on an edit and means off on an add.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerSpec {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub cwd: Option<String>,
    pub inherit_env: Option<bool>,
}

/// Mirror of upstream's `CatalogEntry` (camelCase), with the fields the fork does not read in `extra`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntryRecord {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub transport: String,
    #[serde(default)]
    pub source: String,
    #[serde(default, rename = "envKeys")]
    pub env_keys: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl CatalogEntryRecord {
    pub fn text(&self, key: &str) -> Option<&str> {
        self.extra.get(key).and_then(Value::as_str)
    }
}

impl ServerRecord {
    /// A list of strings upstream types itself (`args`, `disabledTools`); empty when absent.
    pub fn strings(&self, key: &str) -> Vec<String> {
        self.extra
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A boolean upstream types itself (`inheritEnv`); false when absent.
    pub fn switch(&self, key: &str) -> bool {
        self.extra
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// The environment entries, with the keys the fork does not read kept in each entry's `extra`.
    pub fn env(&self) -> Result<Vec<EnvEntry>, String> {
        match self.extra.get("env") {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|error| format!("server {}: env is not readable: {error}", self.id)),
        }
    }
}

/// Checks that live in upstream's code and that the fork applies to what it writes.
pub trait ServerRules {
    /// Whether `text` looks like a credential: a known token prefix, an inline `password=`
    /// assignment and the like.
    fn credential_text(&self, text: &str) -> bool;

    /// Whether the gateway would refuse to launch a process with this variable set (it
    /// preloads or injects code).
    fn spawn_env_refused(&self, key: &str, value: &str) -> bool;
}

/// One page of the MCP Registry's server list, mapped to catalog entries by upstream's own code.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegistryEntries {
    /// The latest, active version of each listed server, with its registry id.
    pub entries: Vec<(String, CatalogEntryRecord)>,
    /// Registry ids whose latest version is no longer active or that upstream cannot map to an
    /// entry.
    pub retired: Vec<String>,
}

/// Upstream's curated catalog.
pub trait CatalogAccess {
    /// Curated entries matching `query`, in the catalog's own order; all of them for an empty
    /// query. Never goes to the network.
    fn curated(&self, query: &str) -> Vec<CatalogEntryRecord>;

    /// The servers of one `GET /v0.1/servers` response, filtered and mapped the way upstream's
    /// live search does it.
    fn registry_entries(&self, body: &Value) -> RegistryEntries;
}

/// What happened to one instance of a server when a gateway process was asked to restart it. A
/// server whose working directory uses `${ROOT}` runs once per project folder: `root` names the
/// folder of such an instance. It is `None` for the server's one shared connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartOutcome {
    pub root: Option<String>,
    pub restarted: bool,
    pub detail: String,
}

/// The calls running on or waiting for one instance of a server; `root` as in [`RestartOutcome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallsInFlight {
    pub root: Option<String>,
    pub calls: usize,
}

/// What the restart listener needs of the router a gateway process owns.
pub trait GatewayRouter: Send + Sync {
    /// Whether this router holds an instance of the server, shared or for a project folder.
    fn holds_server(&self, server_id: &str) -> bool;

    /// The calls in flight on every instance of the server in this process; `None` when it is
    /// not held.
    fn calls_in_flight(&self, server_id: &str) -> Option<Vec<CallsInFlight>>;

    /// Replaces the connection of every instance of the server with a fresh one now. `None` when
    /// it is not held.
    fn restart_server(&self, server_id: &str) -> Option<Vec<RestartOutcome>>;

    /// Marks the tool, resource and prompt lists stale so connected clients read them again.
    fn mark_lists_stale(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert;
    use serde_json::json;

    #[test]
    fn an_env_entry_keeps_the_keys_the_fork_does_not_know() {
        let upstream = json!([
            {"key": "A", "value": "1", "secret": false, "localOnly": true},
            {"key": "B", "secret": true},
        ]);
        let entries: Vec<EnvEntry> = convert(&upstream).unwrap();
        assert_eq!(entries[0].extra["localOnly"], json!(true));
        assert_eq!(entries[1].value, None);
        let back: Value = convert(&entries).unwrap();
        assert_eq!(back, upstream);
    }

    #[test]
    fn a_catalog_record_round_trips_with_unknown_fields() {
        let upstream = json!({
            "name": "Stripe", "description": "d", "transport": "http", "command": null,
            "args": [], "url": "https://x.invalid/mcp", "envKeys": ["K"], "source": "curated",
            "homepage": null, "futureField": [1],
        });
        let record: CatalogEntryRecord = convert(&upstream).unwrap();
        assert_eq!(record.env_keys, ["K"]);
        assert_eq!(record.text("description"), Some("d"));
        let back: Value = convert(&record).unwrap();
        assert_eq!(back, upstream);
    }

    #[test]
    fn server_record_reads_typed_upstream_fields_from_extra() {
        let record: ServerRecord = convert(&json!({
            "id": "a", "name": "A", "args": ["x", "y"], "inheritEnv": true,
            "env": [{"key": "K", "value": "v", "secret": false}],
        }))
        .unwrap();
        assert_eq!(record.strings("args"), ["x", "y"]);
        assert!(record.switch("inheritEnv") && !record.switch("hidePrompts"));
        assert_eq!(record.env().unwrap()[0].value.as_deref(), Some("v"));
        assert!(ServerRecord::default().env().unwrap().is_empty());
    }
}
