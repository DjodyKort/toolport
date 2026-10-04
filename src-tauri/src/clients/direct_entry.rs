use super::*;

#[derive(Debug, Clone)]
pub struct EntryWrite {
    pub path: String,
    pub backup: Option<String>,
}

struct JsonLayout {
    key: &'static str,
    formatter: fn(&ServerEntry) -> serde_json::Value,
    tools_allowlist: bool,
}

fn json_layout(format: &Format) -> Option<JsonLayout> {
    let (key, formatter, tools_allowlist): (_, fn(&ServerEntry) -> serde_json::Value, _) =
        match format {
            Format::JsonMcpServers => ("mcpServers", entry_to_json, false),
            Format::JsonCopilotMcpServers => ("mcpServers", entry_to_json, true),
            Format::JsonDroidMcpServers => ("mcpServers", entry_to_droid_json, false),
            Format::JsonAmpMcpServers => ("amp.mcpServers", entry_to_json, false),
            Format::JsonQwenMcpServers => ("mcpServers", entry_to_qwen_json, false),
            Format::JsonKimiMcpServers => ("mcpServers", entry_to_kimi_json, false),
            Format::JsonServers => ("servers", entry_to_json, false),
            Format::JsonMcp => ("mcp", entry_to_crush_json, false),
            Format::JsonContextServers => ("context_servers", entry_to_json, false),
            _ => return None,
        };
    Some(JsonLayout {
        key,
        formatter,
        tools_allowlist,
    })
}

fn load_json_root(path: &Path) -> Result<(Option<String>, serde_json::Value), String> {
    if !path.exists() {
        return Ok((None, serde_json::Value::Object(serde_json::Map::new())));
    }
    let content = read_config_file(path)?;
    let root = read_existing_json(&content, true)?;
    Ok((Some(content), root))
}

fn json_servers_mut<'a>(
    root: &'a mut serde_json::Value,
    key: &str,
    create: bool,
) -> Result<Option<&'a mut serde_json::Map<String, serde_json::Value>>, String> {
    let object = root
        .as_object_mut()
        .ok_or("Client config root must be an object; leaving it untouched.")?;
    match object.get(key) {
        Some(value) if !value.is_object() => {
            return Err(format!(
                "'{key}' must be an object; leaving the client config untouched."
            ));
        }
        Some(_) => {}
        None if create => {
            object.insert(
                key.to_string(),
                serde_json::Value::Object(serde_json::Map::new()),
            );
        }
        None => return Ok(None),
    }
    Ok(object.get_mut(key).and_then(|v| v.as_object_mut()))
}

fn edit_json(
    path: &Path,
    layout: &JsonLayout,
    name: &str,
    entry: Option<&ServerEntry>,
) -> Result<(), String> {
    let (original, mut root) = load_json_root(path)?;
    let Some(servers) = json_servers_mut(&mut root, layout.key, entry.is_some())? else {
        return Ok(());
    };
    match entry {
        Some(entry) => {
            let mut value = (layout.formatter)(entry);
            if layout.tools_allowlist {
                value
                    .as_object_mut()
                    .unwrap()
                    .insert("tools".into(), serde_json::json!(["*"]));
            }
            servers.insert(name.to_string(), value);
        }
        None => {
            if servers.remove(name).is_none() {
                return Ok(());
            }
        }
    }
    atomic_write_json_config(path, original.as_deref(), &root, layout.key)
}

fn edit_opencode(path: &Path, name: &str, entry: Option<&ServerEntry>) -> Result<(), String> {
    let (original, mut root) = load_json_root(path)?;
    if entry.is_none() && root.get("mcp").is_none() {
        return Ok(());
    }
    let mcp = opencode_mcp_mut(&mut root)?;
    match entry {
        Some(entry) => {
            mcp.insert(name.to_string(), entry_to_opencode_json(entry));
        }
        None => {
            if mcp.remove(name).is_none() {
                return Ok(());
            }
        }
    }
    atomic_write_json_config(path, original.as_deref(), &root, "mcp")
}

fn edit_toml(path: &Path, name: &str, entry: Option<&ServerEntry>) -> Result<(), String> {
    let mut doc = load_toml_document(path)?;
    if entry.is_none() && doc.get("mcp_servers").is_none() {
        return Ok(());
    }
    let keep_header = toml_keeps_servers_header(&doc);
    let servers = toml_mcp_servers_mut(&mut doc);
    match entry {
        Some(entry) => {
            servers.insert(name, toml_value_to_item(&entry_to_toml(entry)));
        }
        None => {
            if servers.remove(name).is_none() {
                return Ok(());
            }
        }
    }
    servers.set_implicit(!(servers.is_empty() || keep_header));
    atomic_write(path, &doc.to_string())
}

