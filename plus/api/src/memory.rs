//! An in-memory `RegistryAccess` for tests of fork logic. It applies the same
//! edit rules as the conduit adapter, on mirror types and a plain `plus` map.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::fork_data::{plus_ops, AuthProbe, DirectEntry, McpmSource, RegistryPlus, SelfMcpRecord};
use crate::{
    HttpAccess, HttpError, HttpRequest, HttpResponse, RegistryAccess, SecretsHost, ServerRecord,
    TrustInputs,
};

#[derive(Debug, Default)]
pub struct MemoryRegistry {
    pub servers: RefCell<Vec<ServerRecord>>,
    /// The registry's unknown fields; fork data lives under `plus`.
    pub unknown_fields: RefCell<Map<String, Value>>,
    pub profiles: usize,
}

impl MemoryRegistry {
    pub fn with_servers(servers: Vec<ServerRecord>) -> Self {
        Self {
            servers: RefCell::new(servers),
            ..Default::default()
        }
    }

    fn edit<T>(
        &self,
        server_id: &str,
        edit: impl FnOnce(&mut ServerRecord) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut servers = self.servers.borrow_mut();
        let server = servers
            .iter_mut()
            .find(|server| server.id == server_id)
            .ok_or_else(|| format!("unknown server {server_id}"))?;
        edit(server)
    }
}

impl RegistryAccess for MemoryRegistry {
    fn registry_path(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/memory/registry.json"))
    }

    fn list_servers(&self) -> Result<Vec<ServerRecord>, String> {
        Ok(self.servers.borrow().clone())
    }

    fn get_server(&self, server_id: &str) -> Result<Option<ServerRecord>, String> {
        Ok(self
            .servers
            .borrow()
            .iter()
            .find(|server| server.id == server_id)
            .cloned())
    }

    fn profile_count(&self) -> Result<usize, String> {
        Ok(self.profiles)
    }

    fn set_server_enabled(&self, server_id: &str, enabled: bool) -> Result<(), String> {
        self.edit(server_id, |server| {
            server.enabled = enabled;
            Ok(())
        })
    }

    fn plus_data(&self) -> Result<RegistryPlus, String> {
        RegistryPlus::extract(&self.unknown_fields.borrow())
    }

    fn set_selfmcp_record(&self, record: Option<SelfMcpRecord>) -> Result<(), String> {
        plus_ops::set_selfmcp(&mut self.unknown_fields.borrow_mut(), record.as_ref())
    }

    fn add_selfmcp_enabled_in(&self, profile_ids: &[String]) -> Result<(), String> {
        plus_ops::update_selfmcp(&mut self.unknown_fields.borrow_mut(), |record| {
            let listed = record.enabled_in.get_or_insert_with(Vec::new);
            for id in profile_ids {
                if !listed.contains(id) {
                    listed.push(id.clone());
                }
            }
        })
    }

    fn set_selfmcp_opt_out(&self, opt_out: bool) -> Result<(), String> {
        plus_ops::update_selfmcp(&mut self.unknown_fields.borrow_mut(), |record| {
            record.opt_out = opt_out.then_some(true);
        })
    }

    fn set_direct_entry(
        &self,
        client: &str,
        entry: &str,
        record: DirectEntry,
    ) -> Result<(), String> {
        plus_ops::set_direct_entry(
            &mut self.unknown_fields.borrow_mut(),
            client,
            entry,
            &record,
        )
    }

    fn clear_direct_entry(&self, client: &str, entry: &str) -> Result<bool, String> {
        Ok(plus_ops::clear_direct_entry(
            &mut self.unknown_fields.borrow_mut(),
            client,
            entry,
        ))
    }

    fn set_server_auth_probe(
        &self,
        server_id: &str,
        probe: Option<AuthProbe>,
    ) -> Result<(), String> {
        self.edit(server_id, |server| {
            match probe {
                Some(probe) => {
                    server
                        .fork
                        .plus
                        .get_or_insert_with(Default::default)
                        .auth_probe = Some(probe);
                }
                None => {
                    if let Some(plus) = server.fork.plus.as_mut() {
                        plus.auth_probe = None;
                        if plus.is_empty() {
                            server.fork.plus = None;
                        }
                    }
                }
            }
            Ok(())
        })
    }

    fn set_server_mcpm_source(
        &self,
        server_id: &str,
        source: Option<McpmSource>,
    ) -> Result<(), String> {
        self.edit(server_id, |server| {
            server.fork.mcpm_source = source;
            Ok(())
        })
    }

    fn merge_server_mcpm_source(
        &self,
        server_id: &str,
        fields: Map<String, Value>,
    ) -> Result<(), String> {
        self.edit(server_id, |server| {
            let current = server.fork.mcpm_source.take().unwrap_or_default();
            server.fork.mcpm_source = Some(current.merged(fields)?);
            Ok(())
        })
    }

    fn set_server_mcpm_oauth(&self, server_id: &str, oauth: Option<bool>) -> Result<(), String> {
        self.edit(server_id, |server| {
            server.fork.mcpm_oauth = oauth;
            Ok(())
        })
    }

