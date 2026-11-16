//! The `servers_*` tools: what `toolportctl server` does, for agents. Add, edit and remove run
//! upstream's own operations through `crate::servers`; a profile is an access set here, listing a
//! server in it is membership, and the global switch decides whether the set reaches it.

use std::time::Duration;

use serde_json::{json, Map, Value};
use toolport_plus_api::{Host, ServerRecord};

use super::ToolError;
use crate::args::{flag, flag_or, list, str_arg};
use crate::servers::env::{Change, Refusal};
use crate::servers::restart::{self, RestartArgs};
use crate::servers::{self, mode, Patch, ServerError, UninstallArgs};

type Outcome = Result<Value, ToolError>;

const CONFIG_KEYS: [&str; 9] = [
    "command",
    "args",
    "env",
    "url",
    "transport",
    "cwd",
    "inheritEnv",
    "declareClientCapabilities",
    "forwardInstructions",
];

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::new("invalid_arguments", message)
}

fn safe_token(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-')
}

fn name_arg(args: &Value) -> Result<&str, ToolError> {
    let name = str_arg(args, "name").unwrap_or_default().trim();
    if !safe_token(name) {
        return Err(invalid("invalid server name"));
    }
    Ok(name)
}

fn registry_error(message: String) -> ToolError {
    ToolError::new("registry_error", message)
}

fn view_servers(host: &dyn Host) -> Result<Vec<ServerRecord>, ToolError> {
    servers::read_servers(host).map_err(ToolError::from)
}

fn resolve(all: &[ServerRecord], key: &str) -> Result<ServerRecord, ToolError> {
    servers::find(all, key)
        .cloned()
        .ok_or_else(|| ToolError::new("not_found", format!("server not found: {key}")))
}

fn load(host: &dyn Host, args: &Value) -> Result<ServerRecord, ToolError> {
    resolve(&view_servers(host)?, name_arg(args)?)
}

fn server_error(error: ServerError) -> ToolError {
    match error {
        ServerError::NotFound(message) => ToolError::new("not_found", message),
        ServerError::Exists => ToolError::new("conflict", "the server already exists"),
        ServerError::Refused(refusal) => refused(refusal),
        ServerError::Rejected(message) => ToolError::backend(message),
    }
}

fn refused(refusal: Refusal) -> ToolError {
    invalid(refusal.message)
}

pub(super) fn list_servers(host: &dyn Host, _: &Value) -> Outcome {
    let view = host.read_view().map_err(registry_error)?;
    servers::list_value(view.as_deref(), true).map_err(registry_error)
}

pub(super) fn get(host: &dyn Host, args: &Value) -> Outcome {
    let name = str_arg(args, "name").unwrap_or_default();
    let view = host.read_view().map_err(registry_error)?;
    let all = match &view {
        Some(view) => view.servers().map_err(registry_error)?,
        None => Vec::new(),
    };
    let server = resolve(&all, name)?;
    let sets = view
        .as_ref()
        .map(|view| servers::access_sets_of(&view.access_sets(), &server.id))
        .unwrap_or_default();
    servers::info_value(&server, &sets).map_err(registry_error)
}

pub(super) fn auth(host: &dyn Host, args: &Value) -> Outcome {
    use crate::auth::LoginError;
    use toolport_plus_api::LaunchFault;
    crate::auth::start_sign_in(host, name_arg(args)?).map_err(|error| match error {
        LoginError::NotFound(message) => ToolError::new("not_found", message),
        LoginError::Unsupported { .. } => ToolError::new("refused", error.message()),
        LoginError::Launch(launch) => match launch.fault {
            LaunchFault::Unsupported => invalid(launch.message),
            LaunchFault::Refused => ToolError::new("refused", launch.message),
            LaunchFault::Spawn => ToolError::backend(launch.message),
        },
        LoginError::Failed(message) => ToolError::backend(message),
    })
}

pub(super) fn list_profiles(host: &dyn Host, _: &Value) -> Outcome {
    let view = host.read_view().map_err(registry_error)?;
    let (sets, default_access) = match &view {
        Some(view) => (view.access_sets(), view.default_access()),
        None => (Vec::new(), None),
    };
    Ok(json!({
        "defaultAccess": default_access,
        "accessSets": sets.iter().map(|set| json!({
            "id": set.id,
            "name": set.name,
            "serverIds": set.server_ids,
        })).collect::<Vec<_>>(),
    }))
}

pub(super) fn add_profile_tag(host: &dyn Host, args: &Value) -> Outcome {
    join_set(host, args, true)
}

pub(super) fn remove_profile_tag(host: &dyn Host, args: &Value) -> Outcome {
    join_set(host, args, false)
}

fn join_set(host: &dyn Host, args: &Value, add: bool) -> Outcome {
    let server = load(host, args)?;
    let tag = str_arg(args, "profile_tag").unwrap_or_default().trim();
    if tag.is_empty() {
        return Err(invalid("profile_tag is empty"));
    }
    let sets = servers::set_member(host, &server.id, tag, add).map_err(server_error)?;
    Ok(json!({"name": server.name, "accessSets": sets, "enabled": server.enabled}))
}

fn config_string(cfg: &Map<String, Value>, key: &str) -> Result<Option<String>, ToolError> {
    match cfg.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(invalid(format!("{key} must be a string"))),
    }
}

