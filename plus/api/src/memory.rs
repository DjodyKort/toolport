//! An in-memory `RegistryAccess` for tests of fork logic. It applies the same
//! edit rules as the conduit adapter, on mirror types and a plain `plus` map.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::fork_data::{
    budget_number, plus_ops, AuthProbe, DirectEntry, McpmSource, RegistryPlus, SelfMcpRecord,
};
use crate::{
    AccessSetInfo, AuthAccess, AuthChild, CallsInFlight, CatalogAccess, CatalogEntryRecord,
    ConsentSink, ContextHost, EnvEntry, GatewayRouter, HostEnv, HttpAccess, HttpError, HttpRequest,
    HttpResponse, LaunchError, LaunchFault, RegistryAccess, RegistryTx, RegistryView,
    RestartOutcome, ScopeKind, ScopeReach, SecretsHost, ServerRecord, ServerRules, ServerSpec,
    TrustInputs, VaultAccess,
};

#[derive(Debug, Default)]
pub struct MemoryRegistry {
    pub servers: RefCell<Vec<ServerRecord>>,
    /// The registry's unknown fields; fork data lives under `plus`.
    pub unknown_fields: RefCell<Map<String, Value>>,
    pub profiles: usize,
    /// Whether a registry file exists; `read_view` answers `None` without one.
    pub exists: bool,
    pub access_sets: RefCell<Vec<AccessSetInfo>>,
    pub default_access: RefCell<Option<String>>,
    pub client_access_sets: RefCell<Vec<String>>,
    /// `client_scopes`: client id to the stored set reference.
    pub client_scopes: RefCell<BTreeMap<String, String>>,
    /// `folder_profiles`: folder path and the stored set reference, in registry order.
    pub folder_routes: RefCell<Vec<(String, String)>>,
    /// `client_discovery`: client id to its pinned mode.
    pub client_discovery: RefCell<BTreeMap<String, String>>,
    /// Narrowed tools per access set id and server id.
    pub tool_scopes: RefCell<BTreeMap<String, BTreeMap<String, Vec<String>>>>,
    /// Instructions per access set id.
    pub instructions: RefCell<BTreeMap<String, String>>,
    /// Where `registry_path` points; the data dir of tests that write files next to it.
    pub path: Option<PathBuf>,
}

/// One registry state a transaction edits: the same rules as the conduit adapter.
#[derive(Debug, Clone, Default)]
pub struct MemoryState {
    pub servers: Vec<ServerRecord>,
    pub access_sets: Vec<AccessSetInfo>,
    pub default_access: Option<String>,
    pub client_access_sets: Vec<String>,
    pub client_scopes: BTreeMap<String, String>,
    pub folder_routes: Vec<(String, String)>,
    pub client_discovery: BTreeMap<String, String>,
    pub tool_scopes: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    pub instructions: BTreeMap<String, String>,
    pub unknown_fields: Map<String, Value>,
}

fn slug(name: &str) -> String {
    let mapped: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    mapped.trim_matches('-').to_string()
}

impl RegistryView for MemoryState {
    fn version(&self) -> u32 {
        3
    }

    fn servers(&self) -> Result<Vec<ServerRecord>, String> {
        Ok(self.servers.clone())
    }

    fn access_sets(&self) -> Vec<AccessSetInfo> {
        self.access_sets.clone()
    }

    fn default_access(&self) -> Option<String> {
        self.default_access.clone()
    }

    fn client_access_sets(&self) -> Vec<String> {
        self.client_access_sets.clone()
    }

    fn scope_reach(&self) -> Vec<ScopeReach> {
        let mut scopes = vec![self.reach_of(ScopeKind::Default, "", "")];
        scopes.extend(
            self.client_scopes
                .iter()
                .map(|(client, reference)| self.reach_of(ScopeKind::Client, client, reference)),
        );
        scopes.extend(
            self.folder_routes
                .iter()
                .map(|(path, reference)| self.reach_of(ScopeKind::Folder, path, reference)),
        );
        scopes
    }

    fn access_set_record(&self, access_set_id: &str) -> Option<Value> {
        let set = self
            .access_sets
            .iter()
            .find(|set| set.id == access_set_id)?;
        let mut record = serde_json::json!({
            "id": set.id,
            "name": set.name,
            "enabledServerIds": set.server_ids,
        });
        if let Some(scope) = self
            .tool_scopes
            .get(&set.id)
            .filter(|scope| !scope.is_empty())
        {
            record["toolScope"] = serde_json::json!(scope);
        }
        if let Some(text) = self.instructions.get(&set.id) {
            record["instructions"] = Value::from(text.as_str());
        }
        Some(record)
    }

    fn folder_route_for(&self, root: &str) -> Option<ScopeReach> {
        let root = route_text(root);
        if root.is_empty() {
            return None;
        }
        self.folder_routes
            .iter()
            .filter_map(|(path, reference)| {
                let base = route_text(path);
                route_covers(&base, &root).then_some((base.len(), path, reference))
            })
            .max_by_key(|(len, _, _)| *len)
            .map(|(_, path, reference)| self.reach_of(ScopeKind::Folder, path, reference))
    }

    fn direct_entries(&self) -> Result<crate::view::DirectEntries, String> {
        Ok(RegistryPlus::extract(&self.unknown_fields)?
            .direct_entries
            .unwrap_or_default())
    }

    fn selfmcp_record(&self) -> Result<Option<SelfMcpRecord>, String> {
        Ok(RegistryPlus::extract(&self.unknown_fields)?.selfmcp)
    }

    fn client_discovery(&self, client_id: &str) -> Option<String> {
        self.client_discovery.get(client_id).cloned()
    }
}

/// The string comparison of upstream's `normalize_path` and `path_is_within`, which this crate
/// cannot call; the adapter's tests hold the same cases against upstream's own matcher.
fn route_text(path: &str) -> String {
    let mut text = path.trim().replace('\\', "/");
    while text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    if cfg!(windows) {
        text = text.to_ascii_lowercase();
    }
    text
}

fn route_covers(base: &str, root: &str) -> bool {
    !base.is_empty()
        && (root == base
            || root.starts_with(base) && root.as_bytes().get(base.len()) == Some(&b'/'))
}

impl MemoryState {
    /// Upstream's resolution on a version 3 registry: an empty reference is the default access,
    /// `@all-enabled` every enabled server, a set matches by id or by name, and any other
    /// reference reaches nothing.
    fn reach_of(&self, kind: ScopeKind, key: &str, reference: &str) -> ScopeReach {
        let reference = reference.trim();
        let lookup = if reference.is_empty() {
            self.default_access.as_deref().unwrap_or("@all-enabled")
        } else {
            reference
        };
        let set = self
            .access_sets
            .iter()
            .find(|set| set.id == lookup)
            .or_else(|| {
                self.access_sets
                    .iter()
                    .find(|set| set.name.eq_ignore_ascii_case(lookup))
            });
        let everything = lookup == "@all-enabled";
        let dangling = set.is_none() && !everything;
        let server_ids = self
            .servers
            .iter()
            .filter(|server| server.enabled)
            .filter(|server| everything || set.is_some_and(|s| s.server_ids.contains(&server.id)))
            .map(|server| server.id.clone())
            .collect();
        ScopeReach {
            kind,
            key: key.to_string(),
            reference: reference.to_string(),
            set_id: set.map(|set| set.id.clone()),
            dangling,
            server_ids,
        }
    }

