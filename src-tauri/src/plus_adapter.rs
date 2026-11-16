//! Implements the `toolport-plus-api` traits over upstream items.

use std::path::PathBuf;

use serde_json::{Map, Value};
use toolport_plus_api::{
    convert, plus_ops, AuthProbe, DirectEntry, ForkServerFields, McpmSource, RegistryAccess,
    RegistryPlus, SelfMcpRecord, ServerRecord,
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

impl RegistryAccess for HostAdapter {
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
}
