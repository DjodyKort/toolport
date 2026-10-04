use super::client;
use super::commands::snapshot;
use super::flags::{switch, value, Flags, Spec};
use super::output::{CtlError, Output};
use crate::catalog::{self, CatalogEntry};
use crate::plus::servers::{self, AddError, Patch};
use crate::registry::{Registry, ServerEntry};
use crate::registry_controller;
use serde_json::{json, Value};

const SEARCH_USAGE: &str = "usage: server search [<query>] [--offline] [--limit <n>]";
const INSTALL_USAGE: &str = "usage: server install <catalog name> [--offline]";
const NEW_USAGE: &str = "usage: server new <name> (--command <cmd> [--arg <a>]... | --url <url> [--transport http|sse]) [--cwd <dir>]";
const EDIT_USAGE: &str = "usage: server edit <id|name> [--name <n>] [--command <cmd>] [--arg <a>]... [--url <url>] [--transport <t>] [--cwd <dir>]";
const INFO_USAGE: &str = "usage: server info <id|name>";
const UNINSTALL_USAGE: &str =
    "usage: server uninstall <id|name> [--dry-run] [--keep-clients] [--keep-secrets]";

const SEARCH: Spec = Spec {
    flags: &[value("--limit"), switch("--offline")],
    ..Spec::PLAIN
};
const INSTALL: Spec = Spec {
    flags: &[switch("--offline")],
    ..Spec::PLAIN
};
const NEW: Spec = Spec {
    flags: &[
        value("--command"),
        value("--arg"),
        value("--url"),
        value("--transport"),
        value("--cwd"),
    ],
    ..Spec::PLAIN
};
const EDIT: Spec = Spec {
    flags: &[
        value("--name"),
        value("--command"),
        value("--arg"),
        value("--url"),
        value("--transport"),
        value("--cwd"),
    ],
    ..Spec::PLAIN
};
const UNINSTALL: Spec = Spec {
    flags: &[
        switch("--dry-run"),
        switch("--keep-clients"),
        switch("--keep-secrets"),
    ],
    ..Spec::PLAIN
};

fn load_registry() -> Result<Registry, CtlError> {
    let snap = snapshot();
    match snap.registry_error {
        Some(error) => Err(CtlError::failed("registry_error", error)),
        None => Ok(snap.registry.unwrap_or_default()),
    }
}

/// A write must not run over an unreadable registry: the locked loader would restore it from
/// the backup instead of refusing.
fn require_readable() -> Result<(), CtlError> {
    load_registry().map(drop)
}

pub(super) fn resolve<'a>(reg: &'a Registry, key: &str) -> Result<&'a ServerEntry, CtlError> {
    servers::find(reg, key).ok_or_else(|| CtlError::not_found(format!("no server '{key}'")))
}

fn catalog_row(entry: &CatalogEntry) -> Value {
    json!({
        "name": entry.name,
        "description": entry.description,
        "transport": entry.transport,
        "source": entry.source,
        "category": entry.category,
        "command": entry.command,
        "args": entry.args,
        "url": entry.url,
        "envKeys": entry.env_keys,
    })
}

fn run_search(query: &str, offline: bool) -> Result<Vec<CatalogEntry>, CtlError> {
    if offline {
        Ok(catalog::search_curated(query))
    } else {
        catalog::search(query).map_err(|e| CtlError::failed("catalog", e))
    }
}

