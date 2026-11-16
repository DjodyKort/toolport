//! `toolportctl server ls|info|install|new|edit|uninstall|restart`: the servers the gateway
//! exposes. Add, edit and remove run upstream's own operations; plain environment entries, the
//! shell-environment switch and the restart through the gateway are this layer's.

use serde_json::json;
use toolport_plus_api::{Host, ServerRecord};

use super::flags::{switch, value, Flags, Spec};
use super::output::{CtlError, Output};
use crate::servers::env::{self, Change};
use crate::servers::restart::{self, RestartArgs};
use crate::servers::{self, Patch, ServerError, UninstallArgs};

const INSTALL_USAGE: &str = "usage: server install <catalog name> [--offline]";
const NEW_USAGE: &str = "usage: server new <name> (--command <cmd> [--arg <a>]... [--env <KEY=VALUE>]... | --url <url> [--transport http|sse]) [--cwd <dir>] [--inherit-env on|off]";
const EDIT_USAGE: &str = "usage: server edit <id|name> [--name <n>] [--command <cmd>] [--arg <a>]... [--env <KEY=VALUE>]... [--unset-env <KEY>]... [--url <url>] [--transport <t>] [--cwd <dir>] [--inherit-env on|off] [--declare-client-capabilities on|off] [--forward-instructions on|off]";
const INFO_USAGE: &str = "usage: server info <id|name>";
const UNINSTALL_USAGE: &str =
    "usage: server uninstall <id|name> [--dry-run] [--keep-clients] [--keep-secrets]";
const RESTART_USAGE: &str =
    "usage: server restart <id|name> [--dry-run] [--wait <seconds>] [--if-idle]";

pub(super) const INSTALL: Spec = Spec {
    flags: &[switch("--offline")],
    ..Spec::PLAIN
};
pub(super) const NEW: Spec = Spec {
    flags: &[
        value("--command"),
        value("--arg"),
        value("--env"),
        value("--url"),
        value("--transport"),
        value("--cwd"),
        value("--inherit-env"),
    ],
    ..Spec::PLAIN
};
pub(super) const EDIT: Spec = Spec {
    flags: &[
        value("--name"),
        value("--command"),
        value("--arg"),
        value("--env"),
        value("--unset-env"),
        value("--url"),
        value("--transport"),
        value("--cwd"),
        value("--inherit-env"),
        value("--declare-client-capabilities"),
        value("--forward-instructions"),
    ],
    ..Spec::PLAIN
};
pub(super) const UNINSTALL: Spec = Spec {
    flags: &[
        switch("--dry-run"),
        switch("--keep-clients"),
        switch("--keep-secrets"),
    ],
    ..Spec::PLAIN
};
pub(super) const RESTART: Spec = Spec {
    flags: &[switch("--dry-run"), value("--wait"), switch("--if-idle")],
    ..Spec::PLAIN
};

fn registry_error(error: String) -> CtlError {
    CtlError::failed("registry_error", error)
}

