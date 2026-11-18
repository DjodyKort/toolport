//! Fork-owned writer for one named client entry. It lives inside `clients` because the lock,
//! the optimistic revision check and the recovery journal (`mutation`, `restore`) are private
//! there. Entries are rendered by upstream's own formatters and written through its
//! `mutation::run`, so a Disconnect later treats the file like any other Toolport touched.
use super::*;

pub(crate) const CHANGED_SINCE_PLAN: &str = "the client file changed since the plan";

#[derive(Debug, Clone)]
pub(crate) struct EntryWrite {
    pub path: String,
    pub backup: Option<String>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct EntryTarget {
    pub path: PathBuf,
    pub format_tag: &'static str,
    pub remote: bool,
}

struct JsonLayout {
    key: &'static str,
    formatter: fn(&ServerEntry) -> serde_json::Value,
    tools_allowlist: bool,
}

enum Layout {
    Json(JsonLayout),
    Toml,
    Unsupported(&'static str),
}

const YAML_OR_PLUGIN: &str = "its config is not a JSON or TOML server list";
const OWN_SHAPE: &str = "its entry shape differs from the standard one";

const fn json(
    key: &'static str,
    formatter: fn(&ServerEntry) -> serde_json::Value,
    tools_allowlist: bool,
) -> Layout {
    Layout::Json(JsonLayout {
        key,
        formatter,
        tools_allowlist,
    })
}

/// Every upstream format is listed, so a new variant fails the build until someone decides
/// whether a direct entry can be written there.
fn layout(format: Format) -> Layout {
    match format {
        Format::JsonMcpServers => json("mcpServers", entry_to_json, false),
        Format::JsonCursorMcpServers => json("mcpServers", entry_to_typed_json, false),
        Format::JsonCopilotMcpServers => json("mcpServers", entry_to_copilot_json, true),
        Format::JsonDroidMcpServers => json("mcpServers", entry_to_droid_json, false),
        Format::JsonQwenMcpServers => json("mcpServers", entry_to_qwen_json, false),
        Format::JsonKimiMcpServers => json("mcpServers", entry_to_kimi_json, false),
        Format::JsonServers => json("servers", entry_to_typed_json, false),
        Format::JsonContextServers => json("context_servers", entry_to_json, false),
        Format::TomlMcpServers => Layout::Toml,
        Format::JsonAmpMcpServers
        | Format::JsonAuggieMcpServers
        | Format::JsonZCodeMcp
        | Format::JsonMcp
        | Format::JsonOpenCodeMcp
        | Format::JsonOpenClawMcp
        | Format::JsonOpenHandsMcpServers
        | Format::JsonNanobotMcp
        | Format::JsonCodewhaleServers => Layout::Unsupported(OWN_SHAPE),
        Format::YamlExtensions
        | Format::YamlMcpServers
        | Format::YamlMcpServersList
        | Format::YamlDshPatch
        | Format::TomlReasonixPlugins => Layout::Unsupported(YAML_OR_PLUGIN),
    }
}

fn format_tag(format: Format) -> &'static str {
    match layout(format) {
        Layout::Json(_) => "json",
        Layout::Toml => "toml",
        Layout::Unsupported(_) => "unsupported",
    }
}

/// Where a direct entry for `client_id` would be written, or why none can be.
pub(crate) fn target(client_id: &str) -> Result<EntryTarget, String> {
    let def = find_def(client_id).ok_or_else(|| format!("unknown client '{client_id}'"))?;
    if let Layout::Unsupported(reason) = layout(def.format) {
        return Err(format!("{} cannot hold a direct entry: {reason}", def.name));
    }
    let path = resolved_definition_path(&def)?;
    Ok(EntryTarget {
        path,
        format_tag: format_tag(def.format),
        remote: !def.uses_connectors,
    })
}

/// The reason a client cannot hold a direct entry, or `None` when it can.
pub(crate) fn unsupported_reason(client_id: &str) -> Option<String> {
    target(client_id).err()
}

/// Hash of the client file's text, `None` when it does not exist. Same value the journal keeps.
pub(crate) fn file_revision(client_id: &str) -> Result<Option<String>, String> {
    let target = target(client_id)?;
    revision_of(&target.path)
}

fn revision_of(path: &Path) -> Result<Option<String>, String> {
    if !mutation::exists(path) {
        return Ok(None);
    }
    Ok(Some(crate::registry::sha256_hex(&read_config_file(path)?)))
}

fn load_json_root(path: &Path) -> Result<(Option<String>, serde_json::Value), String> {
    if !mutation::exists(path) {
        return Ok((None, serde_json::Value::Object(serde_json::Map::new())));
    }
    let content = read_config_file(path)?;
    let root = read_existing_json(&content, true)?;
    Ok((Some(content), root))
}

fn json_value(layout: &JsonLayout, entry: &ServerEntry) -> serde_json::Value {
    let mut value = (layout.formatter)(entry);
    if layout.tools_allowlist {
        if let Some(fields) = value.as_object_mut() {
            fields.insert("tools".into(), serde_json::json!(["*"]));
        }
    }
    value
}

fn edit_json(
    path: &Path,
    layout: &JsonLayout,
    name: &str,
    entry: Option<&ServerEntry>,
) -> Result<(), String> {
    let (original, mut root) = load_json_root(path)?;
    let object = root
        .as_object_mut()
        .ok_or("Client config root must be an object; leaving it untouched.")?;
    match object.get(layout.key) {
        Some(value) if !value.is_object() => {
            return Err(format!(
                "'{}' must be an object; leaving the client config untouched.",
                layout.key
            ));
        }
        Some(_) => {}
        None if entry.is_some() => {
            object.insert(
                layout.key.to_string(),
                serde_json::Value::Object(serde_json::Map::new()),
            );
        }
        None => return Ok(()),
    }
    let servers = object
        .get_mut(layout.key)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("Client config servers must be an object; leaving it untouched.")?;
    match entry {
        Some(entry) => {
            servers.insert(name.to_string(), json_value(layout, entry));
        }
        None => {
            if servers.remove(name).is_none() {
                return Ok(());
            }
        }
    }
    atomic_write_json_config(path, original.as_deref(), &root, layout.key)
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

/// What the entry looks like in the client file, for a plan.
pub(crate) fn render_entry(
    client_id: &str,
    name: &str,
    entry: &ServerEntry,
) -> Result<String, String> {
    let def = find_def(client_id).ok_or_else(|| format!("unknown client '{client_id}'"))?;
    match layout(def.format) {
        Layout::Json(layout) => {
            let value = json_value(&layout, entry);
            Ok(format!(
                "\"{name}\": {}",
                serde_json::to_string(&value).map_err(|error| error.to_string())?
            ))
        }
        Layout::Toml => {
            let mut doc = toml_edit::DocumentMut::new();
            let servers = toml_mcp_servers_mut(&mut doc);
            servers.insert(name, toml_value_to_item(&entry_to_toml(entry)));
            Ok(doc.to_string().trim_end().to_string())
        }
        Layout::Unsupported(reason) => {
            Err(format!("{} cannot hold a direct entry: {reason}", def.name))
        }
    }
}

/// Upserts (`Some`) or removes (`None`) the one entry called `name`, leaving every other key and
/// server alone. `expect` is the file revision the plan saw: the write is refused when the
/// file differs, so a plan never overwrites an edit made after it.
pub(crate) fn set_entry(
    client_id: &str,
    name: &str,
    entry: Option<&ServerEntry>,
    expect: Option<&Option<String>>,
) -> Result<EntryWrite, String> {
    let def = find_def(client_id).ok_or_else(|| format!("unknown client '{client_id}'"))?;
    let layout = layout(def.format);
    if let Layout::Unsupported(reason) = layout {
        return Err(format!("{} cannot hold a direct entry: {reason}", def.name));
    }
    let path = resolved_definition_path(&def)?;
    if let Some(location) = &def.location {
        locations::remember(location)?;
    }
    let outcome = mutation::run(client_id, &path, def.format, || {
        if let Some(expected) = expect {
            if &revision_of(&path)? != expected {
                return Err(CHANGED_SINCE_PLAN.to_string());
            }
        }
        revision_outcome(client_id, &path, || {
            let backup = backup_file(client_id, &path)?;
            match &layout {
                Layout::Json(layout) => edit_json(&path, layout, name, entry)?,
                Layout::Toml => edit_toml(&path, name, entry)?,
                Layout::Unsupported(_) => unreachable!("refused above"),
            }
            Ok(WriteOutcome {
                existed_before: true,
                path: path.display().to_string(),
                backup: backup.map(|backup| backup.display().to_string()),
                managed: None,
                restored: Vec::new(),
                used_move_record: false,
                revision: None,
                warnings: Vec::new(),
                recovery_path: None,
            })
        })
    })?;
    Ok(EntryWrite {
        path: outcome.path,
        backup: outcome.backup,
        revision: outcome.revision,
    })
}

#[cfg(test)]
mod tests {
    use super::super::paths_tests::{read, write, Fixture};
    use super::*;
    use serde_json::json;

    fn launcher(name: &str) -> ServerEntry {
        serde_json::from_value(json!({
            "id": name,
            "name": name,
            "transport": "stdio",
            "command": "/opt/toolport/toolportctl",
            "args": ["direct", "run", name],
            "env": [],
            "source": "plus:direct",
        }))
        .unwrap()
    }

    fn remote(name: &str) -> ServerEntry {
        serde_json::from_value(json!({
            "id": name,
            "name": name,
            "transport": "http",
            "url": "https://example.invalid/mcp",
            "source": "plus:direct",
        }))
        .unwrap()
    }

    #[test]
    fn test01_json_entry_is_inserted_and_other_entries_survive() {
        let fixture = Fixture::new("direct-json");
        let path = client_config_path("claude-code").unwrap();
        write(
            &path,
            &json!({"mcpServers": {"native": {"command": "native"}}, "theme": "dark"}),
        );
        let written = set_entry("claude-code", "alpha", Some(&launcher("alpha")), None).unwrap();
        let after = read(&path);
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["mcpServers"]["native"], json!({"command": "native"}));
        assert_eq!(
            after["mcpServers"]["alpha"]["args"],
            json!(["direct", "run", "alpha"])
        );
        assert!(written.backup.is_some());
        assert_eq!(written.revision, file_revision("claude-code").unwrap());
        drop(fixture);
    }

    #[test]
    fn test02_removing_an_entry_leaves_the_rest_and_a_missing_entry_is_a_no_op() {
        let fixture = Fixture::new("direct-json-rm");
        let path = client_config_path("claude-code").unwrap();
        write(
            &path,
            &json!({"mcpServers": {"alpha": {"command": "x"}, "native": {"command": "n"}}}),
        );
        set_entry("claude-code", "alpha", None, None).unwrap();
        assert_eq!(
            read(&path),
            json!({"mcpServers": {"native": {"command": "n"}}})
        );
        let before = std::fs::read_to_string(&path).unwrap();
        set_entry("claude-code", "alpha", None, None).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        drop(fixture);
    }

    #[test]
    fn test03_typed_formats_carry_their_type_field_and_copilot_its_tools_allowlist() {
        let fixture = Fixture::new("direct-typed");
        for (client, key, kind) in [
            ("vscode", "servers", Some("stdio")),
            ("cursor", "mcpServers", Some("stdio")),
            ("github-copilot-cli", "mcpServers", Some("local")),
        ] {
            let path = client_config_path(client).unwrap();
            write(&path, &json!({}));
            set_entry(client, "alpha", Some(&launcher("alpha")), None).unwrap();
            let entry = &read(&path)[key]["alpha"];
            assert_eq!(entry["type"].as_str(), kind, "{client}");
            if client == "github-copilot-cli" {
                assert_eq!(entry["tools"], json!(["*"]));
            }
        }
        drop(fixture);
    }

    #[test]
    fn test04_a_remote_entry_holds_only_the_address() {
        let fixture = Fixture::new("direct-remote");
        let path = client_config_path("claude-code").unwrap();
        write(&path, &json!({}));
        set_entry("claude-code", "figma", Some(&remote("figma")), None).unwrap();
        assert_eq!(
            read(&path)["mcpServers"]["figma"],
            json!({"url": "https://example.invalid/mcp", "type": "http"})
        );
        drop(fixture);
    }

    #[test]
    fn test05_toml_entry_is_inserted_into_mcp_servers_and_removed_again() {
        let fixture = Fixture::new("direct-toml");
        let path = client_config_path("codex").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "# mine\nmodel = \"x\"\n\n[mcp_servers.native]\ncommand = \"n\"\n",
        )
        .unwrap();
        set_entry("codex", "alpha", Some(&launcher("alpha")), None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# mine"), "{text}");
        assert!(text.contains("[mcp_servers.alpha]"), "{text}");
        assert!(text.contains("[mcp_servers.native]"), "{text}");
        set_entry("codex", "alpha", None, None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("alpha"), "{text}");
        assert!(text.contains("[mcp_servers.native]"), "{text}");
        drop(fixture);
    }

    #[test]
    fn test06_a_file_that_changed_since_the_plan_is_refused_and_left_alone() {
        let fixture = Fixture::new("direct-conflict");
        let path = client_config_path("claude-code").unwrap();
        write(&path, &json!({"mcpServers": {}}));
        let planned = file_revision("claude-code").unwrap();
        write(&path, &json!({"mcpServers": {"edited": {"command": "e"}}}));
        let before = std::fs::read_to_string(&path).unwrap();
        let error = set_entry(
            "claude-code",
            "alpha",
            Some(&launcher("alpha")),
            Some(&planned),
        )
        .unwrap_err();
        assert!(error.contains(CHANGED_SINCE_PLAN), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        drop(fixture);
    }

    #[test]
    fn test07_yaml_and_own_shape_clients_report_a_reason_instead_of_writing() {
        let fixture = Fixture::new("direct-unsupported");
        for client in ["goose", "opencode", "continue"] {
            let reason = unsupported_reason(client);
            assert!(
                reason.is_some_and(|text| text.contains("cannot hold")),
                "{client}"
            );
        }
        assert!(unsupported_reason("claude-code").is_none());
        assert!(unsupported_reason("codex").is_none());
        drop(fixture);
    }

    #[test]
    fn test08_a_non_object_server_list_is_refused_without_touching_the_file() {
        let fixture = Fixture::new("direct-shape");
        let path = client_config_path("claude-code").unwrap();
        write(&path, &json!({"mcpServers": []}));
        let before = std::fs::read_to_string(&path).unwrap();
        let error = set_entry("claude-code", "alpha", Some(&launcher("alpha")), None).unwrap_err();
        assert!(error.contains("must be an object"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        drop(fixture);
    }
}