    fn set_server_mcpm_tags(
        &self,
        server_id: &str,
        tags: Option<Vec<String>>,
    ) -> Result<(), String> {
        self.edit(server_id, |server| {
            server.fork.mcpm_tags = tags;
            Ok(())
        })
    }

    fn set_server_forward_instructions(
        &self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        self.edit(server_id, |server| {
            server.fork.forward_instructions = enabled.then_some(true);
            Ok(())
        })
    }

    fn set_server_declare_client_capabilities(
        &self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        self.edit(server_id, |server| {
            server.fork.declare_client_capabilities = enabled.then_some(true);
            Ok(())
        })
    }

    fn set_server_max_request_timeout_ms(
        &self,
        server_id: &str,
        timeout_ms: Option<u64>,
    ) -> Result<(), String> {
        self.edit(server_id, |server| {
            server.fork.max_request_timeout_ms = timeout_ms;
            Ok(())
        })
    }
}

/// An `HttpAccess` that answers from a queue and records what it was asked.
#[derive(Debug, Default)]
pub struct ScriptedHttp {
    pub responses: RefCell<VecDeque<Result<HttpResponse, HttpError>>>,
    pub requests: RefCell<Vec<HttpRequest>>,
}

impl ScriptedHttp {
    pub fn answering(responses: Vec<Result<HttpResponse, HttpError>>) -> Self {
        Self {
            responses: RefCell::new(responses.into()),
            ..Default::default()
        }
    }
}

impl HttpAccess for ScriptedHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.requests.borrow_mut().push(request.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(HttpError::Other("no scripted response left".into())))
    }
}

/// A login keychain in memory. Nothing here reaches a real keychain: the key,
/// its failures and the call counts are fields a test sets and reads.
#[derive(Debug, Default)]
pub struct MemorySecrets {
    pub data_dir: Option<PathBuf>,
    pub env: RefCell<BTreeMap<String, String>>,
    /// What `login_keychain_available` answers; off by default, as off macOS.
    pub available: bool,
    pub key: Cell<Option<[u8; 32]>>,
    /// Makes `read_master_key` fail with this message.
    pub read_error: RefCell<Option<String>>,
    /// Makes `ensure_master_key` fail with this message.
    pub ensure_error: RefCell<Option<String>>,
    /// Makes `rewrite_master_key_acl` fail with this message.
    pub acl_error: RefCell<Option<String>>,
    pub trust_inputs: RefCell<TrustInputs>,
    pub reads: Cell<usize>,
    pub ensures: Cell<usize>,
    pub acl_rewrites: Cell<usize>,
}

impl MemorySecrets {
    /// A macOS-like host: the login keychain is available.
    pub fn macos(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: Some(data_dir.into()),
            available: true,
            ..Default::default()
        }
    }

    pub fn set_env(&self, name: &str, value: &str) {
        self.env.borrow_mut().insert(name.into(), value.into());
    }
}

impl SecretsHost for MemorySecrets {
    fn data_dir(&self) -> Option<PathBuf> {
        self.data_dir.clone()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        self.env.borrow().get(name).cloned()
    }

    fn login_keychain_available(&self) -> bool {
        self.available
    }

    fn read_master_key(&self) -> Result<Option<[u8; 32]>, String> {
        self.reads.set(self.reads.get() + 1);
        match self.read_error.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(self.key.get()),
        }
    }

    fn ensure_master_key(&self) -> Result<[u8; 32], String> {
        self.ensures.set(self.ensures.get() + 1);
        if let Some(error) = self.ensure_error.borrow().clone() {
            return Err(error);
        }
        let key = self.key.get().unwrap_or([0x42; 32]);
        self.key.set(Some(key));
        Ok(key)
    }

    fn rewrite_master_key_acl(&self) -> Result<bool, String> {
        if let Some(error) = self.acl_error.borrow().clone() {
            return Err(error);
        }
        if self.key.get().is_none() {
            return Ok(false);
        }
        self.acl_rewrites.set(self.acl_rewrites.get() + 1);
        Ok(true)
    }

    fn trust_inputs(&self) -> Result<TrustInputs, String> {
        Ok(self.trust_inputs.borrow().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_registry_applies_server_and_plus_ops() {
        let registry = MemoryRegistry::with_servers(vec![ServerRecord {
            id: "a".into(),
            name: "A".into(),
            ..Default::default()
        }]);
        registry.set_server_forward_instructions("a", true).unwrap();
        registry
            .add_selfmcp_enabled_in(&["p".into(), "p".into()])
            .unwrap();
        assert_eq!(
            registry
                .get_server("a")
                .unwrap()
                .unwrap()
                .fork
                .forward_instructions,
            Some(true)
        );
        assert_eq!(
            registry.plus_data().unwrap().selfmcp.unwrap().enabled_in,
            Some(vec!["p".to_string()])
        );
        assert!(registry.set_server_mcpm_oauth("missing", None).is_err());
    }
}