    /// Upstream's add switches a ready server on and lists it in the default access set (the
    /// first set when none is explicit).
    fn switch_on_like_upstream(&mut self, server_id: &str) -> Result<(), String> {
        self.server_mut(server_id)?.enabled = true;
        let set = self
            .default_access
            .clone()
            .or_else(|| self.access_sets.first().map(|set| set.id.clone()));
        if let Some(set) = set {
            self.set_access_server(&set, server_id, true)?;
        }
        Ok(())
    }

    fn find_set(&self, reference: &str) -> Option<&AccessSetInfo> {
        self.access_sets
            .iter()
            .find(|set| set.id == reference)
            .or_else(|| {
                self.access_sets
                    .iter()
                    .find(|set| set.name.eq_ignore_ascii_case(reference))
            })
    }

    fn server_mut(&mut self, server_id: &str) -> Result<&mut ServerRecord, String> {
        self.servers
            .iter_mut()
            .find(|server| server.id == server_id)
            .ok_or_else(|| format!("unknown server {server_id}"))
    }
}

impl RegistryTx for MemoryState {
    fn add_stdio_server(
        &mut self,
        name: &str,
        command: &str,
        source: &str,
    ) -> Result<String, String> {
        let base = slug(name);
        let mut id = base.clone();
        let mut n = 1;
        while self.servers.iter().any(|server| server.id == id) {
            n += 1;
            id = format!("{base}-{n}");
        }
        let mut record = ServerRecord {
            id: id.clone(),
            name: name.into(),
            ..Default::default()
        };
        for (key, value) in [
            ("transport", "stdio"),
            ("command", command),
            ("source", source),
        ] {
            record.extra.insert(key.into(), Value::String(value.into()));
        }
        self.servers.push(record);
        Ok(id)
    }

    fn repoint_stdio_server(&mut self, server_id: &str, command: &str) -> Result<(), String> {
        let server = self.server_mut(server_id)?;
        server
            .extra
            .insert("transport".into(), Value::String("stdio".into()));
        server
            .extra
            .insert("command".into(), Value::String(command.into()));
        server.extra.remove("url");
        Ok(())
    }

    fn remove_server(&mut self, server_id: &str) -> Result<(), String> {
        self.server_mut(server_id)?;
        self.servers.retain(|server| server.id != server_id);
        for set in &mut self.access_sets {
            set.server_ids.retain(|id| id != server_id);
        }
        Ok(())
    }

    fn set_server_enabled(&mut self, server_id: &str, enabled: bool) -> Result<(), String> {
        self.server_mut(server_id)?.enabled = enabled;
        Ok(())
    }

    fn set_access_server(
        &mut self,
        access_set_id: &str,
        server_id: &str,
        included: bool,
    ) -> Result<(), String> {
        self.server_mut(server_id)?;
        let set = self
            .access_sets
            .iter_mut()
            .find(|set| set.id == access_set_id)
            .ok_or("Access set not found")?;
        set.server_ids.retain(|id| id != server_id);
        if included {
            set.server_ids.push(server_id.into());
        }
        Ok(())
    }

    fn create_access_set(&mut self, name: &str) -> Result<String, String> {
        if name.trim().is_empty() {
            return Err("Give the access set a name".into());
        }
        if name.trim().starts_with('@') {
            return Err("Access-set names beginning with @ are reserved".into());
        }
        let mut base = slug(name);
        while base.contains("--") {
            base = base.replace("--", "-");
        }
        if base.is_empty() {
            base = "item".into();
        }
        let mut id = base.clone();
        let mut n = 1;
        while self.access_sets.iter().any(|set| set.id == id) {
            n += 1;
            id = format!("{base}-{n}");
        }
        self.access_sets.push(AccessSetInfo {
            id: id.clone(),
            name: name.trim().into(),
            server_ids: Vec::new(),
        });
        Ok(id)
    }

    fn restore_access_set(&mut self, record: Value) -> Result<String, String> {
        let text = |key: &str| record[key].as_str().map(String::from);
        let (Some(id), Some(name)) = (text("id"), text("name")) else {
            return Err("the saved access set has no id or name".into());
        };
        if self
            .access_sets
            .iter()
            .any(|set| set.id == id || set.name.eq_ignore_ascii_case(&name))
        {
            return Err(format!("an access set called {name} exists already"));
        }
        let strings = |value: &Value| -> Vec<String> {
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        };
        self.access_sets.push(AccessSetInfo {
            id: id.clone(),
            name,
            server_ids: strings(&record["enabledServerIds"]),
        });
        if let Some(scope) = record["toolScope"].as_object() {
            self.tool_scopes.insert(
                id.clone(),
                scope
                    .iter()
                    .map(|(server, tools)| (server.clone(), strings(tools)))
                    .collect(),
            );
        }
        if let Some(text) = text("instructions") {
            self.instructions.insert(id.clone(), text);
        }
        Ok(id)
    }

    fn delete_access_set(&mut self, access_set_id: &str) -> Result<(), String> {
        if self.default_access.as_deref() == Some(access_set_id) {
            return Err("Cannot remove the default access set or its policy context".into());
        }
        if self.access_sets.len() <= 1 {
            return Err("Cannot remove the last profile".into());
        }
        let before = self.access_sets.len();
        self.access_sets.retain(|set| set.id != access_set_id);
        if self.access_sets.len() == before {
            return Err(format!("No profile with id '{access_set_id}'"));
        }
        self.tool_scopes.remove(access_set_id);
        self.instructions.remove(access_set_id);
        Ok(())
    }

    fn set_default_access(&mut self, access_set_id: Option<&str>) -> Result<(), String> {
        self.default_access = match access_set_id.filter(|set| !set.is_empty()) {
            None => None,
            Some(reference) => Some(
                self.find_set(reference)
                    .map(|set| set.id.clone())
                    .ok_or_else(|| format!("No unique profile matches '{reference}'"))?,
            ),
        };
        Ok(())
    }

    fn set_access_tools(
        &mut self,
        access_set_id: &str,
        server_id: &str,
        tools: Option<Vec<String>>,
    ) -> Result<(), String> {
        if !self.access_sets.iter().any(|set| set.id == access_set_id) {
            return Err(format!("No profile with id '{access_set_id}'"));
        }
        match tools {
            Some(tools) => {
                self.tool_scopes
                    .entry(access_set_id.to_string())
                    .or_default()
                    .insert(server_id.to_string(), tools);
            }
            None => {
                if let Some(scope) = self.tool_scopes.get_mut(access_set_id) {
                    scope.remove(server_id);
                    if scope.is_empty() {
                        self.tool_scopes.remove(access_set_id);
                    }
                }
            }
        }
        Ok(())
    }

