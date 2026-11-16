//! Implements the `toolport-plus-api` traits over upstream items.

use std::path::PathBuf;

use serde_json::{Map, Value};
use toolport_plus_api::{
    convert, plus_ops, AccessSetInfo, AuthProbe, DirectEntry, ForkServerFields, HostEnv,
    HttpAccess, HttpBody, HttpError, HttpRequest, HttpResponse, McpmSource, RegistryAccess,
    RegistryPlus, RegistryTx, RegistryView, SecretsHost, SelfMcpRecord, ServerRecord, TrustInputs,
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
}

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
        return crate::secrets::platform::read_master_key();
        #[cfg(not(target_os = "macos"))]
        Ok(None)
    }

    fn ensure_master_key(&self) -> Result<[u8; 32], String> {
        #[cfg(target_os = "macos")]
        return crate::secrets::platform::ensure_master_key();
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
}

/// Upstream's own agent, so the egress guard and upstream's resolvers apply to every request.
impl HttpAccess for HostAdapter {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let (content_type, body) = encode_body(&request.body)?;
        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if let Some(content_type) = content_type.filter(|_| !request.has_header("content-type")) {
            builder = builder.header("Content-Type", content_type);
        }
        let invalid =
            |error: ureq::http::Error| HttpError::Other(format!("invalid request: {error}"));
        let agent = crate::http_client::agent();
        let response = match body {
            Some(bytes) => run(&agent, builder.body(bytes).map_err(invalid)?, request),
            None => run(&agent, builder.body(()).map_err(invalid)?, request),
        }
        .map_err(map_error)?;
        read_response(response, request.max_body_bytes)
    }
}

fn run<B: ureq::AsSendBody>(
    agent: &ureq::Agent,
    http_request: ureq::http::Request<B>,
    request: &HttpRequest,
) -> Result<crate::http_client::Response, ureq::Error> {
    use ureq::RequestExt as _;

    let configure = http_request
        .with_agent(agent)
        .configure()
        .timeout_global(Some(request.timeout));
    if request.follow_redirects {
        configure.run()
    } else {
        // Without will_error a 3xx past the limit is returned as a response.
        configure
            .max_redirects(0)
            .max_redirects_will_error(false)
            .run()
    }
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
}
