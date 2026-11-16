use serde_json::{json, Value};
use toolport_plus_api::RegistryAccess;

/// What `toolportctl status` prints.
pub fn status(binary: &str, registry: &dyn RegistryAccess) -> Value {
    let counts = match (registry.list_servers(), registry.profile_count()) {
        (Ok(servers), Ok(profiles)) => json!({
            "servers": servers.len(),
            "enabledServers": servers.iter().filter(|server| server.enabled).count(),
            "profiles": profiles,
        }),
        (Err(error), _) | (_, Err(error)) => json!({ "error": error }),
    };
    json!({
        "binary": binary,
        "registryPath": registry.registry_path().map(|path| path.display().to_string()),
        "registry": counts,
    })
}

#[cfg(test)]
mod tests {
    use super::status;
    use serde_json::json;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use toolport_plus_api::{RegistryAccess, ServerRecord};

    struct Fake {
        servers: RefCell<Vec<ServerRecord>>,
    }

    fn record(id: &str, enabled: bool) -> ServerRecord {
        ServerRecord {
            id: id.into(),
            name: id.into(),
            enabled,
            plus: None,
            extra: Default::default(),
        }
    }

    impl RegistryAccess for Fake {
        fn registry_path(&self) -> Option<PathBuf> {
            Some(PathBuf::from("/fake/registry.json"))
        }

        fn list_servers(&self) -> Result<Vec<ServerRecord>, String> {
            Ok(self.servers.borrow().clone())
        }

        fn profile_count(&self) -> Result<usize, String> {
            Ok(1)
        }

        fn set_server_enabled(&self, server_id: &str, enabled: bool) -> Result<(), String> {
            let mut servers = self.servers.borrow_mut();
            let server = servers
                .iter_mut()
                .find(|server| server.id == server_id)
                .ok_or_else(|| format!("unknown server {server_id}"))?;
            server.enabled = enabled;
            Ok(())
        }
    }

    #[test]
    fn status_counts_enabled_servers_through_the_trait() {
        let fake = Fake {
            servers: RefCell::new(vec![record("a", true), record("b", false)]),
        };
        fake.set_server_enabled("b", true).unwrap();
        let report = status("test", &fake);
        assert_eq!(
            report["registry"],
            json!({"servers": 2, "enabledServers": 2, "profiles": 1})
        );
    }
}