    fn set_folder_route(&mut self, path: &str, access_set_id: &str) -> Result<(), String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("choose a project folder".into());
        }
        let reference = access_set_id.trim();
        if reference.is_empty() {
            return Err("choose a profile".into());
        }
        let set = self
            .access_sets
            .iter()
            .find(|set| set.id == reference || set.name == reference)
            .ok_or("the selected profile no longer exists")?;
        let stored = set.id.clone();
        self.folder_routes
            .retain(|(existing, _)| existing.trim() != path);
        self.folder_routes.push((path.to_string(), stored));
        Ok(())
    }

    fn remove_folder_route(&mut self, path: &str) -> Result<(), String> {
        self.folder_routes.retain(|(existing, _)| existing != path);
        Ok(())
    }

    fn set_selfmcp_record(&mut self, record: Option<SelfMcpRecord>) -> Result<(), String> {
        plus_ops::set_selfmcp(&mut self.unknown_fields, record.as_ref())
    }

    fn set_client_scope(
        &mut self,
        client_id: &str,
        access_set_id: Option<&str>,
    ) -> Result<(), String> {
        match access_set_id.map(str::trim).filter(|set| !set.is_empty()) {
            Some(reference) => {
                let stored = self
                    .access_sets
                    .iter()
                    .find(|set| set.id == reference || set.name.eq_ignore_ascii_case(reference))
                    .map_or(reference.to_string(), |set| set.id.clone());
                self.client_scopes.insert(client_id.to_string(), stored);
            }
            None => {
                self.client_scopes.remove(client_id);
            }
        }
        Ok(())
    }

    fn set_client_discovery(&mut self, client_id: &str, mode: Option<&str>) -> Result<(), String> {
        match mode
            .map(|mode| mode.trim().to_ascii_lowercase())
            .filter(|mode| matches!(mode.as_str(), "full" | "lazy" | "grouped"))
        {
            Some(mode) => {
                self.client_discovery.insert(client_id.to_string(), mode);
            }
            None => {
                self.client_discovery.remove(client_id);
            }
        }
        Ok(())
    }

    fn add_server(&mut self, spec: &ServerSpec) -> Result<String, String> {
        let spec = normalized(spec)?;
        let id = self.add_stdio_server(&spec.name, "placeholder", "manual")?;
        self.update_server(&id, &spec)?;
        self.switch_on_like_upstream(&id)?;
        Ok(id)
    }

    fn add_catalog_server(&mut self, entry: &CatalogEntryRecord) -> Result<String, String> {
        let command = entry.text("command").map(String::from);
        let url = entry.text("url").map(String::from);
        if command.is_none() && url.is_none() {
            return Err(format!(
                "{} needs its own endpoint URL. Open it from the catalog to enter one.",
                entry.name
            ));
        }
        let id = self.add_stdio_server(
            &entry.name,
            "placeholder",
            &format!("catalog:{}", entry.source),
        )?;
        let server = self.server_mut(&id)?;
        server
            .extra
            .insert("transport".into(), Value::String(entry.transport.clone()));
        match &command {
            Some(command) => server
                .extra
                .insert("command".into(), Value::String(command.clone())),
            None => server.extra.remove("command"),
        };
        match &url {
            Some(url) => server
                .extra
                .insert("url".into(), Value::String(url.clone())),
            None => server.extra.remove("url"),
        };
        let env: Vec<EnvEntry> = entry
            .env_keys
            .iter()
            .map(|key| EnvEntry {
                key: key.clone(),
                value: None,
                secret: true,
                extra: Map::new(),
            })
            .collect();
        let needs_credential = !env.is_empty();
        self.set_server_env(&id, env)?;
        if !needs_credential {
            self.switch_on_like_upstream(&id)?;
        }
        Ok(id)
    }

    fn update_server(&mut self, server_id: &str, spec: &ServerSpec) -> Result<(), String> {
        let spec = normalized(spec)?;
        let server = self.server_mut(server_id)?;
        server.name = spec.name.clone();
        let set = |server: &mut ServerRecord, key: &str, value: &Option<String>| match value {
            Some(value) => {
                server
                    .extra
                    .insert(key.into(), Value::String(value.clone()));
            }
            None => {
                server.extra.remove(key);
            }
        };
        server
            .extra
            .insert("transport".into(), Value::String(spec.transport.clone()));
        set(server, "command", &spec.command);
        set(server, "url", &spec.url);
        set(server, "cwd", &spec.cwd);
        server.extra.insert(
            "args".into(),
            Value::Array(spec.args.iter().cloned().map(Value::String).collect()),
        );
        if let Some(inherit) = spec.inherit_env {
            server
                .extra
                .insert("inheritEnv".into(), Value::Bool(inherit));
        }
        Ok(())
    }

    fn set_server_env(&mut self, server_id: &str, env: Vec<EnvEntry>) -> Result<(), String> {
        let value = serde_json::to_value(env).map_err(|error| error.to_string())?;
        self.server_mut(server_id)?
            .extra
            .insert("env".into(), value);
        Ok(())
    }

    fn set_forward_instructions(&mut self, server_id: &str, enabled: bool) -> Result<(), String> {
        self.server_mut(server_id)?.fork.forward_instructions = enabled.then_some(true);
        Ok(())
    }

    fn set_serial(&mut self, server_id: &str, enabled: bool) -> Result<(), String> {
        self.server_mut(server_id)?.fork.serial = enabled.then_some(true);
        Ok(())
    }

    fn set_declare_client_capabilities(
        &mut self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        self.server_mut(server_id)?.fork.declare_client_capabilities = enabled.then_some(true);
        Ok(())
    }
}

