//! What the fork logic needs from the AI clients on this machine: which entries a client's own
//! config holds and one writer for a single named entry. Upstream implements it over its client
//! detection and its journalled writer; env values of entries never cross this boundary.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One entry of a client's own server list. `env_keys` are names only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientEntryInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default)]
    pub env_keys: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub config_path: String,
    #[serde(default)]
    pub config_exists: bool,
    #[serde(default)]
    pub app_present: bool,
    /// The client lists its servers through an account, not its file.
    #[serde(default)]
    pub uses_connectors: bool,
    #[serde(default)]
    pub gateway_installed: bool,
    #[serde(default)]
    pub entries: Vec<ClientEntryInfo>,
    /// Set when the config exists and could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Why Toolport cannot write a direct entry for this client; `None` when it can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_unsupported: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An entry to put into a client file. `env` is non-secret by construction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntrySpec {
    #[serde(default)]
    pub name: String,
    /// `stdio`, `http` or `sse`.
    #[serde(default)]
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntryWrite {
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// Hash of the file after the write; what a later plan compares against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A stdio server's launch line as the gateway would run it, secrets resolved from the vault.
/// `env` is the complete environment of the child process.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
}

impl std::fmt::Debug for LaunchCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<&str> = self.env.iter().map(|(key, _)| key.as_str()).collect();
        f.debug_struct("LaunchCommand")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env_keys", &keys)
            .field("cwd", &self.cwd)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientWriteError {
    /// The file differs from the revision the plan saw.
    Conflict(String),
    Failed(String),
}

impl ClientWriteError {
    pub fn message(&self) -> &str {
        match self {
            Self::Conflict(message) | Self::Failed(message) => message,
        }
    }
}

pub trait ClientsHost {
    /// Every client this build knows, installed or not.
    fn detect_clients(&self) -> Vec<ClientInfo>;

    /// Hash of the client's config text; `None` when the file does not exist.
    fn client_revision(&self, client_id: &str) -> Result<Option<String>, String>;

    /// The entry as it would stand in the client's file, for a plan.
    fn render_entry(&self, client_id: &str, spec: &EntrySpec) -> Result<String, String>;

    /// Upserts (`Some`) or removes (`None`) the entry called `name`, inside upstream's lock and
    /// journal. With `expect_revision` the write is refused when the file changed since.
    fn write_entry(
        &self,
        client_id: &str,
        name: &str,
        spec: Option<&EntrySpec>,
        expect_revision: Option<&Option<String>>,
    ) -> Result<EntryWrite, ClientWriteError>;

    /// The installed `toolportctl` a launcher entry runs; `None` when none can be found.
    fn launcher_command(&self) -> Option<String>;

    /// Non-secret variables a launcher entry carries so it finds the same data directory.
    fn launcher_env(&self) -> BTreeMap<String, String>;

    /// The command a stdio server runs with, built the way the gateway builds it: the same
    /// screening of command, arguments and environment, secrets read from the vault now. An error
    /// is the message to show; it holds no secret value.
    fn prepare_launch(&self, server_id: &str) -> Result<LaunchCommand, String>;
}

/// Clients in memory, for tests of the logic above the writer.
#[derive(Debug, Default)]
pub struct MemoryClients {
    pub clients: RefCell<Vec<ClientInfo>>,
    pub launcher: Option<String>,
    pub env: BTreeMap<String, String>,
    pub writes: RefCell<Vec<(String, String, Option<EntrySpec>)>>,
    pub launches: BTreeMap<String, Result<LaunchCommand, String>>,
}

impl MemoryClients {
    pub fn with_client(id: &str, name: &str) -> Self {
        let client = ClientInfo {
            id: id.into(),
            name: name.into(),
            config_path: format!("/home/test/.{id}.json"),
            config_exists: true,
            app_present: true,
            ..Default::default()
        };
        Self {
            clients: RefCell::new(vec![client]),
            launcher: Some("/opt/toolport/toolportctl".into()),
            ..Default::default()
        }
    }

    fn revision_of(client: &ClientInfo) -> Option<String> {
        client
            .config_exists
            .then(|| format!("{:?}", client.entries))
    }

    fn entry_of(spec: &EntrySpec) -> ClientEntryInfo {
        ClientEntryInfo {
            name: spec.name.clone(),
            transport: spec.transport.clone(),
            command: spec.command.clone(),
            args: spec.args.clone(),
            url: spec.url.clone(),
            env_keys: spec.env.keys().cloned().collect(),
            extra: Map::new(),
        }
    }
}

