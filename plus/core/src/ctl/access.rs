//! `toolportctl access-set ls|add|rm|default`, `client scope` and `folder route ls|add|rm`:
//! the access-set writers, rendered over `access`, which owns the checks, the plan and the
//! write.

use serde_json::Value;
use toolport_plus_api::Host;

use super::context::plan_text;
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{no_args, CtlError, Output};
use crate::access::{self, AddArgs, Change, Orphans, RmArgs, ToolChange, ToolsArgs};

const ADD_USAGE: &str = "usage: access-set add <name> [--server <id|name>]... [--dry-run]";
const RM_USAGE: &str = "usage: access-set rm <set> [--detach | --leave] [--dry-run]";
const DEFAULT_USAGE: &str = "usage: access-set default (<set> | --none) [--dry-run]";
const TOOLS_USAGE: &str = "usage: access-set tools [<set>] | <set> <server> (--tool <name>... | --all | --none) [--dry-run]";
const SCOPE_USAGE: &str = "usage: client scope <client> (<set> | --none) [--dry-run]";
const ROUTE_ADD_USAGE: &str = "usage: folder route add <folder> <set> [--dry-run]";
const ROUTE_RM_USAGE: &str = "usage: folder route rm <folder> [--dry-run]";

const fn spec(flags: &'static [super::flags::Flag], operands: Operands) -> Spec {
    Spec {
        flags,
        inline: Inline::Strict,
        unknown: Unknown::Argument,
        operands,
        ..Spec::PLAIN
    }
}

pub(super) const ADD: Spec = spec(
    &[value("--server"), switch("--dry-run")],
    Operands::Max(1, ADD_USAGE),
);
pub(super) const RM: Spec = spec(
    &[switch("--detach"), switch("--leave"), switch("--dry-run")],
    Operands::Max(1, RM_USAGE),
);
pub(super) const DEFAULT: Spec = spec(
    &[switch("--none"), switch("--dry-run")],
    Operands::Max(1, DEFAULT_USAGE),
);
pub(super) const TOOLS: Spec = spec(
    &[
        value("--tool"),
        switch("--all"),
        switch("--none"),
        switch("--dry-run"),
    ],
    Operands::Max(2, TOOLS_USAGE),
);
pub(super) const SCOPE: Spec = spec(
    &[switch("--none"), switch("--dry-run")],
    Operands::Max(2, SCOPE_USAGE),
);
pub(super) const ROUTE_ADD: Spec = spec(&[switch("--dry-run")], Operands::Max(2, ROUTE_ADD_USAGE));
pub(super) const ROUTE_RM: Spec = spec(&[switch("--dry-run")], Operands::Max(1, ROUTE_RM_USAGE));

fn texts(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn names(servers: &Value) -> String {
    let names: Vec<&str> = servers
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|server| server["name"].as_str())
        .collect();
    if names.is_empty() {
        "no servers".to_string()
    } else {
        names.join(", ")
    }
}

pub fn set_ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let data = access::ls(host)?;
    let mut human = String::new();
    for set in data["accessSets"].as_array().into_iter().flatten() {
        let mut uses = Vec::new();
        if set["default"] == Value::Bool(true) {
            uses.push("default access".to_string());
        }
        for client in texts(&set["clients"]) {
            uses.push(format!("client {client}"));
        }
        for folder in texts(&set["folders"]) {
            uses.push(format!("folder {folder}"));
        }
        human.push_str(&format!(
            "{:<18} {}  used by: {}\n",
            set["name"].as_str().unwrap_or(""),
            names(&set["servers"]),
            if uses.is_empty() {
                "nothing".to_string()
            } else {
                uses.join(", ")
            }
        ));
    }
    if data["defaultAccess"].is_null() {
        human.push_str("default access: every enabled server\n");
    }
    Ok(Output::new(data, human))
}

