//! `access-set tools`: which tools of a server an access set exposes. A server with no entry in a
//! set exposes all its tools; a list exposes exactly those, and an empty list none.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::{failed, find_set, note, quote, server_name, step, view, Draft};
use crate::op::OpError;
use crate::servers;

const RULE: &str = "A server with no entry in a set exposes all its tools; an entry lists the only tools it exposes, and an empty entry none. The names are the tool names the server itself reports.";
const MAX_TOOLS: usize = 500;

type Scopes = BTreeMap<String, BTreeMap<String, Vec<String>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolChange<'a> {
    /// Expose exactly these tools of the server; none when the list is empty.
    Only(&'a [String]),
    /// Remove the entry: every tool of the server is exposed again.
    All,
}

pub struct ToolsArgs<'a> {
    pub set: &'a str,
    pub server: &'a str,
    pub change: ToolChange<'a>,
    pub dry_run: bool,
}

/// What `profiles[].toolScope` of the registry file holds, by access set id and server id.
fn read_scopes(host: &dyn Host) -> Result<Scopes, OpError> {
    let Some(path) = host.registry_path() else {
        return Ok(Scopes::new());
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Scopes::new()),
        Err(error) => return Err(failed(format!("cannot read the registry: {error}"))),
    };
    let registry: Value = serde_json::from_str(&text)
        .map_err(|error| failed(format!("the registry is not valid JSON: {error}")))?;
    let mut scopes = Scopes::new();
    for profile in registry["profiles"].as_array().into_iter().flatten() {
        let (Some(id), Some(map)) = (profile["id"].as_str(), profile["toolScope"].as_object())
        else {
            continue;
        };
        let servers = map
            .iter()
            .map(|(server, tools)| {
                let tools = tools
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|tool| tool.as_str().map(str::to_string))
                    .collect();
                (server.clone(), tools)
            })
            .collect();
        scopes.insert(id.to_string(), servers);
    }
    Ok(scopes)
}

pub fn tools_ls(host: &dyn Host, set: Option<&str>) -> Result<Value, OpError> {
    let Some(view) = host.read_view().map_err(failed)? else {
        return Ok(json!({"rule": RULE, "accessSets": []}));
    };
    let sets = view.access_sets();
    let chosen: Vec<_> = match set {
        Some(key) => vec![find_set(&sets, key)?],
        None => sets,
    };
    let known = view.servers().unwrap_or_default();
    let scopes = read_scopes(host)?;
    let rows: Vec<Value> = chosen
        .iter()
        .map(|set| {
            let narrowed: Vec<Value> = scopes
                .get(&set.id)
                .into_iter()
                .flatten()
                .map(|(server, tools)| {
                    json!({
                        "server": {"id": server, "name": server_name(&known, server)},
                        "listed": set.server_ids.contains(server),
                        "tools": tools,
                    })
                })
                .collect();
            json!({"id": set.id, "name": set.name, "narrowed": narrowed})
        })
        .collect();
    Ok(json!({"rule": RULE, "accessSets": rows}))
}

fn clean(tools: &[String]) -> Result<Vec<String>, OpError> {
    let mut seen: Vec<String> = Vec::new();
    for tool in tools {
        let tool = tool.trim();
        if tool.is_empty() {
            return Err(OpError::usage("a tool name is empty"));
        }
        if !seen.iter().any(|known| known == tool) {
            seen.push(tool.to_string());
        }
    }
    if seen.len() > MAX_TOOLS {
        return Err(OpError::usage(format!(
            "at most {MAX_TOOLS} tools can be listed"
        )));
    }
    Ok(seen)
}

fn restore(set: &str, server: &str, before: Option<&Vec<String>>) -> String {
    let base = format!(
        "toolportctl access-set tools {} {}",
        quote(set),
        quote(server)
    );
    match before {
        None => format!("{base} --all"),
        Some(tools) if tools.is_empty() => format!("{base} --none"),
        Some(tools) => format!(
            "{base}{}",
            tools
                .iter()
                .map(|tool| format!(" --tool {}", quote(tool)))
                .collect::<String>()
        ),
    }
}

pub fn tools_set(host: &dyn Host, args: &ToolsArgs) -> Result<Value, OpError> {
    let view = view(host)?;
    let sets = view.access_sets();
    let set = find_set(&sets, args.set)?;
    let known = view.servers().map_err(failed)?;
    let server = servers::find(&known, args.server.trim())
        .ok_or_else(|| OpError::not_found(format!("no server '{}'", args.server.trim())))?;
    if !set.server_ids.contains(&server.id) {
        return Err(OpError::conflict(format!(
            "{} is not listed in {}; list it there before narrowing its tools",
            server.name, set.name
        )));
    }
    let before = read_scopes(host)?
        .get(&set.id)
        .and_then(|servers| servers.get(&server.id))
        .cloned();
    let after = match args.change {
        ToolChange::All => None,
        ToolChange::Only(tools) => Some(clean(tools)?),
    };
    let same = before == after;
    let mut warnings = Vec::new();
    let (summary, detail) = match &after {
        None => (
            format!("Expose every tool of {} in {}", server.name, set.name),
            format!("remove the tool list of {} in {}", server.name, set.name),
        ),
        Some(tools) if tools.is_empty() => {
            warnings.push(format!(
                "{} exposes no tool in {}, so a client bound to it cannot call the server",
                server.name, set.name
            ));
            (
                format!("Expose no tool of {} in {}", server.name, set.name),
                format!("set an empty tool list for {} in {}", server.name, set.name),
            )
        }
        Some(tools) => (
            format!(
                "Limit {} in {} to {} tool(s)",
                server.name,
                set.name,
                tools.len()
            ),
            format!(
                "expose only {} of {} in {}",
                tools.join(", "),
                server.name,
                set.name
            ),
        ),
    };
    if after.as_ref().is_some_and(|tools| !tools.is_empty()) {
        warnings.push(
            "tool names are not checked against what the server lists; a misspelt name hides that tool"
                .to_string(),
        );
    }
    let steps = vec![if same {
        note("the tool list is already this")
    } else {
        step(detail)
    }];
    let (set_id, server_id, list) = (set.id.clone(), server.id.clone(), after.clone());
    let mut data = super::finish(
        host,
        args.dry_run,
        Draft {
            summary: if same {
                format!(
                    "The tools of {} in {} are up to date",
                    server.name, set.name
                )
            } else {
                summary
            },
            steps,
            warnings,
            undo: restore(&set.name, &server.name, before.as_ref()),
        },
        !same,
        &mut |tx| tx.set_access_tools(&set_id, &server_id, list.clone()),
        json!({}),
    )?;
    data["accessSet"] = json!({"id": set.id, "name": set.name});
    data["server"] = json!({"id": server.id, "name": server.name});
    data["tools"] = json!(after);
    data["before"] = json!(before);
    Ok(data)
}