impl ClientsHost for MemoryClients {
    fn detect_clients(&self) -> Vec<ClientInfo> {
        self.clients.borrow().clone()
    }

    fn client_revision(&self, client_id: &str) -> Result<Option<String>, String> {
        self.clients
            .borrow()
            .iter()
            .find(|client| client.id == client_id)
            .map(Self::revision_of)
            .ok_or_else(|| format!("unknown client '{client_id}'"))
    }

    fn render_entry(&self, _client_id: &str, spec: &EntrySpec) -> Result<String, String> {
        serde_json::to_string(&Self::entry_of(spec))
            .map(|text| format!("\"{}\": {text}", spec.name))
            .map_err(|error| error.to_string())
    }

    fn write_entry(
        &self,
        client_id: &str,
        name: &str,
        spec: Option<&EntrySpec>,
        expect_revision: Option<&Option<String>>,
    ) -> Result<EntryWrite, ClientWriteError> {
        let mut clients = self.clients.borrow_mut();
        let client = clients
            .iter_mut()
            .find(|client| client.id == client_id)
            .ok_or_else(|| ClientWriteError::Failed(format!("unknown client '{client_id}'")))?;
        if let Some(reason) = &client.direct_unsupported {
            return Err(ClientWriteError::Failed(reason.clone()));
        }
        if expect_revision.is_some_and(|expected| &Self::revision_of(client) != expected) {
            return Err(ClientWriteError::Conflict(
                "the client file changed since the plan".into(),
            ));
        }
        client.entries.retain(|entry| entry.name != name);
        if let Some(spec) = spec {
            client.entries.push(Self::entry_of(spec));
        }
        client.config_exists = true;
        self.writes
            .borrow_mut()
            .push((client_id.into(), name.into(), spec.cloned()));
        Ok(EntryWrite {
            path: client.config_path.clone(),
            backup: Some(format!("{}.bak", client.config_path)),
            revision: Self::revision_of(client),
            extra: Map::new(),
        })
    }

    fn launcher_command(&self) -> Option<String> {
        self.launcher.clone()
    }

    fn launcher_env(&self) -> BTreeMap<String, String> {
        self.env.clone()
    }

    fn prepare_launch(&self, server_id: &str) -> Result<LaunchCommand, String> {
        self.launches
            .get(server_id)
            .cloned()
            .unwrap_or_else(|| Err(format!("no launch line for '{server_id}'")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test01_client_info_keeps_fields_a_newer_upstream_adds() {
        let upstream = json!({
            "id": "claude-code", "name": "Claude Code", "configPath": "/h/.claude.json",
            "configExists": true, "appPresent": true, "usesConnectors": false,
            "gatewayInstalled": true,
            "entries": [{"name": "a", "transport": "stdio", "command": "c", "args": ["x"],
                         "envKeys": ["K"], "futureEntryField": 1}],
            "futureClientField": {"nested": [1]},
        });
        let info: ClientInfo = crate::convert(&upstream).unwrap();
        assert_eq!(info.entries[0].env_keys, vec!["K".to_string()]);
        let back: Value = crate::convert(&info).unwrap();
        assert_eq!(back, upstream);
    }

    #[test]
    fn test02_entry_spec_and_write_keep_unknown_fields() {
        let spec = json!({"name": "a", "transport": "http", "args": [], "env": {},
                          "url": "https://x.invalid/mcp", "later": true});
        let parsed: EntrySpec = crate::convert(&spec).unwrap();
        assert_eq!(crate::convert::<_, Value>(&parsed).unwrap(), spec);
        let written = json!({"path": "/p", "backup": "/b", "revision": "r", "more": 1});
        let parsed: EntryWrite = crate::convert(&written).unwrap();
        assert_eq!(crate::convert::<_, Value>(&parsed).unwrap(), written);
    }

    #[test]
    fn test03_memory_client_refuses_a_write_planned_against_an_older_file() {
        let host = MemoryClients::with_client("claude-code", "Claude Code");
        let planned = host.client_revision("claude-code").unwrap();
        let spec = EntrySpec {
            name: "a".into(),
            transport: "stdio".into(),
            command: Some("c".into()),
            ..Default::default()
        };
        host.write_entry("claude-code", "a", Some(&spec), Some(&planned))
            .unwrap();
        let error = host
            .write_entry("claude-code", "b", Some(&spec), Some(&planned))
            .unwrap_err();
        assert!(matches!(error, ClientWriteError::Conflict(_)));
    }
}