fn yaml_root_checked(path: &Path) -> Result<(Option<String>, serde_yaml::Value), String> {
    let (original, root) = read_existing_yaml_with_source(path)?;
    if !root.is_mapping() && !root.is_null() {
        return Err("Client config root must be a mapping; leaving it untouched.".into());
    }
    Ok((original, root))
}

fn yaml_section_is(
    root: &serde_yaml::Value,
    key: &str,
    want_sequence: bool,
) -> Result<bool, String> {
    match root.get(key) {
        None | Some(serde_yaml::Value::Null) => Ok(false),
        Some(value) if want_sequence && value.is_sequence() => Ok(true),
        Some(value) if !want_sequence && value.is_mapping() => Ok(true),
        Some(_) => Err(format!(
            "'{key}' has an unexpected shape; leaving the client config untouched."
        )),
    }
}

fn edit_yaml_map(
    path: &Path,
    key: &'static str,
    name: &str,
    entry: Option<&ServerEntry>,
    to_value: fn(&ServerEntry) -> serde_yaml::Value,
) -> Result<(), String> {
    let (original, mut root) = yaml_root_checked(path)?;
    let present = yaml_section_is(&root, key, false)?;
    if entry.is_none() && !present {
        return Ok(());
    }
    let map = if key == "extensions" {
        yaml_extensions_mut(&mut root)
    } else {
        hermes_mcp_servers_mut(&mut root)
    };
    let name_key = serde_yaml::Value::String(name.to_string());
    match entry {
        Some(entry) => {
            map.insert(name_key, to_value(entry));
        }
        None => {
            if map.remove(&name_key).is_none() {
                return Ok(());
            }
        }
    }
    atomic_write_yaml_config(path, original.as_deref(), &root, key)
}

fn edit_continue(path: &Path, name: &str, entry: Option<&ServerEntry>) -> Result<(), String> {
    let (original, mut root) = yaml_root_checked(path)?;
    let present = yaml_section_is(&root, "mcpServers", true)?;
    if entry.is_none() && !present {
        return Ok(());
    }
    let list = continue_servers_mut(&mut root);
    let is_named = |server: &serde_yaml::Value| {
        server
            .as_mapping()
            .and_then(|mapping| mapping.get("name"))
            .and_then(|value| value.as_str())
            == Some(name)
    };
    match entry {
        Some(entry) => {
            let value = entry_to_continue_yaml(entry);
            match list.iter_mut().find(|server| is_named(server)) {
                Some(slot) => *slot = value,
                None => list.push(value),
            }
        }
        None => {
            let before = list.len();
            list.retain(|server| !is_named(server));
            if list.len() == before {
                return Ok(());
            }
        }
    }
    atomic_write_yaml_config(path, original.as_deref(), &root, "mcpServers")
}

/// Upsert (`Some`) or remove (`None`) the single entry called `name` in a client
/// config, leaving every other key and server alone. Unlike the gateway editors this
/// never matches on gateway identity, so a direct entry cannot disturb the gateway.
pub fn set_direct_entry(
    client_id: &str,
    name: &str,
    entry: Option<&ServerEntry>,
) -> Result<EntryWrite, String> {
    let def = find_def(client_id).ok_or_else(|| format!("Unknown client '{client_id}'"))?;
    let path = resolved_definition_path(&def)?;
    match def.format {
        Format::JsonAmpMcpServers => {
            if path.exists() {
                let content = read_config_file(&path)?;
                validate_amp_settings_shape(&read_existing_json(&content, true)?)?;
            }
        }
        Format::JsonMcp => {
            if path.exists() {
                let content = read_config_file(&path)?;
                validate_crush_settings_shape(&read_existing_json(&content, true)?)?;
            }
        }
        _ => {}
    }
    let backup = backup_file(client_id, &path)?;
    match (&def.format, json_layout(&def.format)) {
        (_, Some(layout)) => edit_json(&path, &layout, name, entry)?,
        (Format::JsonOpenCodeMcp, None) => edit_opencode(&path, name, entry)?,
        (Format::TomlMcpServers, None) => edit_toml(&path, name, entry)?,
        (Format::YamlExtensions, None) => {
            edit_yaml_map(&path, "extensions", name, entry, entry_to_goose_yaml)?
        }
        (Format::YamlMcpServers, None) => {
            edit_yaml_map(&path, "mcp_servers", name, entry, entry_to_hermes_yaml)?
        }
        (Format::YamlMcpServersList, None) => edit_continue(&path, name, entry)?,
        _ => return Err(format!("direct entries are not supported for {}", def.id)),
    }
    Ok(EntryWrite {
        path: path.display().to_string(),
        backup: backup.map(|b| b.display().to_string()),
    })
}