fn config_switch(cfg: &Map<String, Value>, key: &str) -> Result<Option<bool>, ToolError> {
    match cfg.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(invalid(format!("{key} must be true or false"))),
    }
}

fn env_changes(cfg: &Map<String, Value>) -> Result<Vec<Change>, ToolError> {
    let map = match cfg.get("env") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(map)) => map,
        Some(_) => {
            return Err(invalid(
                "env must be an object of NAME: value pairs; null removes a name",
            ))
        }
    };
    map.iter()
        .map(|(key, value)| match value {
            Value::String(value) => Ok(Change::Set {
                key: key.clone(),
                value: value.clone(),
            }),
            Value::Null => Ok(Change::Unset { key: key.clone() }),
            _ => Err(invalid(
                "an env value must be a string, or null to remove it",
            )),
        })
        .collect()
}

fn check_config(cfg: &Map<String, Value>) -> Result<(), ToolError> {
    match cfg
        .keys()
        .find(|k| !CONFIG_KEYS.contains(&k.as_str()) && k.as_str() != "name")
    {
        Some(key) => Err(invalid(format!("unsupported config field: {key}"))),
        None => Ok(()),
    }
}

/// A patch never moves a server between transports on its own, unlike `toolportctl server edit`.
fn patch_from(
    name: &str,
    cfg: &Map<String, Value>,
    base: Option<&ServerRecord>,
) -> Result<Patch, ToolError> {
    check_config(cfg)?;
    let args = match cfg.get("args") {
        None => None,
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .map(|v| v.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| invalid("args must be strings"))?,
        ),
        Some(_) => return Err(invalid("args must be a list")),
    };
    let command = config_string(cfg, "command")?;
    let transport = match (config_string(cfg, "transport")?, base) {
        (Some(t), _) => t,
        (None, Some(b)) => b.text("transport").unwrap_or("stdio").to_string(),
        (None, None) if command.is_some() => "stdio".into(),
        (None, None) => "http".into(),
    };
    Ok(Patch {
        name: Some(name.to_string()),
        transport: Some(transport),
        command,
        args,
        url: config_string(cfg, "url")?,
        cwd: config_string(cfg, "cwd")?,
        inherit_env: config_switch(cfg, "inheritEnv")?,
        declare_client_capabilities: config_switch(cfg, "declareClientCapabilities")?,
        forward_instructions: config_switch(cfg, "forwardInstructions")?,
    })
}

pub(super) fn install(host: &dyn Host, args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let cfg = args
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("config must be an object"))?;
    if let Some(inner) = config_string(cfg, "name")? {
        if inner != name {
            return Err(invalid("config.name must match name"));
        }
    }
    let all = view_servers(host)?;
    let existing = servers::named(&all, name).cloned();
    let patch = patch_from(name, cfg, existing.as_ref())?;
    let env = env_changes(cfg)?;
    let exists = || {
        ToolError::new(
            "conflict",
            format!("server {name} already exists; pass force=true to replace"),
        )
    };
    let id = match existing {
        Some(_) if !flag(args, "force") => return Err(exists()),
        Some(server) => {
            servers::update(host, &server.id, &patch, &env).map_err(server_error)?;
            server.id
        }
        None => servers::add(host, &patch, &env).map_err(|error| match error {
            ServerError::Exists => exists(),
            other => server_error(other),
        })?,
    };
    let all = view_servers(host)?;
    let server = resolve(&all, &id)?;
    if let Some(tags) = list(args, "profile_tags") {
        for tag in tags.iter().filter_map(Value::as_str) {
            servers::set_member(host, &server.id, tag, true).map_err(server_error)?;
        }
    }
    Ok(json!({"installed": true, "id": id, "name": server.name}))
}

pub(super) fn update_config(host: &dyn Host, args: &Value) -> Outcome {
    let server = load(host, args)?;
    let patch = args
        .get("patch")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("patch must be an object"))?;
    if patch.contains_key("name") {
        return Err(invalid("a patch cannot rename a server"));
    }
    let fields = patch_from(&server.name, patch, Some(&server))?;
    let env = env_changes(patch)?;
    let env_changed = servers::update(host, &server.id, &fields, &env).map_err(server_error)?;
    let mut keys: Vec<&String> = patch.keys().collect();
    keys.sort();
    let mut done = json!({"id": server.id, "updatedKeys": keys});
    if patch.contains_key("env") {
        done["envChanged"] = json!(env_changed);
    }
    Ok(done)
}

pub(super) fn set_mode(host: &dyn Host, args: &Value) -> Outcome {
    let server = load(host, args)?;
    let requested = mode::parse(str_arg(args, "mode").unwrap_or_default()).map_err(invalid)?;
    mode::apply(host, &server, requested).map_err(server_error)
}

pub(super) fn uninstall(host: &dyn Host, args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let done = servers::uninstall(
        host,
        &UninstallArgs {
            key: name,
            dry_run: false,
            keep_clients: !flag_or(args, "propagate_to_clients", true),
            keep_secrets: false,
        },
    )?;
    Ok(done.to_value())
}

pub(super) fn restart_server(host: &dyn Host, args: &Value) -> Outcome {
    let name = name_arg(args)?;
    let mut ask = RestartArgs::new(name);
    ask.dry_run = flag_or(args, "dry_run", true);
    ask.if_idle = flag(args, "if_idle");
    ask.wait = restart::DEFAULT_WAIT.min(Duration::from_secs(60));
    Ok(restart::restart(host, &ask)?.to_value())
}
