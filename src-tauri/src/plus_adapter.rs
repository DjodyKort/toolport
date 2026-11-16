//! Implements the `toolport-plus-api` traits over upstream items.

use std::path::PathBuf;

use toolport_plus_api::{convert, RegistryAccess, ServerRecord};

pub struct HostAdapter;

impl RegistryAccess for HostAdapter {
    fn registry_path(&self) -> Option<PathBuf> {
        crate::registry::resolved_path()
    }

    fn list_servers(&self) -> Result<Vec<ServerRecord>, String> {
        let registry = crate::registry::load_resolved()?;
        registry.servers.iter().map(convert).collect()
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{self, DataDirTestEnv, Registry, ServerEntry, REGISTRY_ENV_LOCK};

    const FIXTURE: &str = include_str!("../../plus/fixtures/server_entry.json");

    fn fixture() -> (serde_json::Value, ServerEntry) {
        let json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let entry = serde_json::from_value(json.clone()).unwrap();
        (json, entry)
    }

    #[test]
    fn mirror_round_trip_keeps_unknown_upstream_fields_and_the_plus_key() {
        let (json, entry) = fixture();
        assert!(entry.unknown_fields.contains_key("plus"));
        assert!(entry.unknown_fields.contains_key("fieldFromNewerUpstream"));

        let mut record: ServerRecord = convert(&entry).unwrap();
        assert_eq!(
            record.plus.as_ref().unwrap().origin.as_deref(),
            Some("import")
        );
        assert_eq!(record.extra["hidePrompts"], serde_json::json!(true));

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
        let _data_dir = DataDirTestEnv::new("plus-adapter");
        let _guard = REGISTRY_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = std::env::var_os("TOOLPORT_REGISTRY");
        struct RestoreEnv(Option<std::ffi::OsString>);
        impl Drop for RestoreEnv {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var("TOOLPORT_REGISTRY", value),
                    None => std::env::remove_var("TOOLPORT_REGISTRY"),
                }
            }
        }
        let _restore = RestoreEnv(previous);
        std::env::remove_var("TOOLPORT_REGISTRY");

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
}
