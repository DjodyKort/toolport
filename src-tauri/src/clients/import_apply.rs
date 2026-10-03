use super::*;

#[derive(Debug, Clone)]
pub struct ClientApply {
    pub path: String,
    pub wrote_gateway: bool,
    pub removed: Vec<String>,
    pub managed: ManagedEntry,
}

fn json_servers_key(format: &Format) -> Option<&'static str> {
    match format {
        Format::JsonMcpServers
        | Format::JsonCopilotMcpServers
        | Format::JsonDroidMcpServers
        | Format::JsonQwenMcpServers
        | Format::JsonKimiMcpServers => Some("mcpServers"),
        Format::JsonServers => Some("servers"),
        Format::JsonContextServers => Some("context_servers"),
        Format::JsonAmpMcpServers => Some("amp.mcpServers"),
        _ => None,
    }
}

fn remove_entries(def: &ClientDef, path: &Path, names: &[String]) -> Result<(), String> {
    if let Some(key) = json_servers_key(&def.format) {
        let content = read_config_file(path)?;
        let mut root = read_existing_json(&content, true)?;
        if let Some(servers) = root.get_mut(key).and_then(|v| v.as_object_mut()) {
            servers.retain(|name, _| !names.contains(name));
        }
        return atomic_write_json_config(path, Some(&content), &root, key);
    }
    if matches!(def.format, Format::TomlMcpServers) {
        let mut doc = load_toml_document(path)?;
        let servers = toml_mcp_servers_mut(&mut doc);
        for name in names {
            servers.remove(name);
        }
        return atomic_write(path, &doc.to_string());
    }
    Err(format!("removing entries is not supported for {}", def.id))
}

pub fn apply_import(
    client_id: &str,
    profile: &str,
    prune: &[String],
    dry_run: bool,
) -> Result<ClientApply, String> {
    let def = find_def(client_id).ok_or_else(|| format!("Unknown client '{client_id}'"))?;
    let detected = read_client(&def);
    if let Some(error) = detected.error {
        return Err(error);
    }
    let expected = gateway_entry(Some(profile), client_id)?;
    let managed = ManagedEntry::from_gateway_entry(&expected);
    let present = detected
        .servers
        .iter()
        .find(|s| gateway_identity_matches(&s.name, &s.name, s.command.as_deref()));
    if let Some(server) = present {
        let is_ours = server
            .command
            .as_deref()
            .is_some_and(command_is_gateway_binary);
        if !is_ours {
            return Err("existing toolport entry has a custom configuration".into());
        }
    }
    let up_to_date = present.is_some_and(|s| managed_matches_detected(s, &managed));
    let leftovers: Vec<String> = prune
        .iter()
        .filter(|n| detected.servers.iter().any(|s| &s.name == *n))
        .cloned()
        .collect();
    let path = resolved_definition_path(&def)?;
    let result = ClientApply {
        path: path.display().to_string(),
        wrote_gateway: !up_to_date,
        removed: leftovers.clone(),
        managed: managed.clone(),
    };
    if dry_run {
        return Ok(result);
    }
    let mut result = result;
    if !up_to_date {
        let outcome = install_gateway(client_id, Some(profile))?;
        if let Some(m) = outcome.managed {
            result.managed = m;
        }
    }
    if !leftovers.is_empty() {
        backup_file(client_id, &path)?;
        remove_entries(&def, &path, &leftovers)?;
    }
    Ok(result)
}
