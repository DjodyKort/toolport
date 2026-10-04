//! Server lookup, field building and the race-free add shared by the `toolportctl server` commands
//! and the selfmcp server tools. Each surface keeps its own argument parsing and error wording.

use crate::catalog::CatalogEntry;
use crate::registry::{self, Registry, ServerEntry};
use crate::registry_controller::{self, ServerFields};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AddError {
    Exists,
    Failed(String),
}

/// A server by id, else by name ignoring case.
pub(crate) fn find<'a>(reg: &'a Registry, key: &str) -> Option<&'a ServerEntry> {
    reg.servers
        .iter()
        .find(|s| s.id == key)
        .or_else(|| named(reg, key))
}

pub(crate) fn named<'a>(reg: &'a Registry, name: &str) -> Option<&'a ServerEntry> {
    reg.servers
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
}

/// What a surface was asked to set. `None` keeps the base value; `args: Some(vec![])` clears them.
#[derive(Default)]
pub(crate) struct Patch {
    pub name: Option<String>,
    pub transport: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub url: Option<String>,
    pub cwd: Option<String>,
}

/// Without an explicit transport, a given command means stdio and a given url means http (sse
/// stays sse); otherwise the base keeps its transport and a new server defaults to stdio.
pub(crate) fn fields_from(patch: Patch, base: Option<&ServerEntry>) -> ServerFields {
    let inferred = match (
        &patch.command,
        &patch.url,
        base.map(|b| b.transport.as_str()),
    ) {
        (Some(_), _, _) => "stdio",
        (None, Some(_), Some(t @ ("http" | "sse"))) => t,
        (None, Some(_), _) => "http",
        (None, None, Some(t)) => t,
        (None, None, None) => "stdio",
    };
    ServerFields {
        name: patch
            .name
            .or_else(|| base.map(|b| b.name.clone()))
            .unwrap_or_default(),
        transport: patch.transport.unwrap_or_else(|| inferred.to_string()),
        command: patch
            .command
            .or_else(|| base.and_then(|b| b.command.clone())),
        args: patch
            .args
            .or_else(|| base.map(|b| b.args.clone()))
            .unwrap_or_default(),
        url: patch.url.or_else(|| base.and_then(|b| b.url.clone())),
        cwd: patch.cwd.or_else(|| base.and_then(|b| b.cwd.clone())),
    }
}

pub(crate) fn add_returning_id(fields: ServerFields) -> Result<String, AddError> {
    let name = fields.name.trim().to_string();
    add_unique(&name, |reg| {
        registry_controller::apply_add_server(reg, fields)
    })
}

pub(crate) fn add_catalog_returning_id(entry: CatalogEntry) -> Result<String, AddError> {
    let name = entry.name.clone();
    add_unique(&name, |reg| {
        registry_controller::apply_add_catalog_entry(reg, entry)
    })
}

/// The name check runs under the registry lock, so two writers cannot both add the same name.
fn add_unique(
    name: &str,
    add: impl FnOnce(&mut Registry) -> Result<String, String>,
) -> Result<String, AddError> {
    let mut exists = false;
    let added = registry::update(|reg| {
        exists = named(reg, name).is_some();
        if exists {
            return Err(String::new());
        }
        add(reg)
    });
    match added {
        Ok((_, id)) => Ok(id),
        Err(_) if exists => Err(AddError::Exists),
        Err(message) => Err(AddError::Failed(message)),
    }
}

