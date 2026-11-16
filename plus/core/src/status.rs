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
    use toolport_plus_api::{MemoryRegistry, RegistryAccess, ServerRecord};

    fn record(id: &str, enabled: bool) -> ServerRecord {
        ServerRecord {
            id: id.into(),
            name: id.into(),
            enabled,
            ..Default::default()
        }
    }

    #[test]
    fn status_counts_enabled_servers_through_the_trait() {
        let fake = MemoryRegistry {
            profiles: 1,
            ..MemoryRegistry::with_servers(vec![record("a", true), record("b", false)])
        };
        fake.set_server_enabled("b", true).unwrap();
        let report = status("test", &fake);
        assert_eq!(
            report["registry"],
            json!({"servers": 2, "enabledServers": 2, "profiles": 1})
        );
    }
}