fn resolve<'a>(servers: &'a [ServerRecord], key: &str) -> Result<&'a ServerRecord, CtlError> {
    servers::find(servers, key).ok_or_else(|| CtlError::not_found(format!("no server '{key}'")))
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    super::output::no_args(rest)?;
    let view = host.read_view().map_err(registry_error)?;
    let data = servers::list_value(view.as_deref(), false).map_err(registry_error)?;
    let rows = data["servers"].as_array().cloned().unwrap_or_default();
    let human = if rows.is_empty() {
        "No servers.".to_string()
    } else {
        rows.iter()
            .map(|row| {
                format!(
                    "{:<10} {:<24} {:<6} {}",
                    if row["enabled"] == true { "on" } else { "off" },
                    row["name"].as_str().unwrap_or(""),
                    row["transport"].as_str().unwrap_or(""),
                    row["id"].as_str().unwrap_or(""),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(Output::new(data, human))
}

pub fn info(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = Spec::PLAIN.parse(rest)?;
    let key = flags.single(INFO_USAGE)?;
    let view = host.read_view().map_err(registry_error)?;
    let all = match &view {
        Some(view) => view.servers().map_err(registry_error)?,
        None => Vec::new(),
    };
    let server = resolve(&all, key)?;
    let sets = view
        .as_ref()
        .map(|view| servers::access_sets_of(&view.access_sets(), &server.id))
        .unwrap_or_default();
    let data = servers::info_value(server, &sets).map_err(registry_error)?;
    let list = |items: Vec<String>| {
        if items.is_empty() {
            "-".to_string()
        } else {
            items.join(", ")
        }
    };
    let target = match (server.text("url"), server.text("command")) {
        (Some(url), _) => url.to_string(),
        (None, Some(command)) => std::iter::once(command.to_string())
            .chain(server.strings("args"))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    let env = servers::env_entries(server)
        .iter()
        .map(|e| {
            if e.secret {
                format!("{} (secret)", e.key)
            } else {
                e.key.clone()
            }
        })
        .collect::<Vec<_>>();
    let mut human = format!(
        "{}\nId:           {}\nSwitch:       {}\nTransport:    {}\nTarget:       {}\nSource:       {}\nEnv keys:     {}\nAccess sets:  {}\nChild sees:   {}",
        server.name,
        server.id,
        if server.enabled { "on" } else { "off" },
        server.text("transport").unwrap_or("stdio"),
        target,
        server.text("source").unwrap_or("-"),
        list(env),
        list(sets),
        servers::child_environment(server),
    );
    if server.fork.declare_client_capabilities == Some(true) {
        human.push_str("\nDeclares:     client capabilities");
    }
    if server.fork.forward_instructions == Some(true) {
        human.push_str("\nForwards:     instructions");
    }
    Ok(Output::new(data, human))
}

pub fn install(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = INSTALL.parse(rest)?;
    let name = flags.single(INSTALL_USAGE)?;
    let entry = servers::find_catalog(host, name)
        .ok_or_else(|| CtlError::not_found(format!("no catalog entry named '{name}'")))?;
    servers::require_readable(host)?;
    let added = servers::add_catalog(host, &entry).map_err(|error| match error {
        ServerError::Exists => {
            CtlError::conflict(format!("server '{}' is already installed", entry.name))
        }
        other => CtlError::failed("install", describe(&other)),
    })?;
    Ok(Output::new(
        json!({"id": added, "name": entry.name, "envKeys": entry.env_keys, "source": entry.source}),
        format!(
            "Installed {} as {added}.{}",
            entry.name,
            if entry.env_keys.is_empty() {
                String::new()
            } else {
                format!(
                    " Set secrets with `toolportctl secret set {added} <KEY>`: {}",
                    entry.env_keys.join(", ")
                )
            }
        ),
    ))
}

fn describe(error: &ServerError) -> String {
    match error {
        ServerError::NotFound(message) | ServerError::Rejected(message) => message.clone(),
        ServerError::Refused(refusal) => refusal.message.clone(),
        ServerError::Exists => "the server already exists".into(),
    }
}

fn switch_flag(flags: &Flags, flag: &str) -> Result<Option<bool>, CtlError> {
    flags
        .one(flag)
        .map(|raw| servers::parse_switch(raw).map_err(|e| CtlError::usage(format!("{flag}: {e}"))))
        .transpose()
}

fn patch_from(flags: &Flags, name: Option<&str>) -> Result<Patch, CtlError> {
    let own = |flag| flags.one(flag).map(String::from);
    Ok(Patch {
        name: name.map(String::from).or_else(|| own("--name")),
        transport: own("--transport"),
        command: own("--command"),
        args: Some(flags.all("--arg")).filter(|given| !given.is_empty()),
        url: own("--url"),
        cwd: own("--cwd"),
        inherit_env: switch_flag(flags, "--inherit-env")?,
        declare_client_capabilities: switch_flag(flags, "--declare-client-capabilities")?,
        forward_instructions: switch_flag(flags, "--forward-instructions")?,
    })
}

fn env_changes(flags: &Flags) -> Result<Vec<Change>, CtlError> {
    let mut changes = Vec::new();
    for raw in flags.all("--env") {
        changes.push(env::parse_assignment(&raw).map_err(CtlError::usage)?);
    }
    for key in flags.all("--unset-env") {
        if key.is_empty() || key.contains('=') {
            return Err(CtlError::usage("--unset-env takes a name, not KEY=VALUE"));
        }
        changes.push(Change::Unset { key });
    }
    env::check_distinct(&changes).map_err(CtlError::usage)?;
    Ok(changes)
}

fn refused(refusal: env::Refusal) -> CtlError {
    CtlError::failed("input", refusal.message)
}

pub fn new(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = NEW.parse(rest)?;
    let name = flags.single(NEW_USAGE)?.to_string();
    if flags.one("--command").is_none() && flags.one("--url").is_none() {
        return Err(CtlError::usage(NEW_USAGE));
    }
    let env = env_changes(&flags)?;
    let patch = patch_from(&flags, Some(&name))?;
    let spec = servers::spec_from(&patch, None);
    env::apply(host, &mut Vec::new(), &spec.transport, &env).map_err(refused)?;
    servers::require_readable(host)?;
    let added = servers::add(host, &patch, &env).map_err(|error| match error {
        ServerError::Exists => CtlError::conflict(format!("server '{name}' already exists")),
        other => CtlError::failed("input", describe(&other)),
    })?;
    Ok(Output::new(
        json!({"id": added, "name": name}),
        format!("Added {name} as {added}."),
    ))
}

pub fn edit(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = EDIT.parse(rest)?;
    let key = flags.single(EDIT_USAGE)?;
    if !flags.has_values() {
        return Err(CtlError::usage(EDIT_USAGE));
    }
    let env = env_changes(&flags)?;
    let patch = patch_from(&flags, None)?;
    let all = servers::read_servers(host)?;
    let server = resolve(&all, key)?;
    let id = server.id.clone();
    let spec = servers::spec_from(&patch, Some(server));
    env::apply(
        host,
        &mut server.env().map_err(registry_error)?,
        &spec.transport,
        &env,
    )
    .map_err(refused)?;
    let text = |key: &str| server.text(key).map(String::from);
    let mut changed: Vec<String> = [
        ("name", spec.name != server.name),
        (
            "transport",
            spec.transport != server.text("transport").unwrap_or("stdio"),
        ),
        ("command", spec.command != text("command")),
        ("args", spec.args != server.strings("args")),
        ("url", spec.url != text("url")),
        ("cwd", spec.cwd != text("cwd")),
        (
            "inheritEnv",
            spec.inherit_env
                .is_some_and(|on| on != server.switch("inheritEnv")),
        ),
        (
            "declareClientCapabilities",
            patch
                .declare_client_capabilities
                .is_some_and(|on| on != (server.fork.declare_client_capabilities == Some(true))),
        ),
        (
            "forwardInstructions",
            patch
                .forward_instructions
                .is_some_and(|on| on != (server.fork.forward_instructions == Some(true))),
        ),
    ]
    .iter()
    .filter(|(_, differs)| *differs)
    .map(|(n, _)| n.to_string())
    .collect();
    let env_changed = servers::update(host, &id, &patch, &env)
        .map_err(|error| CtlError::failed("input", describe(&error)))?;
    changed.extend(env_changed.iter().map(|key| format!("env.{key}")));
    Ok(Output::new(
        json!({"id": id, "changed": changed}),
        if changed.is_empty() {
            format!("{id}: nothing changed.")
        } else {
            format!("Updated {id}: {}.", changed.join(", "))
        },
    ))
}

pub fn uninstall(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNINSTALL.parse(rest)?;
    let key = flags.single(UNINSTALL_USAGE)?;
    let done = servers::uninstall(
        host,
        &UninstallArgs {
            key,
            dry_run: flags.on("--dry-run"),
            keep_clients: flags.on("--keep-clients"),
            keep_secrets: flags.on("--keep-secrets"),
        },
    )?;
    let human = format!(
        "{} {} ({}).",
        if done.dry_run {
            "Would uninstall"
        } else {
            "Uninstalled"
        },
        done.name,
        done.id
    );
    let mut output = Output::new(done.to_value(), human);
    output.failed = done.failed();
    Ok(output)
}

pub fn restart(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RESTART.parse(rest)?;
    let key = flags.single(RESTART_USAGE)?;
    let mut args = RestartArgs::new(key);
    args.dry_run = flags.on("--dry-run");
    args.if_idle = flags.on("--if-idle");
    if let Some(seconds) = flags.number::<u64>("--wait")? {
        args.wait = std::time::Duration::from_secs(seconds);
    }
    let done = restart::restart(host, &args)?;
    let mut output = Output::new(done.to_value(), done.line());
    output.failed = done.failed();
    Ok(output)
}

#[cfg(test)]
mod tests;