/// A stdio entry with every optional field empty; callers set what differs with `..stdio_entry()`.
pub(crate) fn stdio_entry(name: &str, command: Option<String>, source: &str) -> ServerEntry {
    ServerEntry {
        id: String::new(),
        name: name.to_string(),
        transport: "stdio".into(),
        command,
        args: Vec::new(),
        launch: None,
        env: Vec::new(),
        url: None,
        cwd: None,
        source: Some(source.to_string()),
        disabled_tools: Vec::new(),
        client_credentials: None,
        request_timeout_ms: None,
        max_request_timeout_ms: None,
        initialize_timeout_ms: None,
        unknown_fields: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::testutil::DataDirFx;
    use serde_json::json;

    fn entry(id: &str, name: &str, transport: &str) -> ServerEntry {
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "transport": transport,
            "command": (transport == "stdio").then_some("base-mcp"),
            "args": ["--base"],
            "url": (transport != "stdio").then_some("https://base.example.invalid/mcp"),
            "cwd": "/base",
        }))
        .unwrap()
    }

    fn registry_of(servers: Vec<ServerEntry>) -> Registry {
        Registry {
            servers,
            ..Registry::default()
        }
    }

    fn stdio(name: &str) -> ServerFields {
        fields_from(
            Patch {
                name: Some(name.into()),
                command: Some("run-mcp".into()),
                ..Patch::default()
            },
            None,
        )
    }

    #[test]
    fn find_prefers_the_id_and_falls_back_to_the_name_ignoring_case() {
        let reg = registry_of(vec![
            entry("one", "Two", "stdio"),
            entry("two", "Three", "stdio"),
        ]);
        assert_eq!(find(&reg, "two").unwrap().name, "Three");
        assert_eq!(find(&reg, "TWO").unwrap().id, "one");
        assert_eq!(find(&reg, "three").unwrap().id, "two");
        assert!(find(&reg, "ghost").is_none());
        assert!(find(&reg, " two").is_none());
        let twins = registry_of(vec![
            entry("a", "Same", "stdio"),
            entry("b", "SAME", "stdio"),
        ]);
        assert_eq!(find(&twins, "same").unwrap().id, "a");
        assert_eq!(find(&twins, "b").unwrap().id, "b");
        assert_eq!(named(&reg, "tWo").unwrap().id, "one");
        assert!(named(&reg, "one").is_none());
    }

    #[test]
    fn a_new_server_takes_its_transport_from_the_command_or_the_url() {
        let new = |command: Option<&str>, url: Option<&str>, transport: Option<&str>| {
            fields_from(
                Patch {
                    name: Some("n".into()),
                    transport: transport.map(String::from),
                    command: command.map(String::from),
                    url: url.map(String::from),
                    ..Patch::default()
                },
                None,
            )
        };
        let stdio = new(Some("run"), None, None);
        assert_eq!(
            (
                stdio.transport.as_str(),
                stdio.command.as_deref(),
                stdio.args
            ),
            ("stdio", Some("run"), Vec::<String>::new())
        );
        assert_eq!(new(None, Some("https://x.invalid"), None).transport, "http");
        assert_eq!(
            new(Some("run"), Some("https://x.invalid"), None).transport,
            "stdio"
        );
        assert_eq!(new(None, None, None).transport, "stdio");
        assert_eq!(
            new(None, Some("https://x.invalid"), Some("sse")).transport,
            "sse"
        );
        assert_eq!(new(Some("run"), None, Some("http")).transport, "http");
    }

    #[test]
    fn an_edit_keeps_the_base_values_it_does_not_name() {
        let base = entry("b", "Base", "stdio");
        let kept = fields_from(Patch::default(), Some(&base));
        assert_eq!(
            kept,
            ServerFields {
                name: "Base".into(),
                transport: "stdio".into(),
                command: Some("base-mcp".into()),
                args: vec!["--base".into()],
                url: None,
                cwd: Some("/base".into()),
            }
        );
        let changed = fields_from(
            Patch {
                name: Some("Renamed".into()),
                args: Some(Vec::new()),
                cwd: Some("/elsewhere".into()),
                ..Patch::default()
            },
            Some(&base),
        );
        assert_eq!(
            (
                changed.name.as_str(),
                changed.args.len(),
                changed.cwd.as_deref()
            ),
            ("Renamed", 0, Some("/elsewhere"))
        );
    }

    #[test]
    fn an_edit_moves_the_transport_only_with_the_endpoint_it_sets() {
        let stdio_base = entry("s", "S", "stdio");
        let sse_base = entry("e", "E", "sse");
        let url = |u: &str| Patch {
            url: Some(u.into()),
            ..Patch::default()
        };
        let command = |c: &str| Patch {
            command: Some(c.into()),
            ..Patch::default()
        };
        assert_eq!(
            fields_from(url("https://x.invalid"), Some(&stdio_base)).transport,
            "http"
        );
        assert_eq!(
            fields_from(url("https://x.invalid"), Some(&sse_base)).transport,
            "sse"
        );
        assert_eq!(
            fields_from(command("run"), Some(&sse_base)).transport,
            "stdio"
        );
        assert_eq!(
            fields_from(Patch::default(), Some(&sse_base)).transport,
            "sse"
        );
        let pinned = Patch {
            transport: Some("sse".into()),
            ..command("run")
        };
        assert_eq!(fields_from(pinned, Some(&stdio_base)).transport, "sse");
    }

    #[test]
    fn add_returning_id_names_the_new_server_and_rejects_a_duplicate_name() {
        let fx = DataDirFx::new("servers-core", "add");
        let first = add_returning_id(stdio("  Alpha ")).unwrap();
        let second = add_returning_id(stdio("beta")).unwrap();
        assert_ne!(first, second);
        let reg = registry::load_resolved().unwrap();
        assert_eq!(find(&reg, &first).unwrap().name, "Alpha");
        assert_eq!(
            find(&reg, &second).unwrap().source.as_deref(),
            Some("manual")
        );

        let before = std::fs::read(fx.dir.join("registry.json")).unwrap();
        assert_eq!(add_returning_id(stdio("ALPHA")), Err(AddError::Exists));
        assert_eq!(add_returning_id(stdio(" alpha  ")), Err(AddError::Exists));
        assert_eq!(std::fs::read(fx.dir.join("registry.json")).unwrap(), before);
    }

    #[test]
    fn racing_adds_of_one_name_leave_exactly_one_server() {
        let _fx = DataDirFx::new("servers-core", "race");
        let outcomes: Vec<_> = (0..6)
            .map(|_| std::thread::spawn(|| add_returning_id(stdio("racer"))))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            outcomes.iter().filter(|o| o.is_ok()).count(),
            1,
            "{outcomes:?}"
        );
        assert!(outcomes
            .iter()
            .all(|o| o.is_ok() || *o == Err(AddError::Exists)));
        assert_eq!(registry::load_resolved().unwrap().servers.len(), 1);
    }

    #[test]
    fn add_returning_id_surfaces_the_controller_rejection_without_a_write() {
        let fx = DataDirFx::new("servers-core", "reject");
        let mut fields = stdio("gamma");
        fields.command = None;
        assert_eq!(
            add_returning_id(fields),
            Err(AddError::Failed("enter the command to run".into()))
        );
        let nameless = ServerFields {
            name: " ".into(),
            ..stdio("x")
        };
        assert_eq!(
            add_returning_id(nameless),
            Err(AddError::Failed("give the server a name".into()))
        );
        assert!(!fx.dir.join("registry.json").exists());
    }

    #[test]
    fn a_catalog_add_returns_the_id_and_checks_the_name_under_the_lock() {
        let _fx = DataDirFx::new("servers-core", "catalog");
        let catalog = |name: &str, command: Option<&str>| {
            serde_json::from_value::<CatalogEntry>(json!({
                "name": name,
                "description": "synthetic",
                "transport": "stdio",
                "command": command,
                "args": [],
                "url": null,
                "envKeys": [],
                "source": "curated",
            }))
            .unwrap()
        };
        let id = add_catalog_returning_id(catalog("Pg", Some("pg-mcp"))).unwrap();
        let reg = registry::load_resolved().unwrap();
        assert_eq!(find(&reg, &id).unwrap().command.as_deref(), Some("pg-mcp"));
        assert_eq!(
            add_catalog_returning_id(catalog("pg", Some("other"))),
            Err(AddError::Exists)
        );
        assert!(matches!(
            add_catalog_returning_id(catalog("Hosted", None)),
            Err(AddError::Failed(message)) if message.contains("needs its own endpoint URL")
        ));
        assert_eq!(registry::load_resolved().unwrap().servers.len(), 1);
    }

    #[test]
    fn a_server_that_vanished_fails_inside_the_update_and_the_remove() {
        let _fx = DataDirFx::new("servers-core", "vanished");
        let id = add_returning_id(stdio("delta")).unwrap();
        let fields = stdio("delta");
        registry_controller::remove_server(&id).unwrap();

        let update = registry_controller::update_server_fields(&id, fields).unwrap_err();
        assert_eq!(update, format!("No server with id '{id}'"));
        let remove = registry_controller::remove_server(&id).unwrap_err();
        assert_eq!(remove, format!("No server with id '{id}'"));
        assert!(registry::load_resolved().unwrap().servers.is_empty());
    }

    #[test]
    fn stdio_entry_matches_a_registry_entry_with_only_the_required_fields() {
        let minimal: ServerEntry = serde_json::from_value(json!({
            "id": "",
            "name": "probe",
            "transport": "stdio",
            "command": "probe-mcp",
            "source": "test:probe",
        }))
        .unwrap();
        assert_eq!(
            stdio_entry("probe", Some("probe-mcp".into()), "test:probe"),
            minimal
        );
    }
}
