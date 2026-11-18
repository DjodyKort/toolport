//! Implements the `toolport-plus-api` traits over upstream items.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use toolport_plus_api::{
    convert, plus_ops, AccessSetInfo, AuthProbe, CallsInFlight, CatalogAccess, CatalogEntryRecord,
    ContextHost, DirectEntry, EnvEntry, ForkServerFields, GatewayRouter, HostEnv, HttpAccess,
    HttpBody, HttpError, HttpRequest, HttpResponse, McpmSource, ProcessOutcome, ProcessSpec,
    RegistryAccess, RegistryPlus, RegistryTx, RegistryView, RestartOutcome, RunApproval,
    RunDecision, SecretsHost, SelfMcpRecord, ServerRecord, ServerRules, ServerSpec, SetupHost,
    TaskHost, TrustInputs, VaultAccess,
};

pub struct HostAdapter;

/// Edits the fork-owned keys of one server inside `registry::update`; upstream's
/// own fields on the entry are never touched.
fn edit_server_fork_fields<T>(
    server_id: &str,
    edit: impl FnOnce(&mut ForkServerFields) -> Result<T, String>,
) -> Result<T, String> {
    crate::registry::update(|registry| {
        let server = registry
            .servers
            .iter_mut()
            .find(|server| server.id == server_id)
            .ok_or_else(|| format!("unknown server {server_id}"))?;
        let mut fields = ForkServerFields::extract(&server.unknown_fields)
            .map_err(|error| format!("server {server_id}: {error}"))?;
        let out = edit(&mut fields)?;
        fields.store(&mut server.unknown_fields)?;
        Ok(out)
    })
    .map(|(_, out)| out)
}

fn edit_plus<T>(
    edit: impl FnOnce(&mut Map<String, Value>) -> Result<T, String>,
) -> Result<T, String> {
    crate::registry::update(|registry| edit(&mut registry.unknown_fields)).map(|(_, out)| out)
}

fn mirror(entry: &crate::registry::ServerEntry) -> Result<ServerRecord, String> {
    convert(entry).map_err(|error| format!("server {}: {error}", entry.id))
}

fn view_servers(registry: &crate::registry::Registry) -> Result<Vec<ServerRecord>, String> {
    registry
        .servers
        .iter()
        .map(|entry| {
            let mut record = mirror(entry)?;
            record.enabled = registry.server_enabled(&entry.id);
            Ok(record)
        })
        .collect()
}

fn view_access_sets(registry: &crate::registry::Registry) -> Vec<AccessSetInfo> {
    registry
        .profiles
        .iter()
        .map(|profile| AccessSetInfo {
            id: profile.id.clone(),
            name: profile.name.clone(),
            server_ids: profile.enabled_server_ids.clone(),
        })
        .collect()
}

/// Below version 3 the loader turns the active profile into the legacy context of an implicit
/// default, which is every enabled server; only an explicit set is reported.
fn view_default_access(registry: &crate::registry::Registry) -> Option<String> {
    if registry.version < 3 {
        return None;
    }
    registry
        .default_access_profile_id
        .clone()
        .filter(|id| registry.profiles.iter().any(|profile| &profile.id == id))
}

fn view_client_access_sets(registry: &crate::registry::Registry) -> Vec<String> {
    let mut ids: Vec<String> = registry
        .client_scopes
        .values()
        .filter_map(|reference| registry.canonical_profile_id(reference).ok())
        .filter(|id| registry.profiles.iter().any(|profile| &profile.id == id))
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn view_selfmcp_record(
    registry: &crate::registry::Registry,
) -> Result<Option<SelfMcpRecord>, String> {
    Ok(RegistryPlus::extract(&registry.unknown_fields)?.selfmcp)
}

macro_rules! registry_view {
    ($ty:ty, $registry:ident) => {
        impl RegistryView for $ty {
            fn version(&self) -> u32 {
                self.$registry().version
            }
            fn servers(&self) -> Result<Vec<ServerRecord>, String> {
                view_servers(self.$registry())
            }
            fn access_sets(&self) -> Vec<AccessSetInfo> {
                view_access_sets(self.$registry())
            }
            fn default_access(&self) -> Option<String> {
                view_default_access(self.$registry())
            }
            fn client_access_sets(&self) -> Vec<String> {
                view_client_access_sets(self.$registry())
            }
            fn selfmcp_record(&self) -> Result<Option<SelfMcpRecord>, String> {
                view_selfmcp_record(self.$registry())
            }
        }
    };
}

/// A registry read straight from its file.
struct ReadOnlyView(crate::registry::Registry);

impl ReadOnlyView {
    fn registry(&self) -> &crate::registry::Registry {
        &self.0
    }
}

/// The registry inside `registry::update`.
struct TxView<'a>(&'a mut crate::registry::Registry);

impl TxView<'_> {
    fn registry(&self) -> &crate::registry::Registry {
        self.0
    }

    fn entry(&mut self, server_id: &str) -> Result<&mut crate::registry::ServerEntry, String> {
        self.0
            .servers
            .iter_mut()
            .find(|server| server.id == server_id)
            .ok_or_else(|| format!("unknown server {server_id}"))
    }

    fn fork_fields(
        &mut self,
        server_id: &str,
        edit: impl FnOnce(&mut ForkServerFields),
    ) -> Result<(), String> {
        let entry = self.entry(server_id)?;
        let mut fields = ForkServerFields::extract(&entry.unknown_fields)
            .map_err(|error| format!("server {server_id}: {error}"))?;
        edit(&mut fields);
        fields.store(&mut entry.unknown_fields)
    }
}

fn server_fields(spec: &ServerSpec) -> crate::registry_controller::ServerFields {
    crate::registry_controller::ServerFields {
        sync_local_only: false,
        initialize_timeout_ms: None,
        name: spec.name.clone(),
        transport: spec.transport.clone(),
        command: spec.command.clone(),
        args: spec.args.clone(),
        url: spec.url.clone(),
        cwd: spec.cwd.clone(),
        inherit_env: spec.inherit_env,
        hide_prompts: None,
    }
}

registry_view!(ReadOnlyView, registry);
registry_view!(TxView<'_>, registry);

impl RegistryTx for TxView<'_> {
    fn add_stdio_server(
        &mut self,
        name: &str,
        command: &str,
        source: &str,
    ) -> Result<String, String> {
        let entry: crate::registry::ServerEntry = serde_json::from_value(serde_json::json!({
            "id": "",
            "name": name,
            "transport": "stdio",
            "command": command,
            "source": source,
        }))
        .map_err(|error| error.to_string())?;
        Ok(self.0.add_server(entry))
    }

    fn repoint_stdio_server(&mut self, server_id: &str, command: &str) -> Result<(), String> {
        let entry = self.entry(server_id)?;
        entry.transport = "stdio".into();
        entry.command = Some(command.to_string());
        entry.url = None;
        Ok(())
    }

    fn remove_server(&mut self, server_id: &str) -> Result<(), String> {
        self.0.remove_server(server_id)
    }

    fn set_server_enabled(&mut self, server_id: &str, enabled: bool) -> Result<(), String> {
        let profile = self.0.active_profile_id();
        self.0.set_server_enabled(&profile, server_id, enabled)
    }

    fn set_access_server(
        &mut self,
        access_set_id: &str,
        server_id: &str,
        included: bool,
    ) -> Result<(), String> {
        self.0.set_access_server(access_set_id, server_id, included)
    }

    fn set_selfmcp_record(&mut self, record: Option<SelfMcpRecord>) -> Result<(), String> {
        plus_ops::set_selfmcp(&mut self.0.unknown_fields, record.as_ref())
    }

    fn add_server(&mut self, spec: &ServerSpec) -> Result<String, String> {
        crate::registry_controller::apply_add_server(self.0, server_fields(spec))
    }

    fn add_catalog_server(&mut self, entry: &CatalogEntryRecord) -> Result<String, String> {
        let entry: crate::catalog::CatalogEntry = convert(entry)?;
        if entry.url.is_none() && entry.command.is_none() {
            return Err(format!(
                "{} needs its own endpoint URL. Open it from the catalog to enter one.",
                entry.name
            ));
        }
        Ok(crate::registry_controller::apply_add_entry(
            self.0,
            crate::registry_controller::catalog_server(entry),
        ))
    }

    fn update_server(&mut self, server_id: &str, spec: &ServerSpec) -> Result<(), String> {
        crate::registry_controller::apply_update_server_fields(
            self.0,
            server_id,
            server_fields(spec),
        )
    }

    fn set_server_env(&mut self, server_id: &str, env: Vec<EnvEntry>) -> Result<(), String> {
        self.entry(server_id)?.env = convert(&env)?;
        Ok(())
    }

    fn set_forward_instructions(&mut self, server_id: &str, enabled: bool) -> Result<(), String> {
        self.fork_fields(server_id, |fields| {
            fields.forward_instructions = enabled.then_some(true)
        })
    }

    fn set_declare_client_capabilities(
        &mut self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        self.fork_fields(server_id, |fields| {
            fields.declare_client_capabilities = enabled.then_some(true)
        })
    }
}