pub fn search(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SEARCH.parse(rest)?;
    let limit = match flags.one("--limit") {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| CtlError::usage(SEARCH_USAGE))?,
        None => usize::MAX,
    };
    let query = flags.operands().join(" ");
    let mut found = run_search(&query, flags.on("--offline"))?;
    let total = found.len();
    found.truncate(limit);
    let human = if found.is_empty() {
        "No matches.".to_string()
    } else {
        found
            .iter()
            .map(|e| {
                format!(
                    "{:<24} {:<6} {:<9} {}",
                    e.name, e.transport, e.source, e.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(Output::new(
        json!({
            "query": query,
            "total": total,
            "results": found.iter().map(catalog_row).collect::<Vec<_>>(),
        }),
        human,
    ))
}

pub fn install(rest: &[String]) -> Result<Output, CtlError> {
    let flags = INSTALL.parse(rest)?;
    let name = flags.single(INSTALL_USAGE)?;
    let found = run_search(name, flags.on("--offline"))?;
    let entry = found
        .into_iter()
        .find(|e| e.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| CtlError::not_found(format!("no catalog entry named '{name}'")))?;
    require_readable()?;
    let added = servers::add_catalog_returning_id(entry.clone()).map_err(|e| match e {
        AddError::Exists => {
            CtlError::conflict(format!("server '{}' is already installed", entry.name))
        }
        AddError::Failed(message) => CtlError::failed("install", message),
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

fn patch_from(flags: &Flags, name: Option<&str>) -> Patch {
    let own = |flag| flags.one(flag).map(String::from);
    Patch {
        name: name.map(String::from).or_else(|| own("--name")),
        transport: own("--transport"),
        command: own("--command"),
        args: Some(flags.all("--arg")).filter(|given| !given.is_empty()),
        url: own("--url"),
        cwd: own("--cwd"),
    }
}

pub fn new(rest: &[String]) -> Result<Output, CtlError> {
    let flags = NEW.parse(rest)?;
    let name = flags.single(NEW_USAGE)?.to_string();
    if flags.one("--command").is_none() && flags.one("--url").is_none() {
        return Err(CtlError::usage(NEW_USAGE));
    }
    let fields = servers::fields_from(patch_from(&flags, Some(&name)), None);
    require_readable()?;
    let added = servers::add_returning_id(fields).map_err(|e| match e {
        AddError::Exists => CtlError::conflict(format!("server '{name}' already exists")),
        AddError::Failed(message) => CtlError::failed("input", message),
    })?;
    Ok(Output::new(
        json!({"id": added, "name": name}),
        format!("Added {name} as {added}."),
    ))
}

pub fn edit(rest: &[String]) -> Result<Output, CtlError> {
    let flags = EDIT.parse(rest)?;
    let key = flags.single(EDIT_USAGE)?;
    if !flags.has_values() {
        return Err(CtlError::usage(EDIT_USAGE));
    }
    let reg = load_registry()?;
    let server = resolve(&reg, key)?;
    let id = server.id.clone();
    let fields = servers::fields_from(patch_from(&flags, None), Some(server));
    let changed: Vec<&str> = [
        ("name", fields.name != server.name),
        ("transport", fields.transport != server.transport),
        ("command", fields.command != server.command),
        ("args", fields.args != server.args),
        ("url", fields.url != server.url),
        ("cwd", fields.cwd != server.cwd),
    ]
    .iter()
    .filter(|(_, differs)| *differs)
    .map(|(n, _)| *n)
    .collect();
    registry_controller::update_server_fields(&id, fields)
        .map_err(|e| CtlError::failed("input", e))?;
    Ok(Output::new(
        json!({"id": id, "changed": changed}),
        if changed.is_empty() {
            format!("{id}: nothing changed.")
        } else {
            format!("Updated {id}: {}.", changed.join(", "))
        },
    ))
}

pub fn info(rest: &[String]) -> Result<Output, CtlError> {
    let flags = Spec::PLAIN.parse(rest)?;
    let key = flags.single(INFO_USAGE)?;
    let reg = load_registry()?;
    let s = resolve(&reg, key)?;
    let profiles: Vec<&str> = reg
        .profiles
        .iter()
        .filter(|p| reg.is_enabled(&p.id, &s.id))
        .map(|p| p.id.as_str())
        .collect();
    let env: Vec<Value> = s
        .env
        .iter()
        .map(|e| json!({"key": e.key, "secret": e.secret}))
        .collect();
    let data = json!({
        "id": s.id,
        "name": s.name,
        "transport": s.transport,
        "command": s.command,
        "args": s.args,
        "url": s.url,
        "cwd": s.cwd,
        "source": s.source,
        "env": env,
        "disabledTools": s.disabled_tools,
        "profiles": profiles,
    });
    let target = s
        .url
        .clone()
        .or_else(|| {
            s.command.as_ref().map(|c| {
                std::iter::once(c.clone())
                    .chain(s.args.clone())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        })
        .unwrap_or_default();
    let human = format!(
        "{}\nId:         {}\nTransport:  {}\nTarget:     {}\nSource:     {}\nEnv keys:   {}\nProfiles:   {}",
        s.name,
        s.id,
        s.transport,
        target,
        s.source.as_deref().unwrap_or("-"),
        if s.env.is_empty() {
            "-".to_string()
        } else {
            s.env
                .iter()
                .map(|e| if e.secret { format!("{} (secret)", e.key) } else { e.key.clone() })
                .collect::<Vec<_>>()
                .join(", ")
        },
        if profiles.is_empty() { "-".to_string() } else { profiles.join(", ") },
    );
    Ok(Output::new(data, human))
}

pub fn uninstall(rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNINSTALL.parse(rest)?;
    let key = flags.single(UNINSTALL_USAGE)?;
    let dry_run = flags.on("--dry-run");
    let reg = load_registry()?;
    let server = resolve(&reg, key)?.clone();
    let names = [server.name.clone(), server.id.clone()];
    let plans = if flags.on("--keep-clients") {
        Vec::new()
    } else {
        client::prune_matching(&names, dry_run)
    };
    let secret_keys: Vec<String> = server
        .env
        .iter()
        .filter(|e| e.secret)
        .map(|e| e.key.clone())
        .collect();
    let mut secrets_removed = Vec::new();
    if !dry_run {
        registry_controller::remove_server(&server.id)
            .map_err(|e| CtlError::failed("uninstall", e))?;
        if !flags.on("--keep-secrets") {
            for k in &secret_keys {
                if crate::secrets::delete_secret(&server.id, k).is_ok() {
                    secrets_removed.push(k.clone());
                }
            }
        }
    }
    let failed = plans.iter().any(|p| p.error.is_some());
    let mut human = format!(
        "{} {} ({}).",
        if dry_run {
            "Would uninstall"
        } else {
            "Uninstalled"
        },
        server.name,
        server.id
    );
    for plan in &plans {
        human.push('\n');
        human.push_str(&plan.line(dry_run));
    }
    let mut output = Output::new(
        json!({
            "id": server.id,
            "name": server.name,
            "dryRun": dry_run,
            "clients": plans.iter().map(client::Prune::to_value).collect::<Vec<_>>(),
            "secretsRemoved": secrets_removed,
        }),
        human,
    );
    output.failed = failed;
    Ok(output)
}

pub fn inspect(rest: &[String]) -> Result<Output, CtlError> {
    let flags = Spec::PLAIN.parse(rest)?;
    let key = flags.single("usage: inspect <server id|name>")?;
    let reg = load_registry()?;
    let server = resolve(&reg, key)?;
    let tools =
        crate::playground::list_tools(&server.id).map_err(|e| CtlError::failed("inspect", e))?;
    Ok(tools_output(&[(server.id.clone(), tools)], None))
}

pub fn profile_inspect(rest: &[String]) -> Result<Output, CtlError> {
    let flags = Spec::PLAIN.parse(rest)?;
    if flags.operands().len() > 1 {
        return Err(CtlError::usage("usage: profile inspect [<profile id>]"));
    }
    let reg = load_registry()?;
    let profile_id = match flags.operands().first() {
        Some(id) => reg
            .canonical_profile_id(id)
            .map_err(|_| CtlError::not_found(format!("no profile '{id}'")))?,
        None => reg.active_profile_id(),
    };
    let mut rows = Vec::new();
    for s in reg
        .servers
        .iter()
        .filter(|s| reg.is_enabled(&profile_id, &s.id))
    {
        let tools =
            crate::playground::list_tools(&s.id).map_err(|e| CtlError::failed("inspect", e))?;
        rows.push((s.id.clone(), tools));
    }
    Ok(tools_output(&rows, Some(&profile_id)))
}

fn tools_output(rows: &[(String, Vec<Value>)], profile: Option<&str>) -> Output {
    let servers: Vec<Value> = rows
        .iter()
        .map(|(id, tools)| {
            let list: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.get("name").cloned().unwrap_or(Value::Null),
                        "description": t.get("description").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect();
            json!({"id": id, "tools": list})
        })
        .collect();
    let human = if rows.is_empty() {
        "No servers enabled.".to_string()
    } else {
        rows.iter()
            .map(|(id, tools)| {
                let names: Vec<&str> = tools
                    .iter()
                    .filter_map(|t| t.get("name").and_then(Value::as_str))
                    .collect();
                format!("{id} ({} tools): {}", names.len(), names.join(", "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Output::new(json!({"profile": profile, "servers": servers}), human)
}