/// The rules upstream's editor applies before it stores a server.
fn normalized(spec: &ServerSpec) -> Result<ServerSpec, String> {
    let mut spec = spec.clone();
    spec.name = spec.name.trim().to_string();
    if spec.name.is_empty() {
        return Err("give the server a name".into());
    }
    let nonempty = |value: Option<String>| {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    match spec.transport.as_str() {
        "stdio" => {
            spec.command = nonempty(spec.command);
            if spec.command.is_none() {
                return Err("enter the command to run".into());
            }
            spec.url = None;
            spec.cwd = nonempty(spec.cwd);
        }
        "http" | "sse" => {
            spec.url = nonempty(spec.url);
            let valid = spec
                .url
                .as_deref()
                .map(str::to_ascii_lowercase)
                .is_some_and(|url| url.starts_with("http://") || url.starts_with("https://"));
            if !valid {
                return Err("enter an http:// or https:// server URL".into());
            }
            spec.command = None;
            spec.args.clear();
            spec.cwd = None;
            spec.inherit_env = spec.inherit_env.map(|_| false);
        }
        _ => return Err("choose stdio, HTTP, or SSE transport".into()),
    }
    Ok(spec)
}

impl MemoryRegistry {
    pub fn with_servers(servers: Vec<ServerRecord>) -> Self {
        Self {
            servers: RefCell::new(servers),
            exists: true,
            ..Default::default()
        }
    }

    fn state(&self) -> MemoryState {
        MemoryState {
            servers: self.servers.borrow().clone(),
            access_sets: self.access_sets.borrow().clone(),
            default_access: self.default_access.borrow().clone(),
            client_access_sets: self.client_access_sets.borrow().clone(),
            client_scopes: self.client_scopes.borrow().clone(),
            folder_routes: self.folder_routes.borrow().clone(),
            client_discovery: self.client_discovery.borrow().clone(),
            tool_scopes: self.tool_scopes.borrow().clone(),
            instructions: self.instructions.borrow().clone(),
            unknown_fields: self.unknown_fields.borrow().clone(),
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
        Some(
            self.path
                .clone()
                .unwrap_or_else(|| PathBuf::from("/memory/registry.json")),
        )
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

    fn read_view(&self) -> Result<Option<Box<dyn RegistryView>>, String> {
        Ok(self
            .exists
            .then(|| Box::new(self.state()) as Box<dyn RegistryView>))
    }

    fn transact(
        &self,
        edit: &mut dyn FnMut(&mut dyn RegistryTx) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut state = self.state();
        edit(&mut state)?;
        *self.servers.borrow_mut() = state.servers;
        *self.access_sets.borrow_mut() = state.access_sets;
        *self.default_access.borrow_mut() = state.default_access;
        *self.client_scopes.borrow_mut() = state.client_scopes;
        *self.folder_routes.borrow_mut() = state.folder_routes;
        *self.client_discovery.borrow_mut() = state.client_discovery;
        *self.tool_scopes.borrow_mut() = state.tool_scopes;
        *self.instructions.borrow_mut() = state.instructions;
        *self.unknown_fields.borrow_mut() = state.unknown_fields;
        Ok(())
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

    fn set_server_tool_budget(
        &self,
        server_id: &str,
        tool: &str,
        seconds: Option<f64>,
    ) -> Result<(), String> {
        self.edit(server_id, |server| {
            let budgets = server
                .fork
                .tool_budgets
                .get_or_insert_with(Default::default);
            match seconds {
                Some(seconds) => budgets.insert(tool.to_string(), budget_number(seconds)?),
                None => budgets.remove(tool),
            };
            if budgets.is_empty() {
                server.fork.tool_budgets = None;
            }
            Ok(())
        })
    }

    fn set_server_mode(&self, server_id: &str, mode: Option<&str>) -> Result<(), String> {
        self.edit(server_id, |server| {
            server.fork.server_mode = mode.map(String::from);
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
    /// Every `(server, key)` removed with `delete_server_secret`.
    pub deleted_secrets: RefCell<Vec<(String, String)>>,
    /// The vault: `(server, key)` to value.
    pub vault: RefCell<BTreeMap<(String, String), String>>,
    /// Makes every vault read fail with this message.
    pub vault_error: RefCell<Option<String>>,
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

    fn delete_server_secret(&self, server_id: &str, key: &str) -> Result<(), String> {
        self.deleted_secrets
            .borrow_mut()
            .push((server_id.into(), key.into()));
        self.vault
            .borrow_mut()
            .remove(&(server_id.to_string(), key.to_string()));
        Ok(())
    }
}

/// A registry, a secrets host and a process in memory, for tests of commands and tools.
#[derive(Debug, Default)]
pub struct MemoryHost {
    pub registry: MemoryRegistry,
    pub secrets: MemorySecrets,
    pub seam: bool,
    pub data_dir_used: RefCell<Option<PathBuf>>,
    /// The home directory `ContextHost` reports.
    pub home: Option<PathBuf>,
    /// What `CatalogAccess::curated` searches.
    pub catalog: Vec<CatalogEntryRecord>,
    pub http: ScriptedHttp,
    pub auth: ScriptedAuth,
    pub now: Cell<i64>,
    /// The settings files `SettingsStore` reports; they are read and written as plain files.
    pub settings_paths: Vec<PathBuf>,
    pub tasks: crate::tasks::FakeTaskHost,
    pub launch_at_login: Cell<Option<bool>>,
    pub sync: MemorySync,
    pub clients: crate::clients::MemoryClients,
    /// What `ServerActivity` reports: server id to the time of its latest successful call.
    pub activity: RefCell<BTreeMap<String, i64>>,
    /// Makes `ServerActivity` fail with this message.
    pub activity_error: RefCell<Option<String>>,
    /// What `SavingsAccess::catalog_sessions` returns.
    pub savings_rows: Vec<Value>,
}

/// What the sync seam of a test host answers.
#[derive(Debug, Default)]
pub struct MemorySync {
    pub passphrase: RefCell<Option<String>>,
    pub keyring_locked: Cell<bool>,
    pub upstream_active: Cell<bool>,
}

impl crate::SavingsAccess for MemoryHost {
    fn catalog_sessions(&self) -> Result<Vec<Value>, String> {
        Ok(self.savings_rows.clone())
    }

    fn catalog_summary(&self) -> Result<Value, String> {
        Ok(Value::Object(Map::new()))
    }
}

impl crate::SettingsStore for MemoryHost {
    fn claude_settings_paths(&self) -> Vec<PathBuf> {
        self.settings_paths.clone()
    }

    fn read_settings(&self, path: &Path) -> Result<(Value, Option<String>), String> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map(|value| (value, Some(text)))
                .map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok((Value::Object(Map::new()), None))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn write_key(
        &self,
        path: &Path,
        original: Option<&str>,
        root: &Value,
        key: &str,
    ) -> Result<(), String> {
        let mut current: Value = match original {
            Some(text) => serde_json::from_str(text).map_err(|e| e.to_string())?,
            None => Value::Object(Map::new()),
        };
        match root.get(key) {
            Some(value) => current[key] = value.clone(),
            None => {
                if let Some(map) = current.as_object_mut() {
                    map.remove(key);
                }
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&current).unwrap())
            .map_err(|e| e.to_string())
    }
}

impl RegistryAccess for MemoryHost {
    fn registry_path(&self) -> Option<PathBuf> {
        self.registry.registry_path()
    }
    fn list_servers(&self) -> Result<Vec<ServerRecord>, String> {
        self.registry.list_servers()
    }
    fn get_server(&self, server_id: &str) -> Result<Option<ServerRecord>, String> {
        self.registry.get_server(server_id)
    }
    fn profile_count(&self) -> Result<usize, String> {
        self.registry.profile_count()
    }
    fn read_view(&self) -> Result<Option<Box<dyn RegistryView>>, String> {
        self.registry.read_view()
    }
    fn transact(
        &self,
        edit: &mut dyn FnMut(&mut dyn RegistryTx) -> Result<(), String>,
    ) -> Result<(), String> {
        self.registry.transact(edit)
    }
    fn set_server_enabled(&self, server_id: &str, enabled: bool) -> Result<(), String> {
        self.registry.set_server_enabled(server_id, enabled)
    }
    fn plus_data(&self) -> Result<RegistryPlus, String> {
        self.registry.plus_data()
    }
    fn set_selfmcp_record(&self, record: Option<SelfMcpRecord>) -> Result<(), String> {
        self.registry.set_selfmcp_record(record)
    }
    fn add_selfmcp_enabled_in(&self, profile_ids: &[String]) -> Result<(), String> {
        self.registry.add_selfmcp_enabled_in(profile_ids)
    }
    fn set_selfmcp_opt_out(&self, opt_out: bool) -> Result<(), String> {
        self.registry.set_selfmcp_opt_out(opt_out)
    }
    fn set_direct_entry(
        &self,
        client: &str,
        entry: &str,
        record: DirectEntry,
    ) -> Result<(), String> {
        self.registry.set_direct_entry(client, entry, record)
    }
    fn clear_direct_entry(&self, client: &str, entry: &str) -> Result<bool, String> {
        self.registry.clear_direct_entry(client, entry)
    }
    fn set_server_auth_probe(
        &self,
        server_id: &str,
        probe: Option<AuthProbe>,
    ) -> Result<(), String> {
        self.registry.set_server_auth_probe(server_id, probe)
    }
    fn set_server_mcpm_source(
        &self,
        server_id: &str,
        source: Option<McpmSource>,
    ) -> Result<(), String> {
        self.registry.set_server_mcpm_source(server_id, source)
    }
    fn merge_server_mcpm_source(
        &self,
        server_id: &str,
        fields: Map<String, Value>,
    ) -> Result<(), String> {
        self.registry.merge_server_mcpm_source(server_id, fields)
    }
    fn set_server_mcpm_oauth(&self, server_id: &str, oauth: Option<bool>) -> Result<(), String> {
        self.registry.set_server_mcpm_oauth(server_id, oauth)
    }
    fn set_server_mcpm_tags(
        &self,
        server_id: &str,
        tags: Option<Vec<String>>,
    ) -> Result<(), String> {
        self.registry.set_server_mcpm_tags(server_id, tags)
    }
    fn set_server_forward_instructions(
        &self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        self.registry
            .set_server_forward_instructions(server_id, enabled)
    }
    fn set_server_declare_client_capabilities(
        &self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        self.registry
            .set_server_declare_client_capabilities(server_id, enabled)
    }
    fn set_server_max_request_timeout_ms(
        &self,
        server_id: &str,
        timeout_ms: Option<u64>,
    ) -> Result<(), String> {
        self.registry
            .set_server_max_request_timeout_ms(server_id, timeout_ms)
    }
    fn set_server_tool_budget(
        &self,
        server_id: &str,
        tool: &str,
        seconds: Option<f64>,
    ) -> Result<(), String> {
        self.registry
            .set_server_tool_budget(server_id, tool, seconds)
    }
    fn set_server_mode(&self, server_id: &str, mode: Option<&str>) -> Result<(), String> {
        self.registry.set_server_mode(server_id, mode)
    }
}

impl SecretsHost for MemoryHost {
    fn data_dir(&self) -> Option<PathBuf> {
        self.secrets.data_dir()
    }
    fn env_var(&self, name: &str) -> Option<String> {
        self.secrets.env_var(name)
    }
    fn login_keychain_available(&self) -> bool {
        self.secrets.login_keychain_available()
    }
    fn read_master_key(&self) -> Result<Option<[u8; 32]>, String> {
        self.secrets.read_master_key()
    }
    fn ensure_master_key(&self) -> Result<[u8; 32], String> {
        self.secrets.ensure_master_key()
    }
    fn rewrite_master_key_acl(&self) -> Result<bool, String> {
        self.secrets.rewrite_master_key_acl()
    }
    fn trust_inputs(&self) -> Result<TrustInputs, String> {
        self.secrets.trust_inputs()
    }
    fn delete_server_secret(&self, server_id: &str, key: &str) -> Result<(), String> {
        self.secrets.delete_server_secret(server_id, key)
    }
}

impl MemoryHost {
    fn put_in_vault(&self, server_id: &str, key: &str, value: &str) {
        self.secrets
            .vault
            .borrow_mut()
            .insert((server_id.into(), key.into()), value.into());
    }

    fn change_env(
        &self,
        server_id: &str,
        change: impl Fn(&mut Vec<EnvEntry>),
    ) -> Result<(), String> {
        self.registry.transact(&mut |tx| {
            let server = tx
                .servers()?
                .into_iter()
                .find(|server| server.id == server_id)
                .ok_or_else(|| format!("No server with id '{server_id}'"))?;
            let mut env = server.env()?;
            change(&mut env);
            tx.set_server_env(server_id, env)
        })
    }
}

impl VaultAccess for MemoryHost {
    fn read_vault_secret(&self, server_id: &str, key: &str) -> Result<Option<String>, String> {
        if let Some(error) = self.secrets.vault_error.borrow().clone() {
            return Err(error);
        }
        Ok(self
            .secrets
            .vault
            .borrow()
            .get(&(server_id.to_string(), key.to_string()))
            .cloned())
    }

    fn store_env_secret(&self, server_id: &str, key: &str, value: &str) -> Result<(), String> {
        self.change_env(server_id, |env| {
            match env.iter_mut().find(|entry| entry.key == key) {
                Some(entry) => {
                    entry.secret = true;
                    entry.value = None;
                }
                None => env.push(EnvEntry {
                    key: key.to_string(),
                    secret: true,
                    ..Default::default()
                }),
            }
        })?;
        self.put_in_vault(server_id, key, value);
        Ok(())
    }

    fn store_launch_secret(&self, server_id: &str, key: &str, value: &str) -> Result<(), String> {
        self.put_in_vault(server_id, key, value);
        Ok(())
    }

    fn store_access_token(&self, server_id: &str, token: &str) -> Result<(), String> {
        self.put_in_vault(server_id, crate::secrets::HTTP_AUTH_KEY, token);
        Ok(())
    }

    fn remove_env_secret(&self, server_id: &str, key: &str) -> Result<(), String> {
        self.change_env(server_id, |env| env.retain(|entry| entry.key != key))?;
        self.secrets.delete_server_secret(server_id, key)
    }

    fn clear_access_token(&self, server_id: &str) -> Result<(), String> {
        self.secrets
            .delete_server_secret(server_id, crate::secrets::HTTP_AUTH_KEY)
    }
}

impl ServerRules for MemoryHost {
    fn credential_text(&self, text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        let prefixed = [
            "sk-",
            "ghp_",
            "gho_",
            "github_pat_",
            "xoxb-",
            "xoxp-",
            "xoxa-",
            "akia",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix) && text.len() >= 12);
        prefixed
            || ["password=", "token=", "api-key=", "api_key="]
                .iter()
                .any(|needle| lower.contains(needle))
    }

    fn spawn_env_refused(&self, key: &str, value: &str) -> bool {
        match key {
            "LD_PRELOAD" | "DYLD_INSERT_LIBRARIES" | "BASH_ENV" => true,
            "NODE_OPTIONS" => value.contains("--require"),
            "JAVA_TOOL_OPTIONS" => value.contains("-javaagent"),
            _ => false,
        }
    }
}

impl crate::ServerActivity for MemoryHost {
    fn last_successful_calls(
        &self,
        server_ids: &[String],
    ) -> Result<BTreeMap<String, i64>, String> {
        if let Some(error) = self.activity_error.borrow().clone() {
            return Err(error);
        }
        Ok(self
            .activity
            .borrow()
            .iter()
            .filter(|(id, _)| server_ids.contains(id))
            .map(|(id, at)| (id.clone(), *at))
            .collect())
    }
}

impl CatalogAccess for MemoryHost {
    fn curated(&self, query: &str) -> Vec<CatalogEntryRecord> {
        let query = query.trim().to_ascii_lowercase();
        self.catalog
            .iter()
            .filter(|entry| query.is_empty() || entry.name.to_ascii_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    fn registry_entries(&self, body: &Value) -> crate::RegistryEntries {
        let mut page = crate::RegistryEntries::default();
        let items = body["servers"].as_array().into_iter().flatten();
        for item in items {
            let official = &item["_meta"]["io.modelcontextprotocol.registry/official"];
            if official["isLatest"] == Value::Bool(false) {
                continue;
            }
            let server = item.get("server").unwrap_or(item);
            let Some(id) = server["name"].as_str() else {
                continue;
            };
            if official["status"]
                .as_str()
                .is_some_and(|status| status != "active")
            {
                page.retired.push(id.to_string());
                continue;
            }
            let Some(url) = server["remotes"][0]["url"].as_str() else {
                page.retired.push(id.to_string());
                continue;
            };
            let entry = serde_json::json!({
                "name": server["title"].as_str().unwrap_or(id),
                "description": server["description"].as_str().unwrap_or(""),
                "transport": "http", "command": null, "args": [], "url": url,
                "envKeys": [], "source": "registry", "homepage": null,
                "publisher": id.split_once('/').map(|(namespace, _)| namespace),
            });
            if let Ok(record) = crate::convert(&entry) {
                page.entries.push((id.to_string(), record));
            }
        }
        page
    }
}

/// A gateway router in memory: the servers it holds, with the calls in flight on each, and a
/// log of what the listener did to it.
#[derive(Debug, Default)]
pub struct MemoryGateway {
    pub held: std::sync::Mutex<BTreeMap<String, usize>>,
    /// Servers that run once per project folder: server id, then folder, with the calls in flight.
    pub rooted: std::sync::Mutex<BTreeMap<String, BTreeMap<String, usize>>>,
    /// Servers whose restart fails, with the detail to report.
    pub failing: std::sync::Mutex<BTreeMap<String, String>>,
    /// Folders of a rooted server whose restart fails, keyed `server id` then folder.
    pub failing_roots: std::sync::Mutex<BTreeMap<String, BTreeMap<String, String>>>,
    /// Restarts, as the server id and, for a folder instance, `" in <folder>"`.
    pub restarted: std::sync::Mutex<Vec<String>>,
    pub stale_marks: std::sync::atomic::AtomicUsize,
}

impl MemoryGateway {
    pub fn holding(servers: &[(&str, usize)]) -> Self {
        let gateway = Self::default();
        *gateway.held.lock().unwrap() = servers
            .iter()
            .map(|(id, calls)| (id.to_string(), *calls))
            .collect();
        gateway
    }

    /// A gateway that runs `server_id` once per project folder, with the calls in flight in each.
    pub fn holding_rooted(server_id: &str, folders: &[(&str, usize)]) -> Self {
        let gateway = Self::default();
        gateway.rooted.lock().unwrap().insert(
            server_id.to_string(),
            folders
                .iter()
                .map(|(root, calls)| (root.to_string(), *calls))
                .collect(),
        );
        gateway
    }

    pub fn restarted(&self) -> Vec<String> {
        self.restarted.lock().unwrap().clone()
    }

    pub fn stale_marks(&self) -> usize {
        self.stale_marks.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl GatewayRouter for MemoryGateway {
    fn holds_server(&self, server_id: &str) -> bool {
        self.held.lock().unwrap().contains_key(server_id)
            || self.rooted.lock().unwrap().contains_key(server_id)
    }

    fn calls_in_flight(&self, server_id: &str) -> Option<Vec<CallsInFlight>> {
        let shared = self.held.lock().unwrap().get(server_id).copied();
        let rooted = self.rooted.lock().unwrap().get(server_id).cloned();
        if shared.is_none() && rooted.is_none() {
            return None;
        }
        Some(
            shared
                .map(|calls| CallsInFlight { root: None, calls })
                .into_iter()
                .chain(
                    rooted
                        .into_iter()
                        .flatten()
                        .map(|(root, calls)| CallsInFlight {
                            root: Some(root),
                            calls,
                        }),
                )
                .collect(),
        )
    }

    fn restart_server(&self, server_id: &str) -> Option<Vec<RestartOutcome>> {
        let shared = self.held.lock().unwrap().contains_key(server_id);
        let rooted = self.rooted.lock().unwrap().get(server_id).cloned();
        if !shared && rooted.is_none() {
            return None;
        }
        let mut outcomes = Vec::new();
        let mut restart = |root: Option<String>, failure: Option<String>| {
            outcomes.push(match failure {
                Some(detail) => RestartOutcome {
                    root,
                    restarted: false,
                    detail,
                },
                None => {
                    self.restarted.lock().unwrap().push(match &root {
                        Some(root) => format!("{server_id} in {root}"),
                        None => server_id.to_string(),
                    });
                    RestartOutcome {
                        root,
                        restarted: true,
                        detail: "restarted".into(),
                    }
                }
            });
        };
        if shared {
            restart(None, self.failing.lock().unwrap().get(server_id).cloned());
        }
        for root in rooted.into_iter().flat_map(|folders| folders.into_keys()) {
            let failure = self
                .failing_roots
                .lock()
                .unwrap()
                .get(server_id)
                .and_then(|folders| folders.get(&root))
                .cloned();
            restart(Some(root), failure);
        }
        Some(outcomes)
    }

    fn mark_lists_stale(&self) {
        self.stale_marks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ContextHost for MemoryHost {
    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }
    fn count_tokens(&self, text: &str) -> u64 {
        (text.len() as u64).div_ceil(4)
    }
}

impl HostEnv for MemoryHost {
    fn app_version(&self) -> &'static str {
        "0.0.0-test"
    }
    fn use_data_dir(&self, dir: &std::path::Path) {
        *self.data_dir_used.borrow_mut() = Some(dir.to_path_buf());
    }
    fn seam_live(&self) -> bool {
        self.seam
    }
    fn now_unix(&self) -> i64 {
        self.now.get()
    }
    fn launch_at_login(&self) -> Option<bool> {
        self.launch_at_login.get()
    }
}

impl HttpAccess for MemoryHost {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.http.send(request)
    }
}

/// One `auth` subcommand run: what it prints on stderr and how it ends.
#[derive(Debug, Clone, Default)]
pub struct ScriptedRun {
    pub stderr: Vec<String>,
    /// `None` keeps the process running until it is killed.
    pub exit: Option<i32>,
    pub killed: Arc<AtomicBool>,
    pub detached: Arc<AtomicBool>,
    /// Marks every line and URL as holding a value the launcher keeps secret.
    pub secret: Option<String>,
}

/// The stdio launcher, the review gate, the remote flow and the browser, in memory.
#[derive(Debug, Default)]
pub struct ScriptedAuth {
    pub runs: RefCell<VecDeque<Result<ScriptedRun, LaunchError>>>,
    pub started: RefCell<Vec<String>>,
    pub awaiting_review: RefCell<BTreeSet<String>>,
    pub remote_result: RefCell<Option<Result<(), String>>>,
    pub remote_signins: RefCell<Vec<String>>,
    /// The consent URL a remote sign-in hands to the sink it was given.
    pub remote_consent: RefCell<Option<String>>,
    /// Per remote sign-in: whether a consent sink was passed, so the browser stayed closed.
    pub remote_sinks: RefCell<Vec<bool>>,
    pub opened: RefCell<Vec<String>>,
}

struct ScriptedChild {
    run: ScriptedRun,
}

impl AuthChild for ScriptedChild {
    fn take_stderr(&mut self) -> Option<Box<dyn Read + Send>> {
        let text: String = self
            .run
            .stderr
            .iter()
            .map(|line| format!("{line}\n"))
            .collect();
        Some(Box::new(std::io::Cursor::new(text.into_bytes())))
    }

    fn try_exit(&mut self) -> Option<i32> {
        self.run.exit
    }

    fn kill(&mut self) {
        self.run.killed.store(true, Ordering::SeqCst);
    }

    fn detach(self: Box<Self>) {
        self.run.detached.store(true, Ordering::SeqCst);
    }

    fn scrub(&self, line: String) -> String {
        match &self.run.secret {
            Some(secret) => line.replace(secret.as_str(), "***"),
            None => line,
        }
    }

    fn scrub_url(&self, url: String) -> Option<String> {
        match &self.run.secret {
            Some(secret) if url.contains(secret.as_str()) => None,
            _ => Some(url),
        }
    }

    fn hides_output(&self) -> bool {
        false
    }
}

impl AuthAccess for MemoryHost {
    fn start_auth(&self, server_id: &str) -> Result<Box<dyn AuthChild>, LaunchError> {
        self.auth.started.borrow_mut().push(server_id.to_string());
        let next = self.auth.runs.borrow_mut().pop_front();
        match next {
            Some(Ok(run)) => Ok(Box::new(ScriptedChild { run })),
            Some(Err(error)) => Err(error),
            None => Err(LaunchError::new(LaunchFault::Spawn, "no scripted run left")),
        }
    }

    fn awaits_review(&self, server_id: &str) -> Result<bool, String> {
        Ok(self.auth.awaiting_review.borrow().contains(server_id))
    }

    fn sign_in_remote(
        &self,
        server_id: &str,
        on_consent: Option<ConsentSink>,
    ) -> Result<(), String> {
        self.auth
            .remote_signins
            .borrow_mut()
            .push(server_id.to_string());
        self.auth
            .remote_sinks
            .borrow_mut()
            .push(on_consent.is_some());
        if let (Some(sink), Some(url)) = (on_consent, self.auth.remote_consent.borrow().as_deref())
        {
            sink(url);
        }
        self.auth.remote_result.borrow().clone().unwrap_or(Ok(()))
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.auth.opened.borrow_mut().push(url.to_string());
        Ok(())
    }
}

impl crate::clients::ClientsHost for MemoryHost {
    fn detect_clients(&self) -> Vec<crate::clients::ClientInfo> {
        self.clients.detect_clients()
    }
    fn client_revision(&self, client_id: &str) -> Result<Option<String>, String> {
        self.clients.client_revision(client_id)
    }
    fn render_entry(
        &self,
        client_id: &str,
        spec: &crate::clients::EntrySpec,
    ) -> Result<String, String> {
        self.clients.render_entry(client_id, spec)
    }
    fn write_entry(
        &self,
        client_id: &str,
        name: &str,
        spec: Option<&crate::clients::EntrySpec>,
        expect_revision: Option<&Option<String>>,
    ) -> Result<crate::clients::EntryWrite, crate::clients::ClientWriteError> {
        self.clients
            .write_entry(client_id, name, spec, expect_revision)
    }
    fn gateway_command(&self) -> Option<String> {
        self.clients.gateway_command()
    }
    fn launcher_command(&self) -> Option<String> {
        self.clients.launcher_command()
    }
    fn launcher_env(&self) -> BTreeMap<String, String> {
        self.clients.launcher_env()
    }
    fn prepare_launch(&self, server_id: &str) -> Result<crate::clients::LaunchCommand, String> {
        self.clients.prepare_launch(server_id)
    }
}

impl crate::tasks::TaskHost for MemoryHost {
    fn unattended(&self, run: &mut dyn FnMut()) {
        self.tasks.unattended(run)
    }
    fn redact_text(&self, text: &str) -> String {
        self.tasks.redact_text(text)
    }
    fn tasks_base(&self) -> Option<PathBuf> {
        self.tasks.tasks_base().or_else(|| self.secrets.data_dir())
    }
    fn now(&self) -> i64 {
        self.tasks.now()
    }
    fn task_call_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value, String> {
        self.tasks.task_call_tool(server, tool, args)
    }
    fn task_run_script(
        &self,
        source: &str,
        args: Value,
        servers: &[String],
    ) -> Result<Value, String> {
        self.tasks.task_run_script(source, args, servers)
    }
    fn task_set_secret(&self, server: &str, key: &str, value: &str) -> Result<(), String> {
        self.tasks.task_set_secret(server, key, value)
    }
    fn task_secret_is_set(&self, server: &str, key: &str) -> bool {
        self.tasks.task_secret_is_set(server, key)
    }
    fn task_restart_server(&self, server: &str) -> Result<String, String> {
        self.tasks.task_restart_server(server)
    }
    fn task_server_installed(&self, server: &str) -> bool {
        match self.registry.list_servers() {
            Ok(servers) if !servers.is_empty() => servers.iter().any(|s| s.id == server),
            _ => self.tasks.task_server_installed(server),
        }
    }
    fn task_run_process(
        &self,
        spec: &crate::tasks::ProcessSpec,
        cancelled: &dyn Fn() -> bool,
        started: &dyn Fn(u32),
    ) -> crate::tasks::ProcessOutcome {
        self.tasks.task_run_process(spec, cancelled, started)
    }
    fn task_claude_program(&self) -> String {
        self.tasks.task_claude_program()
    }
    fn task_spawn_runner(&self, run_id: &str) -> Result<(), String> {
        self.tasks.task_spawn_runner(run_id)
    }
    fn task_process_alive(&self, pid: u32) -> bool {
        self.tasks.task_process_alive(pid)
    }
    fn task_kill_group(&self, pid: u32) {
        self.tasks.task_kill_group(pid)
    }
    fn task_approve_run(&self, ask: &crate::tasks::RunApproval) -> crate::tasks::RunDecision {
        self.tasks.task_approve_run(ask)
    }
}

/// The setup seam over the in-memory registry and vault: servers are listed by name with a plain
/// stdio shape, an import adds them as the disabled stdio servers `add_stdio_server` makes, and
/// the access document holds the sets of the registry state.
impl crate::SetupHost for MemoryHost {
    fn setup_servers(&self) -> Result<Vec<Value>, String> {
        Ok(self
            .registry
            .list_servers()?
            .into_iter()
            .map(|server| {
                serde_json::json!({
                    "id": server.id,
                    "name": server.name,
                    "transport": server.text("transport").unwrap_or("stdio"),
                    "command": server.text("command"),
                    "args": server.strings("args"),
                    "env": [],
                    "disabled": !server.enabled,
                    "forkSwitches": {},
                })
            })
            .collect())
    }

    fn setup_add_servers(&self, servers: &[Value]) -> Result<Vec<crate::AddedServer>, String> {
        let mut added = Vec::new();
        for doc in servers {
            let name = doc["name"].as_str().ok_or("a server has no name")?;
            let exists = self
                .registry
                .list_servers()?
                .iter()
                .any(|server| server.name.eq_ignore_ascii_case(name));
            if exists {
                continue;
            }
            let command = doc["command"].as_str().unwrap_or("placeholder");
            let mut id = String::new();
            self.registry.transact(&mut |tx| {
                id = tx.add_stdio_server(name, command, "shared")?;
                Ok(())
            })?;
            added.push(crate::AddedServer {
                name: name.into(),
                id,
            });
        }
        Ok(added)
    }

    fn setup_update_server(&self, doc: &Value) -> Result<crate::UpdatedServer, String> {
        let name = doc["name"].as_str().ok_or("a server has no name")?;
        let before = self
            .registry
            .list_servers()?
            .into_iter()
            .find(|server| server.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("no server named {name}"))?;
        let id = before.id.clone();
        let after = self.registry.edit(&id, |server| {
            let command = doc["command"].as_str().unwrap_or("placeholder");
            server
                .extra
                .insert("command".into(), Value::String(command.into()));
            if server.text("command") != before.text("command") {
                server.enabled = false;
            }
            Ok(server.clone())
        })?;
        Ok(crate::UpdatedServer {
            name: before.name.clone(),
            id,
            before: serde_json::to_value(&before).map_err(|e| e.to_string())?,
            after: serde_json::to_value(&after).map_err(|e| e.to_string())?,
        })
    }

    fn setup_restore_server(&self, updated: &crate::UpdatedServer) -> Result<(), String> {
        let before: ServerRecord =
            serde_json::from_value(updated.before.clone()).map_err(|e| e.to_string())?;
        self.registry.edit(&updated.id, |server| {
            if serde_json::to_value(&*server).map_err(|e| e.to_string())? != updated.after {
                return Err(format!("{} changed since the update", updated.name));
            }
            *server = before;
            Ok(())
        })
    }

    fn setup_access(&self) -> Result<Value, String> {
        let servers = self.registry.list_servers()?;
        let name = |id: &String| {
            servers
                .iter()
                .find(|server| &server.id == id)
                .map_or(id.clone(), |server| server.name.clone())
        };
        let sets: Vec<Value> = self
            .registry
            .access_sets
            .borrow()
            .iter()
            .map(|set| {
                serde_json::json!({
                    "id": set.id,
                    "name": set.name,
                    "servers": set.server_ids.iter().map(name).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(serde_json::json!({
            "accessSets": sets,
            "defaultAccess": self.registry.default_access.borrow().clone(),
            "clientScopes": {},
            "clientDiscovery": {},
            "folderProfiles": [],
            "discoveryMode": null,
            "clients": [],
        }))
    }

    fn setup_merge_access(&self, doc: &Value) -> Result<Value, String> {
        let mut added = Vec::new();
        let servers = self.registry.list_servers()?;
        for set in doc["accessSets"].as_array().into_iter().flatten() {
            let name = set["name"].as_str().unwrap_or_default();
            if self
                .registry
                .access_sets
                .borrow()
                .iter()
                .any(|existing| existing.name.eq_ignore_ascii_case(name))
            {
                continue;
            }
            let ids: Vec<String> = set["servers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter_map(|server| servers.iter().find(|s| s.name == server))
                .map(|server| server.id.clone())
                .collect();
            let id = slug(name);
            self.registry
                .access_sets
                .borrow_mut()
                .push(crate::AccessSetInfo {
                    id: id.clone(),
                    name: name.into(),
                    server_ids: ids,
                });
            added.push(id);
        }
        Ok(serde_json::json!({"accessSets": added}))
    }

    fn setup_unmerge_access(&self, added: &Value) -> Result<(), String> {
        let ids: Vec<&str> = added["accessSets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        self.registry
            .access_sets
            .borrow_mut()
            .retain(|set| !ids.contains(&set.id.as_str()));
        Ok(())
    }

    fn setup_settings(&self) -> Result<Map<String, Value>, String> {
        Ok(self.registry.unknown_fields.borrow().clone())
    }

    fn setup_apply_settings(
        &self,
        changes: &Map<String, Value>,
    ) -> Result<Map<String, Value>, String> {
        let mut fields = self.registry.unknown_fields.borrow_mut();
        let mut previous = Map::new();
        for (key, value) in changes {
            previous.insert(
                key.clone(),
                fields
                    .insert(key.clone(), value.clone())
                    .unwrap_or(Value::Null),
            );
        }
        Ok(previous)
    }

    fn setup_secrets(&self) -> Result<Vec<crate::StaticSecret>, String> {
        let servers = self.registry.list_servers()?;
        Ok(self
            .secrets
            .vault
            .borrow()
            .iter()
            .map(|((server, key), value)| crate::StaticSecret {
                server: servers
                    .iter()
                    .find(|s| &s.id == server)
                    .map_or(server.clone(), |s| s.name.clone()),
                key: key.clone(),
                kind: "env".into(),
                value: value.clone(),
            })
            .collect())
    }

    fn upstream_sync_active(&self) -> Result<bool, String> {
        Ok(self.sync.upstream_active.get())
    }

    fn sync_passphrase(&self, unattended: bool) -> Result<Option<String>, String> {
        if unattended && self.sync.keyring_locked.get() {
            return Err("the keyring is locked".into());
        }
        Ok(self.sync.passphrase.borrow().clone())
    }

    fn sync_store_passphrase(&self, passphrase: &str) -> Result<(), String> {
        *self.sync.passphrase.borrow_mut() = Some(passphrase.to_string());
        Ok(())
    }

    fn sync_clear_passphrase(&self) -> Result<(), String> {
        *self.sync.passphrase.borrow_mut() = None;
        Ok(())
    }

    fn keyring_locked(&self) -> bool {
        self.sync.keyring_locked.get()
    }

    fn setup_store_secret(&self, secret: &crate::StaticSecret) -> Result<(), String> {
        let id = self
            .registry
            .list_servers()?
            .into_iter()
            .find(|server| server.name == secret.server)
            .map(|server| server.id)
            .ok_or_else(|| format!("no server named {}", secret.server))?;
        self.put_in_vault(&id, &secret.key, &secret.value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_route_for_picks_the_longest_mapping_on_a_path_boundary() {
        let state = MemoryState {
            access_sets: vec![AccessSetInfo {
                id: "wide".into(),
                name: "Wide".into(),
                server_ids: vec![],
            }],
            folder_routes: vec![
                ("/a".into(), "wide".into()),
                ("/a/b/".into(), "wide".into()),
                ("/z".into(), "wide".into()),
                ("".into(), "wide".into()),
            ],
            ..Default::default()
        };
        let key = |root: &str| state.folder_route_for(root).map(|scope| scope.key);
        assert_eq!(key("/a/b/c").as_deref(), Some("/a/b/"));
        assert_eq!(key("/a/b").as_deref(), Some("/a/b/"));
        assert_eq!(key("/a/bc").as_deref(), Some("/a"));
        assert_eq!(key("/a\\b\\c").as_deref(), Some("/a/b/"));
        assert_eq!(key("/zz"), None);
        assert_eq!(key(""), None);
    }

    #[test]
    fn memory_transaction_drops_the_edit_when_the_closure_fails() {
        let registry = MemoryRegistry::with_servers(vec![]);
        let result = registry.transact(&mut |tx| {
            tx.add_stdio_server("Probe", "probe-mcp", "test:probe")?;
            Err("stop".into())
        });
        assert!(result.is_err());
        assert!(registry.servers.borrow().is_empty());
    }

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

    fn with_sets(ids: &[&str]) -> MemoryRegistry {
        let registry = MemoryRegistry::with_servers(vec![]);
        *registry.access_sets.borrow_mut() = ids
            .iter()
            .map(|id| AccessSetInfo {
                id: (*id).into(),
                name: id.to_uppercase(),
                server_ids: vec![],
            })
            .collect();
        registry
    }

    #[test]
    fn test01_memory_access_set_writes_follow_upstream_rules() {
        let registry = with_sets(&["work"]);
        registry
            .transact(&mut |tx| {
                assert_eq!(tx.create_access_set(" Deep  Work ")?, "deep-work");
                assert_eq!(tx.create_access_set("deep work")?, "deep-work-2");
                assert!(tx.create_access_set("@all").is_err());
                assert!(tx.create_access_set("  ").is_err());
                tx.set_default_access(Some("WORK"))?;
                assert!(tx.delete_access_set("work").is_err());
                tx.delete_access_set("deep-work-2")
            })
            .unwrap();
        assert_eq!(registry.default_access.borrow().as_deref(), Some("work"));
        assert_eq!(registry.access_sets.borrow().len(), 2);
        let only = with_sets(&["work"]);
        assert!(only
            .transact(&mut |tx| tx.delete_access_set("work"))
            .is_err());
    }

    #[test]
    fn test02_memory_folder_routes_replace_by_path_and_need_an_existing_set() {
        let registry = with_sets(&["work", "focus"]);
        registry
            .transact(&mut |tx| {
                tx.set_folder_route("/a", "work")?;
                tx.set_folder_route("/b", "FOCUS".to_lowercase().as_str())?;
                tx.set_folder_route("/a", "focus")?;
                assert!(tx.set_folder_route("/c", "gone").is_err());
                assert!(tx.set_folder_route("", "work").is_err());
                tx.remove_folder_route("/b")
            })
            .unwrap();
        assert_eq!(
            *registry.folder_routes.borrow(),
            vec![("/a".to_string(), "focus".to_string())]
        );
    }

    #[test]
    fn test03_memory_access_tools_narrow_and_restore() {
        let registry = with_sets(&["work"]);
        registry
            .transact(&mut |tx| tx.set_access_tools("work", "s", Some(vec!["t".into()])))
            .unwrap();
        assert_eq!(registry.tool_scopes.borrow()["work"]["s"], ["t"]);
        registry
            .transact(&mut |tx| tx.set_access_tools("work", "s", None))
            .unwrap();
        assert!(registry.tool_scopes.borrow().is_empty());
        assert!(registry
            .transact(&mut |tx| tx.set_access_tools("gone", "s", None))
            .is_err());
    }
}