impl RegistryAccess for HostAdapter {
    fn read_view(&self) -> Result<Option<Box<dyn RegistryView>>, String> {
        let Some(path) = crate::registry::resolved_path() else {
            return Ok(None);
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map(|registry| Some(Box::new(ReadOnlyView(registry)) as Box<dyn RegistryView>))
                .map_err(|error| format!("registry is not readable: {}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("cannot read registry: {error}")),
        }
    }

    fn transact(
        &self,
        edit: &mut dyn FnMut(&mut dyn RegistryTx) -> Result<(), String>,
    ) -> Result<(), String> {
        crate::registry::update(|registry| edit(&mut TxView(registry))).map(|_| ())
    }

    fn registry_path(&self) -> Option<PathBuf> {
        crate::registry::resolved_path()
    }

    fn list_servers(&self) -> Result<Vec<ServerRecord>, String> {
        let registry = crate::registry::load_resolved()?;
        registry.servers.iter().map(mirror).collect()
    }

    fn get_server(&self, server_id: &str) -> Result<Option<ServerRecord>, String> {
        let registry = crate::registry::load_resolved()?;
        registry
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .map(mirror)
            .transpose()
    }

    fn profile_count(&self) -> Result<usize, String> {
        Ok(crate::registry::load_resolved()?.profiles.len())
    }

    fn set_server_enabled(&self, server_id: &str, enabled: bool) -> Result<(), String> {
        crate::registry::update(|registry| {
            let server = registry
                .servers
                .iter_mut()
                .find(|server| server.id == server_id)
                .ok_or_else(|| format!("unknown server {server_id}"))?;
            server.enabled = enabled;
            Ok(())
        })
        .map(|_| ())
    }

    fn plus_data(&self) -> Result<RegistryPlus, String> {
        RegistryPlus::extract(&crate::registry::load_resolved()?.unknown_fields)
    }

    fn set_selfmcp_record(&self, record: Option<SelfMcpRecord>) -> Result<(), String> {
        edit_plus(|fields| plus_ops::set_selfmcp(fields, record.as_ref()))
    }

    fn add_selfmcp_enabled_in(&self, profile_ids: &[String]) -> Result<(), String> {
        edit_plus(|fields| {
            plus_ops::update_selfmcp(fields, |record| {
                let listed = record.enabled_in.get_or_insert_with(Vec::new);
                for id in profile_ids {
                    if !listed.contains(id) {
                        listed.push(id.clone());
                    }
                }
            })
        })
    }

    fn set_selfmcp_opt_out(&self, opt_out: bool) -> Result<(), String> {
        edit_plus(|fields| {
            plus_ops::update_selfmcp(fields, |record| {
                record.opt_out = opt_out.then_some(true);
            })
        })
    }

    fn set_direct_entry(
        &self,
        client: &str,
        entry: &str,
        record: DirectEntry,
    ) -> Result<(), String> {
        edit_plus(|fields| plus_ops::set_direct_entry(fields, client, entry, &record))
    }

    fn clear_direct_entry(&self, client: &str, entry: &str) -> Result<bool, String> {
        edit_plus(|fields| Ok(plus_ops::clear_direct_entry(fields, client, entry)))
    }

    fn set_server_auth_probe(
        &self,
        server_id: &str,
        probe: Option<AuthProbe>,
    ) -> Result<(), String> {
        edit_server_fork_fields(server_id, |fields| {
            match probe {
                Some(probe) => {
                    fields.plus.get_or_insert_with(Default::default).auth_probe = Some(probe);
                }
                None => {
                    if let Some(plus) = fields.plus.as_mut() {
                        plus.auth_probe = None;
                        if plus.is_empty() {
                            fields.plus = None;
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
        edit_server_fork_fields(server_id, |fields| {
            fields.mcpm_source = source;
            Ok(())
        })
    }

    fn merge_server_mcpm_source(
        &self,
        server_id: &str,
        values: Map<String, Value>,
    ) -> Result<(), String> {
        edit_server_fork_fields(server_id, |fields| {
            fields.mcpm_source = Some(
                fields
                    .mcpm_source
                    .take()
                    .unwrap_or_default()
                    .merged(values)?,
            );
            Ok(())
        })
    }

    fn set_server_mcpm_oauth(&self, server_id: &str, oauth: Option<bool>) -> Result<(), String> {
        edit_server_fork_fields(server_id, |fields| {
            fields.mcpm_oauth = oauth;
            Ok(())
        })
    }

    fn set_server_mcpm_tags(
        &self,
        server_id: &str,
        tags: Option<Vec<String>>,
    ) -> Result<(), String> {
        edit_server_fork_fields(server_id, |fields| {
            fields.mcpm_tags = tags;
            Ok(())
        })
    }

    fn set_server_forward_instructions(
        &self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        edit_server_fork_fields(server_id, |fields| {
            fields.forward_instructions = enabled.then_some(true);
            Ok(())
        })
    }

    fn set_server_declare_client_capabilities(
        &self,
        server_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        edit_server_fork_fields(server_id, |fields| {
            fields.declare_client_capabilities = enabled.then_some(true);
            Ok(())
        })
    }

    fn set_server_max_request_timeout_ms(
        &self,
        server_id: &str,
        timeout_ms: Option<u64>,
    ) -> Result<(), String> {
        edit_server_fork_fields(server_id, |fields| {
            fields.max_request_timeout_ms = timeout_ms;
            Ok(())
        })
    }

    fn set_server_mode(&self, server_id: &str, mode: Option<&str>) -> Result<(), String> {
        edit_server_fork_fields(server_id, |fields| {
            fields.server_mode = mode.map(String::from);
            Ok(())
        })
    }
}

impl ContextHost for HostAdapter {
    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }

    fn count_tokens(&self, text: &str) -> u64 {
        crate::savings::count_tokens(text)
    }
}

impl HostEnv for HostAdapter {
    fn app_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn use_data_dir(&self, dir: &std::path::Path) {
        std::env::set_var("TOOLPORT_DATA_DIR", dir);
    }

    fn seam_live(&self) -> bool {
        "https://toolport.app/"
            .parse()
            .is_ok_and(|uri| crate::seam::egress_check(&uri).is_err())
    }

    fn launch_at_login(&self) -> Option<bool> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let (dir, extension) = if cfg!(target_os = "macos") {
            (home.join("Library/LaunchAgents"), "plist")
        } else if cfg!(target_os = "linux") {
            (home.join(".config/autostart"), "desktop")
        } else {
            return None;
        };
        let found = std::fs::read_dir(dir).ok().is_some_and(|entries| {
            entries.flatten().any(|entry| {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                name.contains("toolport") && name.ends_with(extension)
            })
        });
        Some(found)
    }
}

impl toolport_plus_api::SettingsStore for HostAdapter {
    fn claude_settings_paths(&self) -> Vec<PathBuf> {
        crate::clients::claude_settings_paths()
    }

    fn read_settings(&self, path: &Path) -> Result<(Value, Option<String>), String> {
        crate::clients::read_settings_json(path)
    }

    fn write_hooks(&self, path: &Path, original: Option<&str>, root: &Value) -> Result<(), String> {
        crate::clients::write_settings_json(path, original, root)
    }

    fn write_key(
        &self,
        path: &Path,
        original: Option<&str>,
        root: &Value,
        key: &str,
    ) -> Result<(), String> {
        crate::clients::write_settings_key(path, original, root, key)
    }
}

#[cfg(target_os = "macos")]
static MASTER_KEY: toolport_plus::login_keychain::KeyCache =
    toolport_plus::login_keychain::KeyCache::new();

/// The macOS calls live in upstream's `secrets::platform`; off macOS the mode is never
/// available, so no other method is reached.
impl SecretsHost for HostAdapter {
    fn data_dir(&self) -> Option<PathBuf> {
        crate::registry::conduit_dir()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn login_keychain_available(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn read_master_key(&self) -> Result<Option<[u8; 32]>, String> {
        #[cfg(target_os = "macos")]
        return MASTER_KEY.read(
            crate::secrets::prompts_blocked(),
            crate::secrets::platform::read_master_key,
            crate::secrets::note_refused_ask,
        );
        #[cfg(not(target_os = "macos"))]
        Ok(None)
    }

    fn ensure_master_key(&self) -> Result<[u8; 32], String> {
        #[cfg(target_os = "macos")]
        return MASTER_KEY.ensure(
            crate::secrets::prompts_blocked(),
            crate::secrets::platform::ensure_master_key,
            crate::secrets::note_refused_ask,
        );
        #[cfg(not(target_os = "macos"))]
        Err("no login keychain on this OS".to_string())
    }

    fn rewrite_master_key_acl(&self) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        {
            use crate::secrets::platform;
            use base64::Engine as _;
            let Some(key) = platform::read_master_key()? else {
                return Ok(false);
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(key);
            platform::add_with_shared_access(platform::MASTER_KEY_ACCOUNT, &encoded)?;
            Ok(true)
        }
        #[cfg(not(target_os = "macos"))]
        Ok(false)
    }

    fn trust_inputs(&self) -> Result<TrustInputs, String> {
        let home = dirs::home_dir();
        Ok(TrustInputs {
            exe: std::env::current_exe().map_err(|error| error.to_string())?,
            gateway: crate::clients::resolve_gateway_path().ok().flatten(),
            data_bin: crate::registry::conduit_dir().map(|dir| dir.join("bin")),
            extra_dirs: home
                .into_iter()
                .flat_map(|home| {
                    [
                        home.join(".local").join("bin"),
                        home.join(".cargo").join("bin"),
                    ]
                })
                .collect(),
        })
    }

    fn delete_server_secret(&self, server_id: &str, key: &str) -> Result<(), String> {
        crate::secrets::delete_secret(server_id, key)
    }
}

const PLACEHOLDER_REFUSED: &str = "Enter a credential value. Placeholder values cannot be saved.";

fn refuse_placeholder(value: &str) -> Result<(), String> {
    if crate::import_credentials::provided(value) {
        Ok(())
    } else {
        Err(PLACEHOLDER_REFUSED.to_string())
    }
}

/// The same controllers the app's secret commands run, so a value set here and one set in the
/// app land in the same vault entry and the same registry declaration.
impl VaultAccess for HostAdapter {
    fn read_vault_secret(&self, server_id: &str, key: &str) -> Result<Option<String>, String> {
        crate::secrets::get_vault_secret_result(server_id, key)
    }

    fn store_env_secret(&self, server_id: &str, key: &str, value: &str) -> Result<(), String> {
        refuse_placeholder(value)?;
        crate::registry_controller::set_server_secret_with(
            server_id,
            key,
            value,
            |server_id, key| {
                let (registry, ()) = crate::registry::update(|registry| {
                    crate::registry_controller::apply_secret_declaration(registry, server_id, key)
                })?;
                Ok(registry)
            },
        )
        .map(|_| ())
    }

    fn store_launch_secret(&self, server_id: &str, key: &str, value: &str) -> Result<(), String> {
        refuse_placeholder(value)?;
        crate::registry_controller::set_launch_secret(server_id, key, value).map(|_| ())
    }

    fn store_access_token(&self, server_id: &str, token: &str) -> Result<(), String> {
        refuse_placeholder(token)?;
        crate::registry_controller::set_auth_token(server_id, token)
    }

    fn remove_env_secret(&self, server_id: &str, key: &str) -> Result<(), String> {
        crate::registry_controller::delete_server_secret(server_id, key).map(|_| ())
    }

    fn clear_access_token(&self, server_id: &str) -> Result<(), String> {
        crate::registry_controller::clear_auth_token(server_id)
    }
}

impl ServerRules for HostAdapter {
    fn credential_text(&self, text: &str) -> bool {
        crate::registry::arg_looks_secret(text) || crate::registry::redact_secret_text(text) != text
    }

    fn spawn_env_refused(&self, key: &str, value: &str) -> bool {
        crate::downstream::screen_spawn_env(&[(key.to_string(), value.to_string())]).is_err()
    }
}

/// Registry settings that travel between machines. Ports, tokens and paths stay behind.
const SETUP_SETTINGS: [&str; 12] = [
    "safetyLevel",
    "lazyDiscovery",
    "discoveryMode",
    "codeMode",
    "piiRedaction",
    "blockOnInjection",
    "quarantineOnDrift",
    "humanApproval",
    "denyDestructive",
    "confirmDestructive",
    "gatewayInstructions",
    "liveInspect",
];

fn setup_fork_switches(server: &crate::registry::ServerEntry) -> Value {
    let Ok(fields) = ForkServerFields::extract(&server.unknown_fields) else {
        return Value::Object(Map::new());
    };
    let mut switches = Map::new();
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(value) = value {
            switches.insert(key.into(), value);
        }
    };
    put(
        "forwardInstructions",
        fields.forward_instructions.map(Value::from),
    );
    put(
        "declareClientCapabilities",
        fields.declare_client_capabilities.map(Value::from),
    );
    put(
        "maxRequestTimeoutMs",
        fields.max_request_timeout_ms.map(Value::from),
    );
    put("serverMode", fields.server_mode.map(Value::from));
    put("mcpmOauth", fields.mcpm_oauth.map(Value::from));
    put("mcpmTags", fields.mcpm_tags.map(Value::from));
    Value::Object(switches)
}

fn setup_profile_name(registry: &crate::registry::Registry, reference: &str) -> String {
    registry
        .canonical_profile_id(reference)
        .ok()
        .and_then(|id| registry.profiles.iter().find(|p| p.id == id))
        .map_or_else(|| reference.to_string(), |profile| profile.name.clone())
}

fn setup_profile_id(registry: &crate::registry::Registry, name: &str) -> Option<String> {
    registry
        .profiles
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .map(|p| p.id.clone())
}

/// The registry's own servers, without the gateway entry and the ones kept local.
fn setup_eligible(
    registry: &crate::registry::Registry,
) -> impl Iterator<Item = &crate::registry::ServerEntry> {
    registry
        .servers
        .iter()
        .filter(|s| !crate::clients::is_gateway_server(s) && !crate::personal_sync::keep_local(s))
}

/// The registry as it is on disk. Inspection must not recover, migrate or write, so this reads the
/// file the way `read_view` does; no file is an empty registry.
fn read_registry() -> Result<crate::registry::Registry, String> {
    let empty = || {
        serde_json::from_str(r#"{"servers": [], "profiles": []}"#)
            .map_err(|e: serde_json::Error| e.to_string())
    };
    let Some(path) = crate::registry::resolved_path() else {
        return empty();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("registry is not readable: {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => empty(),
        Err(error) => Err(format!("cannot read registry: {error}")),
    }
}

impl SetupHost for HostAdapter {
    fn setup_servers(&self) -> Result<Vec<Value>, String> {
        let registry = read_registry()?;
        Ok(setup_eligible(&registry)
            .map(|server| {
                let mut doc = crate::personal_sync::export(server);
                if let Some(env) = doc.get_mut("env").and_then(Value::as_array_mut) {
                    env.retain(|entry| {
                        !crate::personal_sync::risky_sync_env(entry["key"].as_str().unwrap_or(""))
                    });
                }
                doc["forkSwitches"] = setup_fork_switches(server);
                doc
            })
            .collect())
    }

    fn setup_add_servers(
        &self,
        servers: &[Value],
    ) -> Result<Vec<toolport_plus_api::AddedServer>, String> {
        let before: std::collections::HashSet<String> = read_registry()?
            .servers
            .iter()
            .map(|s| s.name.to_lowercase())
            .collect();
        let documents: Vec<Value> = servers
            .iter()
            .map(|doc| {
                let mut doc = doc.clone();
                if let Some(map) = doc.as_object_mut() {
                    for key in ["id", "disabled", "forkSwitches"] {
                        map.remove(key);
                    }
                }
                doc
            })
            .collect();
        let text = serde_json::json!({"kind": "conduit-setup", "version": 1, "servers": documents})
            .to_string();
        crate::registry::update(|registry| {
            crate::sharing_controller::apply_import(registry, &text)
        })?;
        let registry = read_registry()?;
        let mut added = Vec::new();
        for doc in servers {
            let name = doc["name"].as_str().unwrap_or_default();
            if before.contains(&name.to_lowercase()) {
                continue;
            }
            let Some(entry) = registry
                .servers
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
            else {
                continue;
            };
            let switches = doc["forkSwitches"].clone();
            edit_server_fork_fields(&entry.id, |fields| {
                if let Some(on) = switches["forwardInstructions"].as_bool() {
                    fields.forward_instructions = Some(on);
                }
                if let Some(on) = switches["declareClientCapabilities"].as_bool() {
                    fields.declare_client_capabilities = Some(on);
                }
                if let Some(ms) = switches["maxRequestTimeoutMs"].as_u64() {
                    fields.max_request_timeout_ms = Some(ms);
                }
                if let Some(mode) = switches["serverMode"].as_str() {
                    fields.server_mode = Some(mode.to_string());
                }
                if let Some(on) = switches["mcpmOauth"].as_bool() {
                    fields.mcpm_oauth = Some(on);
                }
                if let Ok(tags) =
                    serde_json::from_value::<Vec<String>>(switches["mcpmTags"].clone())
                {
                    fields.mcpm_tags = Some(tags);
                }
                Ok(())
            })?;
            added.push(toolport_plus_api::AddedServer {
                name: entry.name.clone(),
                id: entry.id.clone(),
            });
        }
        Ok(added)
    }

    fn setup_access(&self) -> Result<Value, String> {
        let registry = read_registry()?;
        let server_name = |id: &String| {
            registry
                .servers
                .iter()
                .find(|s| &s.id == id)
                .map_or_else(|| id.clone(), |s| s.name.clone())
        };
        let sets: Vec<Value> = registry
            .profiles
            .iter()
            .map(|profile| {
                serde_json::json!({
                    "id": profile.id,
                    "name": profile.name,
                    "servers": profile.enabled_server_ids.iter().map(server_name).collect::<Vec<_>>(),
                })
            })
            .collect();
        let scopes: std::collections::BTreeMap<&String, String> = registry
            .client_scopes
            .iter()
            .filter(|(_, reference)| !reference.is_empty())
            .map(|(client, reference)| (client, setup_profile_name(&registry, reference)))
            .collect();
        let folders: Vec<Value> = registry
            .folder_profiles
            .iter()
            .map(|folder| {
                serde_json::json!({
                    "path": folder.path,
                    "profile": setup_profile_name(&registry, &folder.profile),
                })
            })
            .collect();
        let discovery: std::collections::BTreeMap<&String, &String> =
            registry.client_discovery.iter().collect();
        let mut clients: Vec<&String> = registry.client_managed_entries.keys().collect();
        clients.sort();
        Ok(serde_json::json!({
            "accessSets": sets,
            "defaultAccess": registry
                .default_access_profile_id
                .as_deref()
                .map(|id| setup_profile_name(&registry, id)),
            "clientScopes": scopes,
            "clientDiscovery": discovery,
            "folderProfiles": folders,
            "discoveryMode": registry.discovery_mode,
            "clients": clients,
        }))
    }

    fn setup_merge_access(&self, doc: &Value) -> Result<Value, String> {
        crate::registry::update(|registry| {
            let mut added = serde_json::json!({
                "accessSets": [], "clientScopes": [], "clientDiscovery": [], "folderProfiles": [],
            });
            for set in doc["accessSets"].as_array().into_iter().flatten() {
                let name = set["name"].as_str().unwrap_or_default();
                if name.is_empty() || setup_profile_id(registry, name).is_some() {
                    continue;
                }
                let ids: Vec<String> = set["servers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter_map(|server| {
                        registry
                            .servers
                            .iter()
                            .find(|s| s.name.eq_ignore_ascii_case(server))
                            .map(|s| s.id.clone())
                    })
                    .collect();
                let id = registry.add_profile(name);
                if let Some(profile) = registry.profiles.iter_mut().find(|p| p.id == id) {
                    profile.enabled_server_ids = ids;
                }
                added["accessSets"]
                    .as_array_mut()
                    .unwrap()
                    .push(Value::from(id));
            }
            for (client, set) in doc["clientScopes"].as_object().into_iter().flatten() {
                let has = registry
                    .client_scopes
                    .get(client)
                    .is_some_and(|v| !v.is_empty());
                let Some(id) = set
                    .as_str()
                    .and_then(|name| setup_profile_id(registry, name))
                else {
                    continue;
                };
                if !has {
                    registry.client_scopes.insert(client.clone(), id.clone());
                    added["clientScopes"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({"client": client, "set": id}));
                }
            }
            for (client, mode) in doc["clientDiscovery"].as_object().into_iter().flatten() {
                if let (false, Some(mode)) = (
                    registry.client_discovery.contains_key(client),
                    mode.as_str(),
                ) {
                    registry
                        .client_discovery
                        .insert(client.clone(), mode.to_string());
                    added["clientDiscovery"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({"client": client, "mode": mode}));
                }
            }
            for folder in doc["folderProfiles"].as_array().into_iter().flatten() {
                let (Some(path), Some(id)) = (
                    folder["path"].as_str(),
                    folder["profile"]
                        .as_str()
                        .and_then(|name| setup_profile_id(registry, name)),
                ) else {
                    continue;
                };
                if !registry.folder_profiles.iter().any(|f| f.path == path) {
                    registry
                        .folder_profiles
                        .push(crate::registry::FolderProfile {
                            path: path.to_string(),
                            profile: id.clone(),
                            unknown_fields: Default::default(),
                        });
                    added["folderProfiles"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({"path": path, "set": id}));
                }
            }
            Ok(added)
        })
        .map(|(_, added)| added)
    }

    fn setup_unmerge_access(&self, added: &Value) -> Result<(), String> {
        crate::registry::update(|registry| {
            for entry in added["clientScopes"].as_array().into_iter().flatten() {
                let (Some(client), Some(set)) = (entry["client"].as_str(), entry["set"].as_str())
                else {
                    continue;
                };
                if registry.client_scopes.get(client).map(String::as_str) == Some(set) {
                    registry.client_scopes.remove(client);
                }
            }
            for entry in added["clientDiscovery"].as_array().into_iter().flatten() {
                let (Some(client), Some(mode)) = (entry["client"].as_str(), entry["mode"].as_str())
                else {
                    continue;
                };
                if registry.client_discovery.get(client).map(String::as_str) == Some(mode) {
                    registry.client_discovery.remove(client);
                }
            }
            for entry in added["folderProfiles"].as_array().into_iter().flatten() {
                let (Some(path), Some(set)) = (entry["path"].as_str(), entry["set"].as_str())
                else {
                    continue;
                };
                registry
                    .folder_profiles
                    .retain(|f| !(f.path == path && f.profile == set));
            }
            for id in added["accessSets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let in_use = registry.client_scopes.values().any(|v| v == id)
                    || registry.folder_profiles.iter().any(|f| f.profile == id)
                    || registry.default_access_profile_id.as_deref() == Some(id)
                    || registry.active_profile_id.as_deref() == Some(id);
                if !in_use {
                    let _ = registry.remove_profile(id);
                }
            }
            Ok(())
        })
        .map(|_| ())
    }

    fn setup_settings(&self) -> Result<Map<String, Value>, String> {
        let registry = read_registry()?;
        let Value::Object(all) = serde_json::to_value(&registry).map_err(|e| e.to_string())? else {
            return Err("the registry did not serialise to an object".into());
        };
        Ok(SETUP_SETTINGS
            .iter()
            .filter_map(|key| all.get(*key).map(|v| ((*key).to_string(), v.clone())))
            .collect())
    }

    fn setup_apply_settings(
        &self,
        changes: &Map<String, Value>,
    ) -> Result<Map<String, Value>, String> {
        crate::registry::update(|registry| {
            let Value::Object(mut all) =
                serde_json::to_value(&*registry).map_err(|e| e.to_string())?
            else {
                return Err("the registry did not serialise to an object".to_string());
            };
            let mut previous = Map::new();
            for (key, value) in changes {
                if !SETUP_SETTINGS.contains(&key.as_str()) {
                    return Err(format!("{key} is not a setting that travels"));
                }
                previous.insert(key.clone(), all.get(key).cloned().unwrap_or(Value::Null));
                if value.is_null() {
                    all.remove(key);
                } else {
                    all.insert(key.clone(), value.clone());
                }
            }
            *registry = serde_json::from_value(Value::Object(all)).map_err(|e| e.to_string())?;
            Ok(previous)
        })
        .map(|(_, previous)| previous)
    }

    fn setup_secrets(&self) -> Result<Vec<toolport_plus_api::StaticSecret>, String> {
        let registry = read_registry()?;
        let mut found = Vec::new();
        let mut read =
            |server: &crate::registry::ServerEntry, key: &str, kind: &str| -> Result<(), String> {
                if let Some(value) = crate::secrets::get_vault_secret_result(&server.id, key)? {
                    found.push(toolport_plus_api::StaticSecret {
                        server: server.name.clone(),
                        key: key.to_string(),
                        kind: kind.to_string(),
                        value,
                    });
                }
                Ok(())
            };
        for server in setup_eligible(&registry) {
            for entry in server.env.iter().filter(|e| e.secret) {
                read(server, &entry.key, "env")?;
            }
            for input in server
                .launch
                .iter()
                .flat_map(|l| l.inputs.iter())
                .filter(|i| i.secret)
            {
                read(server, &input.key, "launch")?;
            }
            if server.url.is_some() {
                read(server, toolport_plus_api::HTTP_AUTH_KEY, "token")?;
            }
        }
        Ok(found)
    }

    fn setup_store_secret(&self, secret: &toolport_plus_api::StaticSecret) -> Result<(), String> {
        let registry = read_registry()?;
        let id = registry
            .servers
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&secret.server))
            .map(|s| s.id.clone())
            .ok_or_else(|| format!("no server named {}", secret.server))?;
        match secret.kind.as_str() {
            "env" => self.store_env_secret(&id, &secret.key, &secret.value),
            "launch" => self.store_launch_secret(&id, &secret.key, &secret.value),
            "token" => self.store_access_token(&id, &secret.value),
            other => Err(format!("unknown kind of secret: {other}")),
        }
    }
}

impl CatalogAccess for HostAdapter {
    fn curated(&self, query: &str) -> Vec<CatalogEntryRecord> {
        crate::catalog::search_curated(query)
            .iter()
            .filter_map(|entry| convert(entry).ok())
            .collect()
    }
}

/// The router one gateway process owns, as the restart listener sees it, with the launches it
/// keeps once per project folder.
pub(crate) struct RouterHandle {
    pub(crate) router: std::sync::Arc<crate::router::Router>,
    pub(crate) pooled: Vec<crate::seam::PooledLaunch>,
    pub(crate) dirty: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl RouterHandle {
    fn folders<'a>(
        &'a self,
        server_id: &'a str,
    ) -> impl Iterator<Item = (&'a String, &'a crate::router::SharedServerSlot)> {
        self.pooled
            .iter()
            .filter(move |(id, _, _)| id == server_id)
            .map(|(_, root, slot)| (root, slot))
    }
}

impl GatewayRouter for RouterHandle {
    fn holds_server(&self, server_id: &str) -> bool {
        self.router.plus_holds_server(server_id) || self.folders(server_id).next().is_some()
    }

    fn calls_in_flight(&self, server_id: &str) -> Option<Vec<CallsInFlight>> {
        let shared = self
            .router
            .plus_calls_in_flight(server_id)
            .map(|calls| CallsInFlight { root: None, calls });
        let instances: Vec<_> = shared
            .into_iter()
            .chain(self.folders(server_id).map(|(root, slot)| CallsInFlight {
                root: Some(root.clone()),
                calls: slot.plus_calls_in_flight(),
            }))
            .collect();
        (!instances.is_empty()).then_some(instances)
    }

    fn restart_server(&self, server_id: &str) -> Option<Vec<RestartOutcome>> {
        let shared = self
            .router
            .plus_restart_server(server_id)
            .map(|(restarted, detail)| RestartOutcome {
                root: None,
                restarted,
                detail,
            });
        // One thread per folder: each fresh process can take seconds to come up.
        let folders: Vec<_> = std::thread::scope(|scope| {
            self.folders(server_id)
                .map(|(root, slot)| {
                    scope.spawn(move || {
                        let (restarted, detail) = slot.plus_restart();
                        RestartOutcome {
                            root: Some(root.clone()),
                            restarted,
                            detail,
                        }
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect()
        });
        let instances: Vec<_> = shared.into_iter().chain(folders).collect();
        (!instances.is_empty()).then_some(instances)
    }

    fn mark_lists_stale(&self) {
        use crate::downstream::change;
        self.dirty.fetch_or(
            change::TOOLS | change::RESOURCES | change::PROMPTS,
            std::sync::atomic::Ordering::SeqCst,
        );
    }
}

const MAX_REDIRECTS: u32 = 5;

/// Headers a caller sets for one origin: they travel only to that origin.
const CROSS_ORIGIN_HEADERS: [&str; 3] = ["accept", "accept-language", "user-agent"];

/// Upstream's own agent, so the egress guard and upstream's resolvers apply to every request.
///
/// Redirects are followed here, hop by hop: ureq drops `Authorization` and `Cookie` on a
/// redirect but forwards every other header, so a token in `X-Api-Key` would reach another host.
impl HttpAccess for HostAdapter {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let (content_type, body) = encode_body(&request.body)?;
        let mut headers = request.headers.clone();
        if let Some(content_type) = content_type.filter(|_| !request.has_header("content-type")) {
            headers.push(("Content-Type".into(), content_type.into()));
        }
        let agent = crate::http_client::agent();
        let mut method = request.method.as_str().to_string();
        let mut url = url::Url::parse(&request.url)
            .map_err(|error| HttpError::Other(format!("invalid request: {error}")))?;
        let started = std::time::Instant::now();
        let mut body = body;
        let mut redirects = 0;
        loop {
            let mut builder = ureq::http::Request::builder()
                .method(method.as_str())
                .uri(url.as_str());
            for (name, value) in &headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
            let invalid =
                |error: ureq::http::Error| HttpError::Other(format!("invalid request: {error}"));
            let left = request.timeout.saturating_sub(started.elapsed());
            if left.is_zero() {
                return Err(HttpError::Timeout("the request deadline passed".into()));
            }
            let response = match &body {
                Some(bytes) => run(&agent, builder.body(bytes.clone()).map_err(invalid)?, left),
                None => run(&agent, builder.body(()).map_err(invalid)?, left),
            }
            .map_err(map_error)?;
            let Some(next) = request
                .follow_redirects
                .then(|| redirect_target(&response, &url))
                .flatten()
            else {
                return read_response(response, request.max_body_bytes);
            };
            let status = response.status().as_u16();
            let keeps_method = matches!(status, 307 | 308);
            if keeps_method && (method != "GET" || body.is_some()) {
                return read_response(response, request.max_body_bytes);
            }
            if redirects == MAX_REDIRECTS {
                return Err(HttpError::TooManyRedirects);
            }
            redirects += 1;
            if !keeps_method {
                method = "GET".into();
                body = None;
                headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
            }
            headers.retain(|(name, _)| !name.eq_ignore_ascii_case("cookie"));
            if origin(&next) != origin(&url) {
                headers.retain(|(name, _)| {
                    CROSS_ORIGIN_HEADERS
                        .iter()
                        .any(|kept| name.eq_ignore_ascii_case(kept))
                });
            }
            url = next;
        }
    }
}

fn origin(url: &url::Url) -> (String, Option<String>, Option<u16>) {
    (
        url.scheme().to_string(),
        url.host_str().map(str::to_string),
        url.port_or_known_default(),
    )
}

fn redirect_target(
    response: &crate::http_client::Response,
    current: &url::Url,
) -> Option<url::Url> {
    if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
        return None;
    }
    let location = response.headers().get("location")?.to_str().ok()?;
    current.join(location).ok()
}

fn run<B: ureq::AsSendBody>(
    agent: &ureq::Agent,
    http_request: ureq::http::Request<B>,
    timeout: std::time::Duration,
) -> Result<crate::http_client::Response, ureq::Error> {
    use ureq::RequestExt as _;

    http_request
        .with_agent(agent)
        .configure()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .max_redirects_will_error(false)
        .run()
}

fn encode_body(body: &HttpBody) -> Result<(Option<&'static str>, Option<Vec<u8>>), HttpError> {
    Ok(match body {
        HttpBody::Empty => (None, None),
        HttpBody::Json(value) => (
            Some("application/json"),
            Some(serde_json::to_vec(value).map_err(|error| HttpError::Other(error.to_string()))?),
        ),
        HttpBody::Form(pairs) => (
            Some("application/x-www-form-urlencoded"),
            Some(
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(pairs)
                    .finish()
                    .into_bytes(),
            ),
        ),
    })
}

fn read_response(
    mut response: crate::http_client::Response,
    max_body_bytes: usize,
) -> Result<HttpResponse, HttpError> {
    use std::io::Read as _;
    use ureq::ResponseExt as _;

    let status = response.status().as_u16();
    let url = response.get_uri().to_string();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            Some((name.as_str().to_string(), value.to_str().ok()?.to_string()))
        })
        .collect();
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(max_body_bytes as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| HttpError::Other(format!("reading the response failed: {error}")))?;
    let truncated = body.len() > max_body_bytes;
    body.truncate(max_body_bytes);
    Ok(HttpResponse {
        status,
        headers,
        body,
        truncated,
        url,
    })
}

fn map_error(error: ureq::Error) -> HttpError {
    if crate::http_client::connect_error(&error).is_some() {
        return HttpError::Connect(crate::http_client::transport_error_message(&error));
    }
    match &error {
        ureq::Error::Timeout(_) => HttpError::Timeout(error.to_string()),
        ureq::Error::HostNotFound => HttpError::Connect(error.to_string()),
        ureq::Error::TooManyRedirects => HttpError::TooManyRedirects,
        ureq::Error::Other(inner) if toolport_plus::egress::is_refusal(&inner.to_string()) => {
            HttpError::EgressRefused(inner.to_string())
        }
        _ => HttpError::Other(crate::http_client::transport_error_message(&error)),
    }
}

fn read_registry_copy() -> Result<crate::registry::Registry, String> {
    let Some(path) = crate::registry::resolved_path() else {
        return Ok(crate::registry::Registry::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("registry is not readable: {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(crate::registry::Registry::default())
        }
        Err(error) => Err(format!("cannot read registry: {error}")),
    }
}

/// The environment of an `auth` run: what upstream hands a server process, built from the same
/// allowlist, with the server's own entries last.
fn auth_environment(server: &crate::registry::ServerEntry) -> Vec<(String, String)> {
    let own: Vec<(String, String)> = server
        .env
        .iter()
        .filter_map(|var| {
            let value = if var.secret {
                crate::secrets::get_secret(&server.id, &var.key)
            } else {
                var.value.clone()
            };
            value.map(|value| (var.key.clone(), value))
        })
        .collect();
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(name, _)| {
            if crate::downstream::is_control_env_name(name) {
                return false;
            }
            server.inherit_env || crate::downstream::CHILD_ENV_ALLOWLIST.contains(&name.as_str())
        })
        .collect();
    env.retain(|(name, _)| !own.iter().any(|(key, _)| key == name));
    env.extend(own);
    env
}

struct AuthProcess {
    child: Option<std::process::Child>,
    stderr: Option<std::process::ChildStderr>,
    server: crate::registry::ServerEntry,
    env: Vec<(String, String)>,
    resolved: crate::launch_inputs::ResolvedArgs,
}

impl AuthProcess {
    fn secret_values(&self) -> impl Iterator<Item = &str> {
        self.server
            .env
            .iter()
            .filter(|var| var.secret)
            .filter_map(|var| {
                self.env
                    .iter()
                    .find(|(key, _)| key == &var.key)
                    .map(|(_, value)| value.as_str())
            })
            .filter(|value| !value.is_empty())
    }
}

impl toolport_plus_api::AuthChild for AuthProcess {
    fn take_stderr(&mut self) -> Option<Box<dyn std::io::Read + Send>> {
        self.stderr
            .take()
            .map(|stderr| Box::new(stderr) as Box<dyn std::io::Read + Send>)
    }

    fn try_exit(&mut self) -> Option<i32> {
        let status = self.child.as_mut()?.try_wait().ok().flatten()?;
        Some(status.code().unwrap_or(-1))
    }

    fn kill(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        // The child leads its own process group, so this reaches `npx` to `node` chains. An
        // unreaped child still owns its pid, which keeps the group id from being reused.
        #[cfg(unix)]
        if child.try_wait().ok().flatten().is_none() {
            // SAFETY: signals the process group of a child this value spawned and has not reaped.
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    fn detach(mut self: Box<Self>) {
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    fn scrub(&self, line: String) -> String {
        let line = self.resolved.redact(line);
        self.secret_values()
            .fold(line, |text, value| text.replace(value, "***"))
    }

    fn scrub_url(&self, url: String) -> Option<String> {
        let client_id = url::Url::parse(&url).ok().and_then(|parsed| {
            parsed
                .query_pairs()
                .find(|(key, _)| key == "client_id")
                .map(|(_, value)| value.into_owned())
        });
        let url = self
            .secret_values()
            .filter(|value| client_id.as_deref() != Some(*value))
            .fold(url, |text, value| text.replace(value, "***"));
        let bound = self
            .resolved
            .args
            .iter()
            .zip(&self.server.args)
            .filter(|(resolved, configured)| resolved != configured)
            .map(|(resolved, _)| resolved.as_str());
        let leaks = bound
            .into_iter()
            .any(|value| !value.is_empty() && url.contains(value));
        (!leaks).then_some(url)
    }

    fn hides_output(&self) -> bool {
        self.server.launch.is_some()
    }
}

impl Drop for AuthProcess {
    fn drop(&mut self) {
        toolport_plus_api::AuthChild::kill(self);
    }
}

fn launch_error(
    fault: toolport_plus_api::LaunchFault,
    message: impl Into<String>,
) -> toolport_plus_api::LaunchError {
    toolport_plus_api::LaunchError::new(fault, message)
}

impl toolport_plus_api::AuthAccess for HostAdapter {
    fn start_auth(
        &self,
        server_id: &str,
    ) -> Result<Box<dyn toolport_plus_api::AuthChild>, toolport_plus_api::LaunchError> {
        use toolport_plus_api::LaunchFault::{Refused, Spawn, Unsupported};
        let registry = read_registry_copy().map_err(|m| launch_error(Spawn, m))?;
        let server = registry
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .cloned()
            .ok_or_else(|| launch_error(Unsupported, "unknown server"))?;
        let command = match (&server.command, server.transport.as_str()) {
            (Some(command), "stdio") => command.clone(),
            _ => {
                return Err(launch_error(
                    Unsupported,
                    "the auth flow only applies to stdio servers",
                ))
            }
        };
        let resolved = crate::launch_inputs::resolve_args(&server)
            .map_err(|message| launch_error(Refused, message))?;
        let env = auth_environment(&server);
        let (command, mut args) = crate::downstream::normalize_invocation(&command, &resolved.args);
        args.push("auth".to_string());
        crate::downstream::screen_spawn_command(&command, &args)
            .and_then(|()| crate::downstream::screen_spawn_env(&env))
            .map_err(|message| launch_error(Refused, resolved.redact(message)))?;
        let mut cmd = std::process::Command::new(crate::downstream::resolve_command(&command));
        crate::hostenv::strip_bundled_env(&mut cmd);
        cmd.env_clear().args(&args).envs(env.iter().cloned());
        #[cfg(not(windows))]
        if !server.inherit_env {
            cmd.env("PATH", crate::downstream::augmented_path());
        }
        if let Some(dir) = server
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            let dir = std::path::Path::new(dir);
            if !dir.is_dir() {
                return Err(launch_error(
                    Refused,
                    format!("working directory {} does not exist", dir.display()),
                ));
            }
            cmd.current_dir(dir);
        }
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().map_err(|error| {
            launch_error(Spawn, resolved.redact(format!("could not start: {error}")))
        })?;
        let stderr = child.stderr.take();
        Ok(Box::new(AuthProcess {
            child: Some(child),
            stderr,
            server,
            env,
            resolved,
        }))
    }

    fn awaits_review(&self, server_id: &str) -> Result<bool, String> {
        let registry = read_registry_copy()?;
        Ok(registry
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .is_some_and(|server| {
                server.needs_team_enable_review()
                    && !registry.is_enabled(&registry.active_profile_id(), server_id)
            }))
    }

    fn sign_in_remote(
        &self,
        server_id: &str,
        on_consent: Option<toolport_plus_api::ConsentSink>,
    ) -> Result<(), String> {
        let registry = read_registry_copy()?;
        let url = registry
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .and_then(|server| server.url.clone())
            .ok_or_else(|| "the server has no address to sign in to".to_string())?;
        let attempt = crate::oauth_controller::start_attempt();
        let sign_in = || crate::oauth_controller::authenticate(server_id, &url, &attempt);
        match on_consent {
            Some(sink) => {
                toolport_plus::oauth::consent::with_observer(move |url| sink(url), sign_in)
            }
            None => sign_in(),
        }
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        crate::oauth::open_web_url(url)
    }
}

fn is_launch_secret(server: &str, key: &str) -> bool {
    crate::registry::load().ok().is_some_and(|registry| {
        registry
            .servers
            .iter()
            .find(|entry| entry.id == server)
            .and_then(|entry| entry.launch.as_ref())
            .is_some_and(|launch| launch.inputs.iter().any(|i| i.key == key && i.secret))
    })
}

/// Runs an inline script whose `toolport.call`s may only reach `servers`; the first call to
/// another server fails the step with a message, whatever the script does with the error.
fn run_guarded_script(source: &str, args: Value, servers: &[String]) -> Result<Value, String> {
    use std::sync::{Arc, Mutex};
    let allowed = servers.to_vec();
    let denied: Arc<Mutex<Option<String>>> = Arc::default();
    let note = Arc::clone(&denied);
    let call: crate::codemode::CallBinding = Arc::new(move |name: &str, args: Value| {
        let Some((server, tool)) = crate::codemode::split_exposed_name(name) else {
            return serde_json::json!({"error": format!("{name:?} is not server__tool")});
        };
        {
            let mut slot = note
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if slot.is_none() && !allowed.iter().any(|s| s == server) {
                let list = if allowed.is_empty() {
                    "none".to_string()
                } else {
                    allowed.join(", ")
                };
                *slot = Some(format!(
                    "the script called the server {server:?}, which is not in requires.servers ({list}); the call was not made"
                ));
            }
            if let Some(why) = slot.as_ref() {
                return serde_json::json!({"isError": true, "error": why});
            }
        }
        crate::playground::call_tool(server, tool, args)
            .unwrap_or_else(|e| serde_json::json!({"isError": true, "error": e}))
    });
    let outcome = crate::codemode::run_script(
        source,
        args,
        call,
        None,
        crate::codemode::Limits::default(),
        &[],
    );
    let refused = denied
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(why) = refused {
        return Err(why);
    }
    match outcome.error {
        Some(error) => Err(error),
        None => Ok(outcome.value),
    }
}

impl TaskHost for HostAdapter {
    fn unattended(&self, run: &mut dyn FnMut()) {
        crate::secrets::without_prompts(run);
    }

    fn redact_text(&self, text: &str) -> String {
        crate::secret_redact::redact_text(text).into_owned()
    }

    fn tasks_base(&self) -> Option<PathBuf> {
        crate::registry::conduit_dir()
    }

    fn now(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn task_call_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value, String> {
        crate::playground::call_tool(server, tool, args)
    }

    fn task_run_script(
        &self,
        source: &str,
        args: Value,
        servers: &[String],
    ) -> Result<Value, String> {
        run_guarded_script(source, args, servers)
    }

    fn task_set_secret(&self, server: &str, key: &str, value: &str) -> Result<(), String> {
        if is_launch_secret(server, key) {
            crate::registry_controller::set_launch_secret(server, key, value)
        } else {
            crate::registry_controller::set_server_secret(server, key, value)
        }
        .map(|_| ())
    }

    fn task_secret_is_set(&self, server: &str, key: &str) -> bool {
        crate::secrets::get_secret_result(server, key)
            .ok()
            .flatten()
            .is_some()
    }

    fn task_restart_server(&self, server: &str) -> Result<String, String> {
        use toolport_plus::servers::restart::{self, Status};
        let mut ask = restart::RestartArgs::new(server);
        ask.wait = std::time::Duration::from_secs(5);
        let done = restart::restart(self, &ask).map_err(|error| error.message)?;
        match done.status {
            Status::Failed => Err(format!("a gateway could not restart {server}")),
            other => Ok(format!("restart {server}: {}", other.as_str())),
        }
    }

    fn task_server_installed(&self, server: &str) -> bool {
        match self.read_view().and_then(|view| match view {
            Some(view) => view.servers().map(Some),
            None => Ok(None),
        }) {
            Ok(Some(servers)) => servers.iter().any(|s| s.id == server),
            _ => true,
        }
    }

    fn task_run_process(
        &self,
        spec: &ProcessSpec,
        cancelled: &dyn Fn() -> bool,
        started: &dyn Fn(u32),
    ) -> ProcessOutcome {
        toolport_plus::tasks::proc::run_process(spec, cancelled, started)
    }

    fn task_claude_program(&self) -> String {
        std::env::var("TOOLPORT_CLAUDE_BIN").unwrap_or_else(|_| "claude".into())
    }

    fn task_spawn_runner(&self, run_id: &str) -> Result<(), String> {
        toolport_plus::tasks::proc::spawn_runner(run_id)
    }

    fn task_process_alive(&self, pid: u32) -> bool {
        toolport_plus::tasks::proc::alive(pid)
    }

    fn task_kill_group(&self, pid: u32) {
        toolport_plus::tasks::proc::kill_group(pid);
    }

    fn task_approve_run(&self, ask: &RunApproval) -> RunDecision {
        use crate::approval::{new_correlation_id, ApprovalDecision, ApprovalReason};
        let request = crate::approval::ApprovalRequest {
            token: String::new(),
            id: new_correlation_id(),
            client: Some("self-mcp".into()),
            client_name: None,
            client_label: None,
            host: None,
            host_thread: None,
            server: "toolport".into(),
            tool: "tasks_run".into(),
            reason: ApprovalReason::Destructive,
            arguments: serde_json::json!({
                "task": ask.task,
                "steps": ask.steps,
                "writesSecrets": ask.writes_secrets,
                "requires": ask.requires,
            }),
            tool_fingerprint: None,
            url_elicitation: None,
            pii_release: None,
        };
        match crate::approval::request_human_decision(request) {
            ApprovalDecision::Approved => RunDecision::Approved,
            ApprovalDecision::Denied => RunDecision::Denied,
            ApprovalDecision::Timeout => RunDecision::Timeout,
            ApprovalDecision::Unreachable => RunDecision::Unreachable,
            ApprovalDecision::StaleState => RunDecision::Stale,
            _ => RunDecision::Denied,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{self, DataDirTestEnv, Registry, ServerEntry, REGISTRY_ENV_LOCK};
    use serde_json::json;

    const FIXTURE: &str = include_str!("../../plus/fixtures/server_entry.json");
    const REGISTRY_V1: &str = include_str!("../../plus/fixtures/registry_v1_sanitized.json");

    fn fixture() -> (serde_json::Value, ServerEntry) {
        let json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let entry = serde_json::from_value(json.clone()).unwrap();
        (json, entry)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unattended_work_does_not_read_a_cold_master_key() {
        let mut result = None;
        HostAdapter.unattended(&mut || result = Some(HostAdapter.read_master_key()));
        assert_eq!(
            result.unwrap(),
            Err(toolport_plus::login_keychain::UNATTENDED_MESSAGE.to_string())
        );
    }

    #[test]
    fn task_output_masks_a_registered_secret_in_its_encoded_forms() {
        use base64::Engine as _;
        let _lock = crate::registry::data_dir_test_lock();
        let dir = std::env::temp_dir().join(format!("tp-task-redact-{}", std::process::id()));
        let _data = crate::registry::DataDirOverride::set(&dir);
        let secret = "task pass&word \"Zq81xx-7";
        crate::secret_redact::register("task-redact-server", "TASK_KEY", secret);
        let forms = [
            secret.to_string(),
            base64::engine::general_purpose::STANDARD.encode(secret),
            serde_json::to_string(secret).unwrap(),
            urlencoding::encode(secret).into_owned(),
        ];
        for form in forms {
            let shown = HostAdapter.redact_text(&format!("printed {form} done"));
            assert!(!shown.contains(&form), "{shown}");
            assert!(shown.contains("[secret:TASK_KEY]"), "{shown}");
        }
    }

    struct RestoreEnv(Option<std::ffi::OsString>);
    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("TOOLPORT_REGISTRY", value),
                None => std::env::remove_var("TOOLPORT_REGISTRY"),
            }
        }
    }

    /// A scratch data dir whose registry resolves there, not through an env override.
    struct Scratch {
        // Drop order is declaration order: the data dir lock goes last.
        _restore: RestoreEnv,
        _guard: std::sync::MutexGuard<'static, ()>,
        data_dir: DataDirTestEnv,
    }

    impl Scratch {
        fn new(label: &str) -> Self {
            let data_dir = DataDirTestEnv::new(label);
            let guard = REGISTRY_ENV_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let restore = RestoreEnv(std::env::var_os("TOOLPORT_REGISTRY"));
            std::env::remove_var("TOOLPORT_REGISTRY");
            Self {
                _restore: restore,
                _guard: guard,
                data_dir,
            }
        }

        fn write_registry(&self, text: &str) {
            std::fs::write(self.data_dir.dir.join("registry.json"), text).unwrap();
        }

        fn raw_registry(&self) -> serde_json::Value {
            let text = std::fs::read_to_string(self.data_dir.dir.join("registry.json")).unwrap();
            serde_json::from_str(&text).unwrap()
        }
    }

    #[test]
    fn mirror_round_trip_keeps_unknown_upstream_fields_and_the_fork_keys() {
        let (json, entry) = fixture();
        for key in ForkServerFields::KEYS {
            assert!(
                entry.unknown_fields.contains_key(key),
                "upstream now types `{key}`; the fork mirror must follow"
            );
        }
        assert!(entry.unknown_fields.contains_key("fieldFromNewerUpstream"));

        let mut record: ServerRecord = convert(&entry).unwrap();
        let probe = record
            .fork
            .plus
            .as_ref()
            .unwrap()
            .auth_probe
            .as_ref()
            .unwrap();
        assert_eq!(probe.kind.as_deref(), Some("slack"));
        assert_eq!(record.fork.forward_instructions, Some(true));
        assert_eq!(record.extra["hidePrompts"], json!(true));

        let unchanged: ServerEntry = convert(&record).unwrap();
        assert_eq!(unchanged, entry);

        record.enabled = true;
        let toggled: ServerEntry = convert(&record).unwrap();
        let mut expected = entry;
        expected.enabled = true;
        assert_eq!(toggled, expected);
        assert_eq!(
            serde_json::to_value(&toggled).unwrap()["plus"],
            json["plus"]
        );
    }

    #[test]
    fn set_server_enabled_keeps_what_the_mirror_does_not_know() {
        let _scratch = Scratch::new("plus-adapter");

        let (json, entry) = fixture();
        let mut registry = Registry::default();
        registry.servers.push(entry);
        registry::save(&registry).unwrap();

        HostAdapter
            .set_server_enabled("fixture-files", true)
            .unwrap();

        let stored = registry::load_resolved().unwrap();
        let server = &stored.servers[0];
        assert!(server.enabled);
        assert_eq!(server.unknown_fields["plus"], json["plus"]);
        assert_eq!(
            server.unknown_fields["fieldFromNewerUpstream"],
            json["fieldFromNewerUpstream"]
        );
        assert!(server.hide_prompts);

        let error = HostAdapter.set_server_enabled("missing", true).unwrap_err();
        assert!(error.contains("unknown server missing"));
    }

    fn fork_values(server: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        ForkServerFields::KEYS
            .iter()
            .filter_map(|key| server.get(*key).map(|value| ((*key).into(), value.clone())))
            .collect()
    }

    /// Fork data and profiles in `after` equal those in the v1 fixture `before`.
    fn assert_fork_data_and_profiles_kept(before: &serde_json::Value, after: &serde_json::Value) {
        assert_eq!(after["plus"], before["plus"], "top-level plus");
        assert_eq!(after["profiles"], before["profiles"], "profiles");
        let servers_before = before["servers"].as_array().unwrap();
        let servers_after = after["servers"].as_array().unwrap();
        assert_eq!(servers_after.len(), servers_before.len());
        for (kept, original) in servers_after.iter().zip(servers_before) {
            assert_eq!(kept["id"], original["id"]);
            assert_eq!(
                fork_values(kept),
                fork_values(original),
                "fork keys of {}",
                original["id"]
            );
        }
    }

    #[test]
    fn v1_registry_loads_as_v3_and_keeps_plus_fork_server_fields_and_profiles() {
        let scratch = Scratch::new("plus-registry-v1");
        let before: serde_json::Value = serde_json::from_str(REGISTRY_V1).unwrap();
        assert_eq!(before["profiles"].as_array().unwrap().len(), 8);
        let covered: std::collections::BTreeSet<&str> = before["servers"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|server| server.as_object().unwrap().keys().map(String::as_str))
            .collect();
        for key in ForkServerFields::KEYS {
            assert!(covered.contains(key), "fixture has no server with `{key}`");
        }
        scratch.write_registry(REGISTRY_V1);

        let loaded = registry::load_resolved().unwrap();
        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.profiles.len(), 8);
        for server in &loaded.servers {
            for key in ForkServerFields::KEYS {
                let original = before["servers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|candidate| candidate["id"] == server.id.as_str())
                    .unwrap();
                assert_eq!(
                    server.unknown_fields.contains_key(key),
                    original.get(key).is_some(),
                    "`{key}` of {} must stay in unknown fields",
                    server.id
                );
            }
        }
        assert_fork_data_and_profiles_kept(&before, &scratch.raw_registry());

        // A full load and save, as every upstream writer does.
        registry::update(|_| Ok(())).unwrap();
        assert_fork_data_and_profiles_kept(&before, &scratch.raw_registry());

        let servers = HostAdapter.list_servers().unwrap();
        assert_eq!(servers.len(), 8);
        let plus = HostAdapter.plus_data().unwrap();
        assert_eq!(
            plus.selfmcp.unwrap().enabled_in.unwrap(),
            vec!["default", "client-a", "client-b"]
        );
        assert_eq!(plus.extra["futureSection"], before["plus"]["futureSection"]);
        let direct = &plus.direct_entries.unwrap()["client-a"]["Synthetic Entry"];
        assert_eq!(direct.url.as_deref(), Some("https://example.test/direct"));
        let untyped = servers.iter().find(|s| s.id == "srv-untyped").unwrap();
        assert!(untyped.fork.mcpm_source.as_ref().unwrap().kind.is_none());
        assert_eq!(untyped.fork.mcpm_oauth, Some(false));
    }

    #[test]
    fn fork_ops_change_their_key_and_keep_every_other_fork_field() {
        let scratch = Scratch::new("plus-registry-ops");
        let before: serde_json::Value = serde_json::from_str(REGISTRY_V1).unwrap();
        scratch.write_registry(REGISTRY_V1);
        let adapter = HostAdapter;

        adapter
            .set_server_mcpm_tags("srv-git", Some(vec!["only".into()]))
            .unwrap();
        adapter
            .set_server_forward_instructions("srv-git", false)
            .unwrap();
        adapter
            .set_server_max_request_timeout_ms("srv-git", None)
            .unwrap();
        adapter.set_server_mode("srv-git", None).unwrap();
        adapter.set_server_auth_probe("srv-untyped", None).unwrap();
        adapter
            .merge_server_mcpm_source(
                "srv-git",
                json!({"branch": "next", "last_checked": "later"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            )
            .unwrap();

        let after = scratch.raw_registry();
        let git = |value: &serde_json::Value| value["servers"][0].clone();
        let mut expected = git(&before);
        expected["mcpmTags"] = json!(["only"]);
        expected
            .as_object_mut()
            .unwrap()
            .remove("forwardInstructions");
        expected
            .as_object_mut()
            .unwrap()
            .remove("maxRequestTimeoutMs");
        expected.as_object_mut().unwrap().remove("serverMode");
        expected["mcpmSource"]["branch"] = json!("next");
        expected["mcpmSource"]["last_checked"] = json!("later");
        let got = git(&after);
        for key in ForkServerFields::KEYS {
            assert_eq!(got.get(key), expected.get(key), "{key}");
        }
        assert_eq!(got["declareClientCapabilities"], json!(true));
        assert_eq!(got["requestTimeoutMs"], json!(45000));

        let untyped = &after["servers"][4];
        assert_eq!(untyped["plus"], json!({"futurePlusField": 1}));
        assert_eq!(after["profiles"], before["profiles"]);
        assert_eq!(after["plus"], before["plus"]);

        let error = adapter
            .set_server_mcpm_oauth("missing", Some(true))
            .unwrap_err();
        assert!(error.contains("unknown server missing"), "{error}");
    }

    #[test]
    fn plus_ops_write_through_the_registry_and_prune_empty_containers() {
        let scratch = Scratch::new("plus-registry-plus-ops");
        let before: serde_json::Value = serde_json::from_str(REGISTRY_V1).unwrap();
        scratch.write_registry(REGISTRY_V1);
        let adapter = HostAdapter;

        adapter
            .add_selfmcp_enabled_in(&["default".into(), "client-z".into()])
            .unwrap();
        adapter.set_selfmcp_opt_out(true).unwrap();
        let plus = adapter.plus_data().unwrap();
        let record = plus.selfmcp.unwrap();
        assert_eq!(
            record.enabled_in.unwrap(),
            vec!["default", "client-a", "client-b", "client-z"]
        );
        assert_eq!(record.opt_out, Some(true));
        assert_eq!(record.extra["futureSelfField"], json!(true));

        adapter
            .set_direct_entry(
                "client-b",
                "Other",
                DirectEntry {
                    server: "s".into(),
                    command: "c".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(adapter.clear_direct_entry("client-b", "Other").unwrap());
        assert!(!adapter.clear_direct_entry("client-b", "Other").unwrap());
        assert!(adapter
            .clear_direct_entry("client-a", "Synthetic Entry")
            .unwrap());
        adapter.set_selfmcp_record(None).unwrap();

        let after = scratch.raw_registry();
        assert_eq!(
            after["plus"],
            json!({"futureSection": before["plus"]["futureSection"]})
        );
        assert_eq!(after["profiles"], before["profiles"]);
    }

    /// Reads a copy of the real v1 registry named by `TOOLPORT_V1_COPY`, never the live file.
    #[test]
    #[ignore = "needs TOOLPORT_V1_COPY, the path of a copy of a v1 registry.json"]
    fn copy_of_the_live_v1_registry_keeps_plus_fork_server_fields_and_profiles() {
        let copy = std::env::var("TOOLPORT_V1_COPY").expect("TOOLPORT_V1_COPY");
        let text = std::fs::read_to_string(copy).unwrap();
        let before: serde_json::Value = serde_json::from_str(&text).unwrap();
        let scratch = Scratch::new("plus-registry-live-copy");
        scratch.write_registry(&text);

        let loaded = registry::load_resolved().unwrap();
        assert_eq!(loaded.version, 3);
        registry::update(|_| Ok(())).unwrap();
        assert_fork_data_and_profiles_kept(&before, &scratch.raw_registry());
        let servers = HostAdapter.list_servers().unwrap();
        assert_eq!(servers.len(), before["servers"].as_array().unwrap().len());
        HostAdapter.plus_data().unwrap();

        let mut fork_keys_kept = 0;
        for server in before["servers"].as_array().unwrap() {
            fork_keys_kept += fork_values(server).len();
        }
        println!(
            "live copy: servers={} profiles={} fork_server_keys={} plus_keys={} version={}",
            loaded.servers.len(),
            loaded.profiles.len(),
            fork_keys_kept,
            before["plus"].as_object().map_or(0, |plus| plus.len()),
            scratch.raw_registry()["version"]
        );
    }

    mod vault {
        use super::*;
        use toolport_plus_api::HTTP_AUTH_KEY;

        const VALUE: &str = "FAKE-vaulted-value-1c9e";

        fn with_registry(test: impl FnOnce(&std::path::Path)) {
            crate::secrets::tests::with_isolated_vault(|| {
                let dir = crate::registry::conduit_dir().unwrap();
                std::fs::write(
                    dir.join("registry.json"),
                    json!({
                        "version": 1,
                        "servers": [
                            {"id": "srv-alpha", "name": "alpha", "transport": "stdio",
                             "command": "alpha-mcp", "args": ["<launch-input>"],
                             "launch": {"inputs": [{"key": "LAUNCH_TOKEN", "label": "Token", "secret": true}],
                                        "bindings": [{"index": 0, "parts": [{"kind": "input", "key": "LAUNCH_TOKEN"}]}]}},
                            {"id": "srv-beta", "name": "beta", "transport": "http",
                             "url": "https://beta.example.invalid/mcp"},
                        ],
                        "profiles": [{"id": "default", "name": "Default",
                                      "enabledServerIds": ["srv-alpha"]}],
                        "activeProfileId": "default",
                    })
                    .to_string(),
                )
                .unwrap();
                test(&dir.join("registry.json"));
            });
        }

        fn env_of_alpha(registry: &std::path::Path) -> serde_json::Value {
            let text = std::fs::read_to_string(registry).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert!(!text.contains(VALUE));
            match &value["servers"][0]["env"] {
                serde_json::Value::Null => json!([]),
                env => env.clone(),
            }
        }

        #[test]
        fn the_access_token_key_is_the_one_upstream_uses() {
            assert_eq!(HTTP_AUTH_KEY, crate::secrets::HTTP_AUTH_KEY);
        }

        #[test]
        fn an_env_secret_lands_in_the_vault_and_is_declared_without_its_value() {
            with_registry(|registry| {
                HostAdapter
                    .store_env_secret("srv-alpha", "API_TOKEN", VALUE)
                    .unwrap();
                assert_eq!(
                    HostAdapter
                        .read_vault_secret("srv-alpha", "API_TOKEN")
                        .unwrap()
                        .as_deref(),
                    Some(VALUE)
                );
                assert_eq!(
                    env_of_alpha(registry),
                    json!([{"key": "API_TOKEN", "secret": true}])
                );
                HostAdapter
                    .remove_env_secret("srv-alpha", "API_TOKEN")
                    .unwrap();
                assert_eq!(
                    HostAdapter
                        .read_vault_secret("srv-alpha", "API_TOKEN")
                        .unwrap(),
                    None
                );
                assert_eq!(env_of_alpha(registry), json!([]));
            });
        }

        #[test]
        fn a_placeholder_is_refused_and_nothing_is_stored() {
            with_registry(|registry| {
                for placeholder in ["${API_TOKEN}", "<your-key>", "YOUR_API_KEY"] {
                    let error = HostAdapter
                        .store_env_secret("srv-alpha", "API_TOKEN", placeholder)
                        .unwrap_err();
                    assert!(
                        error.contains("Placeholder values cannot be saved"),
                        "{error}"
                    );
                }
                assert_eq!(
                    HostAdapter
                        .read_vault_secret("srv-alpha", "API_TOKEN")
                        .unwrap(),
                    None
                );
                assert_eq!(env_of_alpha(registry), json!([]));
            });
        }

        #[test]
        fn a_launch_secret_is_vaulted_without_an_env_declaration() {
            with_registry(|registry| {
                HostAdapter
                    .store_launch_secret("srv-alpha", "LAUNCH_TOKEN", VALUE)
                    .unwrap();
                assert_eq!(
                    HostAdapter
                        .read_vault_secret("srv-alpha", "LAUNCH_TOKEN")
                        .unwrap()
                        .as_deref(),
                    Some(VALUE)
                );
                assert_eq!(env_of_alpha(registry), json!([]));
                let error = HostAdapter
                    .store_launch_secret("srv-alpha", "NOT_DECLARED", VALUE)
                    .unwrap_err();
                assert!(
                    error.contains("not a declared secret launch input"),
                    "{error}"
                );
            });
        }

        #[test]
        fn the_access_token_of_a_remote_server_is_stored_and_cleared() {
            with_registry(|_| {
                HostAdapter.store_access_token("srv-beta", VALUE).unwrap();
                assert_eq!(
                    HostAdapter
                        .read_vault_secret("srv-beta", HTTP_AUTH_KEY)
                        .unwrap()
                        .as_deref(),
                    Some(VALUE)
                );
                HostAdapter.clear_access_token("srv-beta").unwrap();
                assert_eq!(
                    HostAdapter
                        .read_vault_secret("srv-beta", HTTP_AUTH_KEY)
                        .unwrap(),
                    None
                );
            });
        }
    }

    mod auth {
        use super::*;
        use toolport_plus::auth::{AuthState, Prober, Selector};
        use toolport_plus_api::AuthAccess;

        const LAUNCH_VALUE: &str = "FAKE-launch-value-5d2c";

        /// A stdio server whose `auth` command prints its vault value and a consent link, then waits.
        fn write_server(dir: &std::path::Path, script_body: &str, probe: bool) -> String {
            let script = dir.join("fake-auth-mcp");
            std::fs::write(&script, format!("#!/bin/sh\n{script_body}\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let mut server = json!({
                "id": "srv-auth", "name": "auth-mcp", "enabled": true, "transport": "stdio",
                "command": script.to_string_lossy(), "args": [],
                "env": [{"key": "LAUNCH_TOKEN", "secret": true}],
            });
            if probe {
                server["plus"] = json!({"authProbe": {"kind": "stdio"}});
            }
            std::fs::write(
                dir.join("registry.json"),
                json!({"version": 1, "servers": [server], "profiles": [], "activeProfileId": "default"})
                    .to_string(),
            )
            .unwrap();
            "srv-auth".to_string()
        }

        #[test]
        fn a_stdio_probe_runs_the_real_auth_command_and_keeps_vault_values_out_of_the_status_file()
        {
            crate::secrets::tests::with_isolated_vault(|| {
                let dir = crate::registry::conduit_dir().unwrap();
                let id = write_server(
                    &dir,
                    "[ \"$1\" = auth ] || exit 2\necho \"token $LAUNCH_TOKEN\" >&2\necho \"open https://accounts.example.invalid/consent?client_id=pub\" >&2\nsleep 30",
                    true,
                );
                crate::secrets::set_secret(&id, "LAUNCH_TOKEN", LAUNCH_VALUE).unwrap();
                let prober = Prober::new(&HostAdapter).unwrap();
                let run = prober.run(&Selector::One(id.clone()), true).unwrap();
                assert_eq!(run.reports[0].tracked.state, AuthState::NeedsReauth);
                assert_eq!(run.reports[0].tracked.reason, "not_authed");
                let status = std::fs::read_to_string(dir.join("auth/status.json")).unwrap();
                assert!(!status.contains(LAUNCH_VALUE));
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(dir.join("auth/status.json"))
                        .unwrap()
                        .permissions()
                        .mode();
                    assert_eq!(mode & 0o777, 0o600);
                }
            });
        }

        #[test]
        fn the_sign_in_tool_returns_the_link_and_scrubs_the_vault_value_from_the_tail() {
            crate::secrets::tests::with_isolated_vault(|| {
                let dir = crate::registry::conduit_dir().unwrap();
                let id = write_server(
                    &dir,
                    "echo \"token $LAUNCH_TOKEN\" >&2\necho \"open https://accounts.example.invalid/consent?client_id=pub\" >&2\nsleep 1",
                    false,
                );
                crate::secrets::set_secret(&id, "LAUNCH_TOKEN", LAUNCH_VALUE).unwrap();
                let out = toolport_plus::auth::start_sign_in(&HostAdapter, &id).unwrap();
                assert_eq!(
                    out["authUrl"],
                    "https://accounts.example.invalid/consent?client_id=pub"
                );
                assert!(!out.to_string().contains(LAUNCH_VALUE));
            });
        }

        #[test]
        fn a_team_server_awaiting_review_is_not_started() {
            let scratch = Scratch::new("auth-review");
            let id = write_server(&scratch.data_dir.dir, "exit 0", true);
            let mut registry = scratch.raw_registry();
            registry["servers"][0]["source"] = json!("team:acme");
            registry["servers"][0]["teamEnableReview"] = json!(true);
            scratch.write_registry(&registry.to_string());
            assert!(HostAdapter.awaits_review(&id).unwrap());
            let prober = Prober::new(&HostAdapter).unwrap();
            assert!(!prober.has_probe(&id));
        }
    }

    mod http {
        use super::*;
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::Duration;
        use toolport_plus_api::HttpMethod;

        /// Answers a fixed set of paths on 127.0.0.1 until dropped.
        struct Loopback {
            port: u16,
            stop: Arc<AtomicBool>,
        }

        impl Loopback {
            fn start() -> Self {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                listener.set_nonblocking(true).unwrap();
                let port = listener.local_addr().unwrap().port();
                let stop = Arc::new(AtomicBool::new(false));
                let flag = Arc::clone(&stop);
                std::thread::spawn(move || {
                    while !flag.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                std::thread::spawn(move || answer(stream));
                            }
                            Err(_) => std::thread::sleep(Duration::from_millis(5)),
                        }
                    }
                });
                Self { port, stop }
            }

            fn url(&self, path: &str) -> String {
                format!("http://127.0.0.1:{}{path}", self.port)
            }

            /// The same listener under another host name, which counts as another host.
            fn other_host_url(&self, path: &str) -> String {
                format!("http://localhost:{}{path}", self.port)
            }
        }

        impl Drop for Loopback {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::Relaxed);
            }
        }

        fn answer(mut stream: TcpStream) {
            stream.set_nonblocking(false).unwrap();
            let mut received = Vec::new();
            let mut chunk = [0u8; 1024];
            let header_end = loop {
                let read = stream.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    return;
                }
                received.extend_from_slice(&chunk[..read]);
                if let Some(at) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                    break at + 4;
                }
            };
            let head = String::from_utf8_lossy(&received[..header_end]).into_owned();
            let length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            while received.len() < header_end + length {
                let read = stream.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    break;
                }
                received.extend_from_slice(&chunk[..read]);
            }
            let request_line = head.lines().next().unwrap_or_default().to_string();
            let mut parts = request_line.split(' ');
            let method = parts.next().unwrap_or_default();
            let path = parts.next().unwrap_or_default();
            let content_type = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-type")
                        .then(|| value.trim().to_string())
                })
                .unwrap_or_default();
            let sent_body = String::from_utf8_lossy(&received[header_end..]).into_owned();

            let (status, extra, payload) = match path {
                "/ok" => ("200 OK", "", "fine".to_string()),
                "/json" => (
                    "200 OK",
                    "Content-Type: application/json\r\n",
                    r#"{"ok":true}"#.to_string(),
                ),
                "/unauthorized" => (
                    "401 Unauthorized",
                    "Content-Type: application/json\r\n",
                    r#"{"error":"invalid_token"}"#.to_string(),
                ),
                "/missing" => ("404 Not Found", "", "nothing here".to_string()),
                "/boom" => ("500 Internal Server Error", "", "broken".to_string()),
                "/redirect" => ("302 Found", "Location: /ok\r\n", String::new()),
                "/loop" => ("302 Found", "Location: /loop\r\n", String::new()),
                "/headers" => (
                    "200 OK",
                    "Content-Type: application/json\r\n",
                    serde_json::Value::Object(
                        head.lines()
                            .skip(1)
                            .filter_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                Some((
                                    name.to_ascii_lowercase(),
                                    serde_json::Value::String(value.trim().to_string()),
                                ))
                            })
                            .collect(),
                    )
                    .to_string(),
                ),
                p if p.starts_with("/redirect-to/") => {
                    let (code, target) = p["/redirect-to/".len()..].split_once('?').unwrap();
                    return respond_with(
                        &mut stream,
                        &format!("{code} Redirect"),
                        &format!("Location: {target}\r\n"),
                        "",
                    );
                }
                "/to-blocked" => (
                    "302 Found",
                    "Location: http://toolport.app/latest.json\r\n",
                    String::new(),
                ),
                "/big" => ("200 OK", "", "x".repeat(5000)),
                "/slow" => {
                    std::thread::sleep(Duration::from_secs(2));
                    ("200 OK", "", "late".to_string())
                }
                "/echo" => (
                    "200 OK",
                    "Content-Type: application/json\r\n",
                    serde_json::json!({
                        "method": method,
                        "contentType": content_type,
                        "body": sent_body,
                    })
                    .to_string(),
                ),
                _ => ("404 Not Found", "", String::new()),
            };
            respond_with(&mut stream, status, extra, &payload);
        }

        fn respond_with(stream: &mut TcpStream, status: &str, extra: &str, payload: &str) {
            let response = format!(
                "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }

        fn send(request: HttpRequest) -> Result<HttpResponse, HttpError> {
            HostAdapter.send(&request)
        }

        #[test]
        fn a_200_returns_body_headers_and_final_url() {
            let server = Loopback::start();
            let response = send(HttpRequest::get(server.url("/json"))).unwrap();
            assert_eq!(response.status, 200);
            assert!(response.is_success());
            assert_eq!(response.json(), Some(json!({"ok": true})));
            assert_eq!(response.header("content-type"), Some("application/json"));
            assert_eq!(response.url, server.url("/json"));
        }

        #[test]
        fn a_401_is_a_response_with_its_body_not_an_error() {
            let server = Loopback::start();
            let response = send(HttpRequest::get(server.url("/unauthorized"))).unwrap();
            assert_eq!(response.status, 401);
            assert!(!response.is_success());
            assert_eq!(response.json(), Some(json!({"error": "invalid_token"})));
        }

        #[test]
        fn a_404_and_a_500_are_responses() {
            let server = Loopback::start();
            let missing = send(HttpRequest::get(server.url("/missing"))).unwrap();
            assert_eq!(
                (missing.status, missing.text()),
                (404, "nothing here".into())
            );
            let broken = send(HttpRequest::get(server.url("/boom"))).unwrap();
            assert_eq!((broken.status, broken.text()), (500, "broken".into()));
        }

        #[test]
        fn a_redirect_is_followed_and_reports_where_it_ended() {
            let server = Loopback::start();
            let response = send(HttpRequest::get(server.url("/redirect"))).unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.text(), "fine");
            assert_eq!(response.url, server.url("/ok"));
        }

        #[test]
        fn a_redirect_is_returned_as_it_is_when_following_is_off() {
            let server = Loopback::start();
            let response = send(HttpRequest::get(server.url("/redirect")).no_redirects()).unwrap();
            assert_eq!(response.status, 302);
            assert_eq!(response.header("location"), Some("/ok"));
        }

        fn credentialed(url: String) -> HttpRequest {
            HttpRequest::get(url)
                .header("Accept", "application/json")
                .header("Authorization", "Bearer synthetic-token")
                .header("Cookie", "session=synthetic")
                .header("Proxy-Authorization", "Basic c3ludGhldGlj")
                .header("X-Api-Key", "synthetic-key")
        }

        const CREDENTIAL_HEADERS: [&str; 4] = [
            "authorization",
            "cookie",
            "proxy-authorization",
            "x-api-key",
        ];

        #[test]
        fn ureq_alone_forwards_credential_headers_other_than_authorization_and_cookie() {
            let first = Loopback::start();
            let second = Loopback::start();
            let target = second.other_host_url("/headers");
            let mut response = crate::http_client::agent()
                .get(first.url(&format!("/redirect-to/302?{target}")))
                .header("Authorization", "Bearer synthetic-token")
                .header("Cookie", "session=synthetic")
                .header("Proxy-Authorization", "Basic c3ludGhldGlj")
                .header("X-Api-Key", "synthetic-key")
                .call()
                .unwrap();
            let seen: Value = response.body_mut().read_json().unwrap();
            assert!(seen.get("authorization").is_none(), "{seen}");
            assert!(seen.get("cookie").is_none(), "{seen}");
            assert_eq!(seen["x-api-key"], "synthetic-key");
            assert_eq!(seen["proxy-authorization"], "Basic c3ludGhldGlj");
        }

        #[test]
        fn a_redirect_to_another_host_carries_no_credential_header() {
            let first = Loopback::start();
            let second = Loopback::start();
            let target = second.other_host_url("/headers");
            let response = send(credentialed(
                first.url(&format!("/redirect-to/302?{target}")),
            ))
            .unwrap();
            assert_eq!(response.url, target);
            let seen = response.json().unwrap();
            for name in CREDENTIAL_HEADERS {
                assert!(seen.get(name).is_none(), "{name} was forwarded: {seen}");
            }
            assert_eq!(seen["accept"], "application/json");
        }

        #[test]
        fn a_redirect_to_another_port_of_the_same_host_counts_as_another_origin() {
            let first = Loopback::start();
            let second = Loopback::start();
            let target = second.url("/headers");
            let seen = send(credentialed(
                first.url(&format!("/redirect-to/302?{target}")),
            ))
            .unwrap()
            .json()
            .unwrap();
            assert!(seen.get("x-api-key").is_none(), "{seen}");
        }

        #[test]
        fn a_redirect_inside_one_origin_keeps_the_headers_but_not_the_cookie() {
            let server = Loopback::start();
            let target = server.url("/headers");
            let seen = send(credentialed(
                server.url(&format!("/redirect-to/302?{target}")),
            ))
            .unwrap()
            .json()
            .unwrap();
            assert_eq!(seen["authorization"], "Bearer synthetic-token");
            assert_eq!(seen["x-api-key"], "synthetic-key");
            assert!(seen.get("cookie").is_none(), "{seen}");
        }

        #[test]
        fn a_302_after_a_post_becomes_a_get_without_body_or_content_type() {
            let server = Loopback::start();
            let target = server.url("/echo");
            let request = HttpRequest::post(
                server.url(&format!("/redirect-to/302?{target}")),
                HttpBody::Json(json!({"a": 1})),
            );
            let seen = send(request).unwrap().json().unwrap();
            assert_eq!(seen["method"], "GET");
            assert_eq!(seen["body"], "");
            assert_eq!(seen["contentType"], "");
        }

        #[test]
        fn a_307_after_a_post_is_returned_as_a_response_not_replayed() {
            let server = Loopback::start();
            let target = server.url("/echo");
            let request = HttpRequest::post(
                server.url(&format!("/redirect-to/307?{target}")),
                HttpBody::Json(json!({"a": 1})),
            );
            let response = send(request).unwrap();
            assert_eq!(response.status, 307);
            assert_eq!(response.header("location"), Some(target.as_str()));
        }

        #[test]
        fn a_redirect_loop_ends_in_too_many_redirects() {
            let server = Loopback::start();
            let error = send(HttpRequest::get(server.url("/loop"))).unwrap_err();
            assert_eq!(error, HttpError::TooManyRedirects);
        }

        #[test]
        fn a_refused_connection_is_a_connect_error() {
            let closed = {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                listener.local_addr().unwrap().port()
            };
            let error = send(HttpRequest::get(format!("http://127.0.0.1:{closed}/"))).unwrap_err();
            assert!(matches!(error, HttpError::Connect(_)), "{error:?}");
        }

        #[test]
        fn a_blocked_host_is_refused_before_any_connection() {
            for url in [
                "https://toolport.app/",
                "http://updates.toolport.app/latest.json",
            ] {
                let error = send(HttpRequest::get(url)).unwrap_err();
                assert!(
                    matches!(&error, HttpError::EgressRefused(message) if message.contains("egress to toolport.app")),
                    "{url}: {error:?}"
                );
            }
        }

        #[test]
        fn a_redirect_to_a_blocked_host_is_refused_too() {
            let server = Loopback::start();
            let error = send(HttpRequest::get(server.url("/to-blocked"))).unwrap_err();
            assert!(matches!(error, HttpError::EgressRefused(_)), "{error:?}");
        }

        #[test]
        fn json_and_form_bodies_go_out_with_their_content_type() {
            let server = Loopback::start();
            let sent = send(HttpRequest::post(
                server.url("/echo"),
                HttpBody::Json(json!({"a": 1})),
            ))
            .unwrap()
            .json()
            .unwrap();
            assert_eq!(sent["method"], "POST");
            assert_eq!(sent["contentType"], "application/json");
            assert_eq!(sent["body"], r#"{"a":1}"#);

            let sent = send(HttpRequest::post(
                server.url("/echo"),
                HttpBody::Form(vec![("grant_type".into(), "refresh token".into())]),
            ))
            .unwrap()
            .json()
            .unwrap();
            assert_eq!(sent["contentType"], "application/x-www-form-urlencoded");
            assert_eq!(sent["body"], "grant_type=refresh+token");

            let put = send(HttpRequest::new(HttpMethod::Put, server.url("/echo")))
                .unwrap()
                .json()
                .unwrap();
            assert_eq!(put["method"], "PUT");
        }

        #[test]
        fn a_body_past_the_cap_is_cut_and_flagged() {
            let server = Loopback::start();
            let response = send(HttpRequest::get(server.url("/big")).max_body_bytes(100)).unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.body.len(), 100);
            assert!(response.truncated);
            assert_eq!(response.json(), None);
        }

        #[test]
        fn a_slow_server_hits_the_request_timeout() {
            let server = Loopback::start();
            let error =
                send(HttpRequest::get(server.url("/slow")).timeout(Duration::from_millis(200)))
                    .unwrap_err();
            assert!(matches!(error, HttpError::Timeout(_)), "{error:?}");
        }
    }

    #[test]
    fn read_view_never_writes_and_reads_a_v1_registry_as_it_is_on_disk() {
        let scratch = Scratch::new("read-view-v1");
        assert!(HostAdapter.read_view().unwrap().is_none());
        scratch.write_registry(REGISTRY_V1);
        let path = scratch.data_dir.dir.join("registry.json");
        let before = std::fs::read(&path).unwrap();
        let view = HostAdapter.read_view().unwrap().unwrap();
        assert_eq!(view.version(), 1);
        assert_eq!(view.servers().unwrap().len(), 8);
        assert_eq!(view.access_sets().len(), 8);
        assert_eq!(view.default_access(), None);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn read_view_reports_a_registry_that_does_not_parse() {
        let scratch = Scratch::new("read-view-bad");
        scratch.write_registry("{ not json");
        let error = HostAdapter.read_view().err().unwrap();
        assert!(error.starts_with("registry is not readable:"), "{error}");
    }

    #[test]
    fn a_transaction_adds_a_server_and_lists_it_in_an_access_set_in_one_save() {
        let scratch = Scratch::new("transact-add");
        scratch.write_registry(
            &json!({
                "version": 3,
                "servers": [],
                "profiles": [{"id": "focus", "name": "Focus", "enabledServerIds": []}],
            })
            .to_string(),
        );
        HostAdapter
            .transact(&mut |tx| {
                let id = tx.add_stdio_server("Probe", "probe-mcp", "test:probe")?;
                tx.set_server_enabled(&id, true)?;
                tx.set_access_server("focus", &id, true)?;
                tx.set_selfmcp_record(Some(SelfMcpRecord {
                    enabled_in: Some(vec!["focus".into()]),
                    ..Default::default()
                }))
            })
            .unwrap();
        let saved = scratch.raw_registry();
        assert_eq!(saved["servers"][0]["id"], "probe");
        assert_eq!(saved["servers"][0]["enabled"], true);
        assert_eq!(saved["servers"][0]["source"], "test:probe");
        assert_eq!(saved["profiles"][0]["enabledServerIds"], json!(["probe"]));
        assert_eq!(saved["plus"]["selfmcp"]["enabledIn"], json!(["focus"]));
        let view = HostAdapter.read_view().unwrap().unwrap();
        assert!(view.servers().unwrap()[0].enabled);
        assert_eq!(view.default_access(), None);
    }

    #[test]
    fn a_failing_transaction_leaves_the_registry_file_as_it_was() {
        let scratch = Scratch::new("transact-fail");
        scratch.write_registry(&json!({"version": 3, "servers": [], "profiles": []}).to_string());
        let before = scratch.raw_registry();
        let result = HostAdapter.transact(&mut |tx| {
            tx.add_stdio_server("Probe", "probe-mcp", "test:probe")?;
            tx.set_access_server("missing", "probe", true)
        });
        assert!(result.is_err());
        assert_eq!(scratch.raw_registry(), before);
    }

    #[test]
    fn the_view_resolves_the_access_sets_clients_are_scoped_to_by_id_or_name() {
        let scratch = Scratch::new("client-sets");
        scratch.write_registry(
            &json!({
                "version": 3,
                "servers": [],
                "profiles": [
                    {"id": "a", "name": "Alpha", "enabledServerIds": []},
                    {"id": "b", "name": "Beta", "enabledServerIds": []},
                ],
                "defaultAccessProfileId": "a",
                "clientScopes": {"claude-code": "b", "cursor": "Alpha", "zed": "gone"},
            })
            .to_string(),
        );
        let view = HostAdapter.read_view().unwrap().unwrap();
        assert_eq!(
            view.client_access_sets(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(view.default_access().as_deref(), Some("a"));
    }

    #[test]
    fn the_seam_is_live_in_a_plus_build() {
        assert!(HostAdapter.seam_live());
    }

    #[test]
    fn server_operations_run_upstreams_own_add_edit_and_remove() {
        let scratch = Scratch::new("server-ops");
        scratch.write_registry(
            &json!({"version": 3, "servers": [], "profiles": [
            {"id": "focus", "name": "Focus", "enabledServerIds": []}]})
            .to_string(),
        );
        let spec = ServerSpec {
            name: " Gamma ".into(),
            transport: "stdio".into(),
            command: Some("gamma-mcp".into()),
            args: vec!["--x".into()],
            inherit_env: Some(true),
            ..Default::default()
        };
        let mut added = String::new();
        HostAdapter
            .transact(&mut |tx| {
                added = tx.add_server(&spec)?;
                tx.set_server_env(
                    &added,
                    vec![EnvEntry {
                        key: "MODE".into(),
                        value: Some("fast".into()),
                        ..Default::default()
                    }],
                )?;
                tx.set_forward_instructions(&added, true)?;
                tx.set_access_server("focus", &added, true)
            })
            .unwrap();
        let stored = scratch.raw_registry();
        let server = &stored["servers"][0];
        assert_eq!(server["name"], "Gamma", "upstream's add trims the name");
        assert_eq!(
            server["enabled"], true,
            "upstream's add switches a server on when it needs no credential"
        );
        assert_eq!(server["inheritEnv"], true);
        assert_eq!(
            server["env"][0],
            json!({"key": "MODE", "value": "fast", "secret": false})
        );
        assert_eq!(server["forwardInstructions"], true);
        assert_eq!(stored["profiles"][0]["enabledServerIds"], json!([added]));

        let edit = ServerSpec {
            args: vec!["--y".into()],
            inherit_env: None,
            ..spec.clone()
        };
        HostAdapter
            .transact(&mut |tx| {
                tx.update_server(&added, &edit)?;
                tx.set_forward_instructions(&added, false)
            })
            .unwrap();
        let stored = scratch.raw_registry();
        assert_eq!(stored["servers"][0]["args"], json!(["--y"]));
        assert_eq!(
            stored["servers"][0]["inheritEnv"], true,
            "None keeps the saved choice"
        );
        assert!(stored["servers"][0].get("forwardInstructions").is_none());

        let rejected = HostAdapter
            .transact(&mut |tx| {
                tx.update_server(
                    &added,
                    &ServerSpec {
                        command: None,
                        ..edit.clone()
                    },
                )
            })
            .unwrap_err();
        assert_eq!(rejected, "enter the command to run");
        HostAdapter
            .transact(&mut |tx| tx.remove_server(&added))
            .unwrap();
        let stored = scratch.raw_registry();
        assert_eq!(stored["servers"], json!([]));
        assert_eq!(stored["profiles"][0]["enabledServerIds"], json!([]));
    }

    #[test]
    fn a_curated_catalog_entry_is_added_the_way_upstreams_catalog_add_does_it() {
        let scratch = Scratch::new("catalog-add");
        scratch.write_registry(&json!({"version": 3, "servers": [], "profiles": []}).to_string());
        let entry = HostAdapter
            .curated("")
            .into_iter()
            .find(|entry| !entry.env_keys.is_empty() && entry.text("command").is_some())
            .expect("the curated catalog has a local entry that needs a key");
        let upstream: crate::catalog::CatalogEntry = convert(&entry).unwrap();
        assert_eq!(convert::<_, CatalogEntryRecord>(&upstream).unwrap(), entry);
        let mut id = String::new();
        HostAdapter
            .transact(&mut |tx| {
                id = tx.add_catalog_server(&entry)?;
                Ok(())
            })
            .unwrap();
        let loaded = registry::load_resolved().unwrap();
        let server = loaded.servers.iter().find(|s| s.id == id).unwrap();
        assert_eq!(server.name, entry.name);
        assert!(!server.enabled);
        assert_eq!(
            server.source.as_deref(),
            Some(format!("catalog:{}", entry.source).as_str())
        );
        assert!(server.env.iter().all(|e| e.secret && e.value.is_none()));
        let hosted = CatalogEntryRecord {
            extra: Default::default(),
            ..entry
        };
        assert!(HostAdapter
            .transact(&mut |tx| tx.add_catalog_server(&hosted).map(drop))
            .is_err());
    }

    #[test]
    fn the_credential_and_launch_rules_are_upstreams_own() {
        let rules = HostAdapter;
        assert!(rules.credential_text("sk-abcdefghijklmnop"));
        assert!(rules.credential_text("password=hunter2"));
        assert!(!rules.credential_text("/opt/jdk"));
        assert!(!rules.credential_text("sk-x"));
        assert!(rules.spawn_env_refused("LD_PRELOAD", "/tmp/x.so"));
        assert!(rules.spawn_env_refused("NODE_OPTIONS", "--require ./x.js"));
        assert!(!rules.spawn_env_refused("JAVA_TOOL_OPTIONS", "-Xmx512m"));
    }

    /// docs/migration.md says a global off "removes it from every access set"; the code only
    /// flips the flag. The Servers screen and the restart flow rely on the code.
    #[test]
    fn turning_a_v3_server_off_keeps_its_access_set_memberships() {
        let scratch = Scratch::new("off-keeps-sets");
        scratch.write_registry(
            &json!({
                "version": 3,
                "servers": [
                    {"id": "alpha", "name": "alpha", "transport": "stdio", "command": "a", "enabled": true},
                    {"id": "beta", "name": "beta", "transport": "stdio", "command": "b", "enabled": true},
                ],
                "profiles": [
                    {"id": "focus", "name": "Focus", "enabledServerIds": ["alpha"]},
                    {"id": "wide", "name": "Wide", "enabledServerIds": ["alpha", "beta"]},
                ],
                "defaultAccessProfileId": "wide",
                "clientScopes": {"cursor": "focus"},
            })
            .to_string(),
        );
        let members = |registry: &Registry| -> Vec<Vec<String>> {
            registry
                .profiles
                .iter()
                .map(|profile| profile.enabled_server_ids.clone())
                .collect()
        };
        let before = members(&registry::load_resolved().unwrap());

        // The command behind the Servers page switch.
        registry::update(|registry| {
            crate::registry_controller::apply_server_enabled(registry, "", "alpha", false, false)
        })
        .unwrap();
        let off = registry::load_resolved().unwrap();
        assert!(!off.server_enabled("alpha"));
        assert_eq!(
            members(&off),
            before,
            "an off server stays listed in its sets"
        );
        assert!(
            !off.is_enabled("focus", "alpha"),
            "off: unreachable through its set"
        );
        assert!(!off.is_enabled("wide", "alpha"));

        registry::update(|registry| {
            crate::registry_controller::apply_server_enabled(registry, "", "alpha", true, false)
        })
        .unwrap();
        let on = registry::load_resolved().unwrap();
        assert!(
            on.is_enabled("focus", "alpha"),
            "back on: its sets reach it again"
        );
        assert!(on.is_enabled("wide", "alpha"));
        assert!(
            !on.is_enabled("focus", "beta"),
            "a server never listed stays out"
        );

        // The bulk switch and the fork's own transaction keep memberships too.
        registry::update(|registry| registry.set_all_enabled("wide", false)).unwrap();
        assert_eq!(members(&registry::load_resolved().unwrap()), before);
        HostAdapter
            .transact(&mut |tx| {
                tx.set_server_enabled("alpha", true)?;
                tx.set_server_enabled("beta", false)
            })
            .unwrap();
        let after = registry::load_resolved().unwrap();
        assert_eq!(members(&after), before);
        assert!(after.is_enabled("focus", "alpha") && !after.is_enabled("wide", "beta"));
    }

    #[test]
    fn the_self_server_is_reachable_through_the_access_sets_clients_use_and_no_other() {
        use toolport_plus::selfmcp::register::{install, Intent};
        let scratch = Scratch::new("selfmcp-access");
        scratch.write_registry(
            &json!({
                "version": 3,
                "servers": [],
                "profiles": [
                    {"id": "focus", "name": "Focus", "enabledServerIds": []},
                    {"id": "wide", "name": "Wide", "enabledServerIds": []},
                    {"id": "idle", "name": "Idle", "enabledServerIds": []},
                ],
                "defaultAccessProfileId": "focus",
                "clientScopes": {"cursor": "wide"},
            })
            .to_string(),
        );
        let done = install(&HostAdapter, None, Intent::Install).unwrap();
        let loaded = registry::load_resolved().unwrap();
        assert!(loaded.server_enabled(&done.id));
        assert!(loaded.is_enabled("focus", &done.id));
        assert!(loaded.is_enabled("wide", &done.id));
        assert!(!loaded.is_enabled("idle", &done.id));
        assert_eq!(
            scratch.raw_registry()["plus"]["selfmcp"]["enabledIn"],
            json!(["focus", "wide"])
        );
        assert_eq!(done.enabled, vec!["focus".to_string(), "wide".to_string()]);
    }
}

#[cfg(feature = "desktop")]
#[path = "plus_adapter_notifier.rs"]
mod notifier;
#[cfg(feature = "desktop")]
pub(crate) use notifier::AppNotifier;

#[cfg(test)]
#[path = "plus_adapter_oauth_tests.rs"]
mod oauth_tests;

#[cfg(test)]
#[path = "plus_adapter_hooks_tests.rs"]
mod hooks_tests;