pub fn set_add(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = ADD.parse(rest)?;
    let servers = flags.all("--server");
    let data = access::add(
        host,
        &AddArgs {
            name: flags.single(ADD_USAGE)?,
            servers: &servers,
            dry_run: flags.on("--dry-run"),
        },
    )?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn set_rm(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RM.parse(rest)?;
    let orphans = match (flags.on("--detach"), flags.on("--leave")) {
        (true, true) => return Err(CtlError::usage("choose --detach or --leave, not both")),
        (true, false) => Some(Orphans::Detach),
        (false, true) => Some(Orphans::Leave),
        (false, false) => None,
    };
    let data = access::rm(
        host,
        &RmArgs {
            set: flags.single(RM_USAGE)?,
            orphans,
            dry_run: flags.on("--dry-run"),
        },
    )?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn set_tools(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = TOOLS.parse(rest)?;
    let tools = flags.all("--tool");
    let changing = !tools.is_empty() || flags.on("--all") || flags.on("--none");
    let set = flags.operands().first().map(String::as_str);
    let Some(server) = flags.operands().get(1) else {
        if changing || flags.on("--dry-run") {
            return Err(CtlError::usage(TOOLS_USAGE));
        }
        let data = access::tools_ls(host, set)?;
        let mut human = String::new();
        for row in data["accessSets"].as_array().into_iter().flatten() {
            let narrowed = row["narrowed"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            if narrowed.is_empty() {
                human.push_str(&format!(
                    "{:<18} every tool of every server\n",
                    row["name"].as_str().unwrap_or("")
                ));
            }
            for entry in narrowed {
                let list = texts(&entry["tools"]);
                human.push_str(&format!(
                    "{:<18} {}: {}\n",
                    row["name"].as_str().unwrap_or(""),
                    entry["server"]["name"].as_str().unwrap_or(""),
                    if list.is_empty() {
                        "no tools".to_string()
                    } else {
                        list.join(", ")
                    }
                ));
            }
        }
        return Ok(Output::new(data, human));
    };
    let set = set.ok_or_else(|| CtlError::usage(TOOLS_USAGE))?;
    let change = match (tools.is_empty(), flags.on("--all"), flags.on("--none")) {
        (false, false, false) => ToolChange::Only(&tools),
        (true, true, false) => ToolChange::All,
        (true, false, true) => ToolChange::Only(&[]),
        _ => return Err(CtlError::usage(TOOLS_USAGE)),
    };
    let data = access::tools_set(
        host,
        &ToolsArgs {
            set,
            server,
            change,
            dry_run: flags.on("--dry-run"),
        },
    )?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

fn change_of<'a>(
    operand: Option<&'a String>,
    none: bool,
    usage: &str,
) -> Result<Change<'a>, CtlError> {
    match (operand, none) {
        (Some(_), true) => Err(CtlError::usage(format!(
            "give a set or --none, not both\n{usage}"
        ))),
        (Some(set), false) => Ok(Change::Set(set)),
        (None, true) => Ok(Change::Clear),
        (None, false) => Err(CtlError::usage(usage)),
    }
}

pub fn set_default(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = DEFAULT.parse(rest)?;
    let change = change_of(flags.operands().first(), flags.on("--none"), DEFAULT_USAGE)?;
    let data = access::default(host, change, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn client_scope(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SCOPE.parse(rest)?;
    let client = flags
        .operands()
        .first()
        .ok_or_else(|| CtlError::usage(SCOPE_USAGE))?;
    let change = change_of(flags.operands().get(1), flags.on("--none"), SCOPE_USAGE)?;
    let data = access::scope(host, client, change, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn route_ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let data = access::routes(host)?;
    let mut human = String::new();
    for route in data["routes"].as_array().into_iter().flatten() {
        let set = match route["accessSet"]["name"].as_str() {
            Some(name) => format!("{name}: {}", names(&route["servers"])),
            None => format!(
                "{} (no such access set, reaches nothing)",
                route["reference"].as_str().unwrap_or("")
            ),
        };
        human.push_str(&format!(
            "{}  ->  {set}\n",
            route["path"].as_str().unwrap_or("")
        ));
    }
    if human.is_empty() {
        human.push_str("no folder routes (folder route add <folder> <set>)\n");
    }
    human.push_str(&format!("{}\n", data["rule"].as_str().unwrap_or("")));
    Ok(Output::new(data, human))
}

pub fn route_add(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = ROUTE_ADD.parse(rest)?;
    let [path, set] = flags.operands() else {
        return Err(CtlError::usage(ROUTE_ADD_USAGE));
    };
    let data = access::route_add(host, path, set, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn route_rm(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = ROUTE_RM.parse(rest)?;
    let data = access::route_rm(host, flags.single(ROUTE_RM_USAGE)?, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}
