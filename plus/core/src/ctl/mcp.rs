//! `toolportctl mcp install|uninstall|doctor|tools|call`: lifecycle of the self-management server.

use std::io::Read;
use std::path::Path;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::flags::{switch, value, Inline, Operands, Spec};
use super::output::{CtlError, Output};
use crate::selfmcp::{self, register, Gate, RESOURCES, TOOLS};

const USAGE: &str = "usage: mcp <install [--access-set <id>] | uninstall | doctor | tools | call <tool> [--args <json> | --args-stdin]>";
const CALL_USAGE: &str = "usage: mcp call <tool> [--args <json> | --args-stdin]";

pub fn run(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let Some((sub, args)) = rest.split_first() else {
        return Err(CtlError::usage(USAGE));
    };
    match sub.as_str() {
        "install" => install(host, args),
        "uninstall" => Spec::NONE.parse(args).and_then(|_| uninstall(host)),
        "doctor" => Spec::NONE.parse(args).and_then(|_| doctor(host)),
        "tools" => Spec::NONE.parse(args).and_then(|_| tools()),
        "call" => call(host, args),
        _ => Err(CtlError::usage(USAGE)),
    }
}

pub(super) const INSTALL: Spec = Spec {
    flags: &[value("--access-set")
        .alias(&["--profile"])
        .needs("an access set id")],
    ..Spec::NONE
};

pub(super) const CALL: Spec = Spec {
    flags: &[
        value("--args").needs("a JSON object"),
        switch("--args-stdin"),
    ],
    inline: Inline::Value,
    operands: Operands::Max(1, CALL_USAGE),
    ..Spec::NONE
};

const TOKEN_PREFIXES: &[&str] = &[
    "sk-",
    "ghp_",
    "gho_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "AKIA",
    "Bearer ",
    "eyJ",
    "-----BEGIN",
];
const SECRET_MAPS: &[&str] = &["env", "headers"];

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

fn closest_tools(asked: &str) -> Vec<&'static str> {
    let limit = (asked.len() / 3).max(3);
    let mut scored: Vec<(usize, &'static str)> = TOOLS
        .iter()
        .map(|t| {
            let near = t.name.contains(asked) || asked.contains(t.name);
            (
                if near {
                    0
                } else {
                    edit_distance(asked, t.name)
                },
                t.name,
            )
        })
        .filter(|(score, _)| *score <= limit)
        .collect();
    scored.sort();
    scored.into_iter().take(5).map(|(_, name)| name).collect()
}

fn unknown_tool(asked: &str) -> CtlError {
    let close = closest_tools(asked);
    CtlError::usage(if close.is_empty() {
        format!("unknown tool: {asked}; `toolportctl mcp tools` lists them")
    } else {
        format!("unknown tool: {asked}; closest: {}", close.join(", "))
    })
}

fn secret_at(value: &Value, path: &str, in_secret_map: bool) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => {
            let token = TOKEN_PREFIXES.iter().any(|p| text.starts_with(p));
            (token || in_secret_map).then(|| path.to_string())
        }
        Value::Object(map) => map.iter().find_map(|(key, inner)| {
            let here = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            if crate::redact::sensitive_key(key)
                && matches!(inner, Value::String(_) | Value::Array(_) | Value::Object(_))
                && inner.as_str() != Some("")
            {
                return Some(here);
            }
            let maps = in_secret_map || SECRET_MAPS.contains(&key.to_ascii_lowercase().as_str());
            secret_at(inner, &here, maps)
        }),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(i, inner)| secret_at(inner, &format!("{path}[{i}]"), in_secret_map)),
        _ => None,
    }
}

fn parse_arguments(text: &str, source: &str) -> Result<Value, CtlError> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let parsed: Value = serde_json::from_str(text)
        .map_err(|e| CtlError::usage(format!("{source} is not valid JSON: {e}")))?;
    if parsed.is_object() {
        Ok(parsed)
    } else {
        Err(CtlError::usage(format!("{source} must be one JSON object")))
    }
}

fn call(host: &dyn Host, args: &[String]) -> Result<Output, CtlError> {
    let flags = CALL.parse(args)?;
    let asked = flags.single(CALL_USAGE)?;
    let inline = flags.one("--args");
    let from_stdin = flags.on("--args-stdin");
    if inline.is_some() && from_stdin {
        return Err(CtlError::usage("--args and --args-stdin are exclusive"));
    }
    let tool = selfmcp::find_tool(asked).ok_or_else(|| unknown_tool(asked))?;
    let arguments = if from_stdin {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| CtlError::failed("mcp", format!("cannot read stdin: {e}")))?;
        parse_arguments(&text, "stdin")?
    } else {
        let arguments = parse_arguments(inline.unwrap_or(""), "--args")?;
        if let Some(at) = secret_at(&arguments, "", false) {
            return Err(CtlError::usage(format!(
                "--args holds a secret-looking value at `{at}`: pass the arguments with --args-stdin so it never reaches a command line"
            )));
        }
        arguments
    };
    let result = selfmcp::call_tool(host, tool.name, &arguments)
        .map_err(|e| CtlError::failed(e.kind, e.message))?;
    let human = serde_json::to_string_pretty(&result).unwrap_or_default();
    Ok(Output::new(
        json!({"tool": tool.name, "tier": tool.tier, "isError": false, "result": result}),
        human,
    ))
}

fn install(host: &dyn Host, args: &[String]) -> Result<Output, CtlError> {
    let flags = INSTALL.parse(args)?;
    let access_set = flags.one("--access-set").map(String::from);
    let installed = register::install(host, access_set.as_deref(), register::Intent::Install)
        .map_err(|e| CtlError::failed("mcp", e))?;
    let action = installed.outcome.as_str();
    let command = register::binary_path(host);
    let mut human = format!("self server '{}' {action}; command {command}", installed.id);
    if !installed.enabled.is_empty() {
        human.push_str(&format!("; listed in {}", installed.enabled.join(", ")));
    }
    Ok(Output::new(
        json!({
            "id": installed.id, "action": action, "command": command,
            "accessSet": access_set, "enabled": installed.enabled,
        }),
        human,
    ))
}

fn uninstall(host: &dyn Host) -> Result<Output, CtlError> {
    let removed = register::uninstall_self_server(host).map_err(|e| CtlError::failed("mcp", e))?;
    let human = match &removed {
        Some(id) => format!("self server '{id}' removed"),
        None => "self server is not installed".to_string(),
    };
    Ok(Output::new(
        json!({"removed": removed.is_some(), "id": removed}),
        human,
    ))
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

fn check(name: &'static str, ok: bool, detail: impl Into<String>) -> Check {
    Check {
        name,
        ok,
        detail: detail.into(),
    }
}

fn runnable(command: &str) -> bool {
    if Path::new(command).components().count() > 1 {
        return Path::new(command).is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            let candidate = dir.join(command);
            candidate.is_file() || candidate.with_extension("exe").is_file()
        })
    })
}

fn handshake_detail(host: &dyn Host) -> (bool, String) {
    let reply = selfmcp::handle_message(
        host,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    let name = reply
        .as_ref()
        .and_then(|r| r["result"]["serverInfo"]["name"].as_str());
    (
        name == Some(selfmcp::SERVER_NAME),
        name.unwrap_or("no initialize reply").to_string(),
    )
}

fn set_list(sets: &[register::SetState]) -> String {
    let ids: Vec<&str> = sets.iter().map(|s| s.id.as_str()).collect();
    ids.join(", ")
}

fn set_json(set: &register::SetState) -> Value {
    json!({"id": set.id, "member": set.member, "optedOut": set.opted_out})
}

fn doctor(host: &dyn Host) -> Result<Output, CtlError> {
    let view = host.read_view().ok().flatten();
    let status = view
        .as_ref()
        .and_then(|view| register::status(view.as_ref()).ok());
    let entry = view
        .as_ref()
        .and_then(|view| view.servers().ok())
        .and_then(|servers| register::find_self(&servers).cloned());
    let wanted = register::binary_path(host);
    let standing = status
        .as_ref()
        .map_or(register::Standing::Missing, |s| s.standing);
    let opted_out = standing == register::Standing::OptedOut;
    let mut checks = vec![check(
        "registry_entry",
        entry.is_some() || opted_out,
        match &entry {
            Some(e) => e.id.clone(),
            None if opted_out => "opted out; `toolportctl mcp install` brings it back".into(),
            None => "run `toolportctl mcp install`".into(),
        },
    )];
    if opted_out {
        for name in [
            "command_matches_binary",
            "binary_present",
            "server_enabled",
            "listed_in_default_access",
            "listed_in_client_access_sets",
        ] {
            checks.push(check(name, true, "skipped: opted out"));
        }
    } else {
        let registered = entry
            .as_ref()
            .and_then(|e| e.text("command").map(String::from));
        checks.push(check(
            "command_matches_binary",
            registered.as_deref() == Some(wanted.as_str()),
            registered.unwrap_or_else(|| wanted.clone()),
        ));
        checks.push(check("binary_present", runnable(&wanted), wanted));
        let (ok, detail) = match &entry {
            Some(e) if e.enabled => (true, "switched on".to_string()),
            Some(_) => (false, "switched off in Servers".to_string()),
            None => (false, "-".to_string()),
        };
        checks.push(check("server_enabled", ok, detail));
        let (ok, detail) = match (
            &entry,
            status.as_ref().and_then(|s| s.default_access.as_ref()),
        ) {
            (None, _) => (false, "-".to_string()),
            (Some(_), None) => (true, "the default is every enabled server".to_string()),
            (Some(_), Some(set)) if set.member => (true, set.id.clone()),
            (Some(_), Some(set)) if set.opted_out => {
                (true, format!("{}: taken out on purpose", set.id))
            }
            (Some(_), Some(set)) => (false, format!("{}: run `toolportctl mcp install`", set.id)),
        };
        checks.push(check("listed_in_default_access", ok, detail));
        let sets = status
            .as_ref()
            .map(|s| s.client_access_sets.as_slice())
            .unwrap_or(&[]);
        let unset: Vec<register::SetState> = sets
            .iter()
            .filter(|s| !s.member && !s.opted_out)
            .cloned()
            .collect();
        let off: Vec<register::SetState> = sets.iter().filter(|s| s.opted_out).cloned().collect();
        let detail = if !unset.is_empty() {
            format!(
                "not listed in {}: run `toolportctl mcp install`",
                set_list(&unset)
            )
        } else if !off.is_empty() {
            format!("taken out on purpose in {}", set_list(&off))
        } else if sets.is_empty() {
            "no client is scoped to an access set".into()
        } else {
            set_list(sets)
        };
        checks.push(check(
            "listed_in_client_access_sets",
            unset.is_empty(),
            detail,
        ));
    }
    let (shaken, who) = handshake_detail(host);
    checks.push(check("handshake", shaken, who));
    checks.push(check(
        "catalog",
        !TOOLS.is_empty() && !RESOURCES.is_empty(),
        format!("{} tools, {} resources", TOOLS.len(), RESOURCES.len()),
    ));
    let failed = checks.iter().any(|c| !c.ok);
    let mut human = format!("state {}\n", standing.as_str());
    for c in &checks {
        human.push_str(&format!(
            "{:<30} {:<4} {}\n",
            c.name,
            if c.ok { "ok" } else { "FAIL" },
            c.detail
        ));
    }
    let data = json!({
        "state": standing.as_str(),
        "switchedOn": status.as_ref().is_some_and(|s| s.switched_on),
        "defaultAccess": status.as_ref().and_then(|s| s.default_access.as_ref()).map(set_json),
        "clientAccessSets": status
            .as_ref()
            .map(|s| s.client_access_sets.iter().map(set_json).collect::<Vec<_>>())
            .unwrap_or_default(),
        "checks": checks.iter().map(|c| json!({"name": c.name, "ok": c.ok, "detail": c.detail})).collect::<Vec<_>>(),
    });
    let mut out = Output::new(data, human);
    out.failed = failed;
    Ok(out)
}

fn gate_name(gate: Gate) -> &'static str {
    match gate {
        Gate::None => "none",
        Gate::Always => "always",
        Gate::UnlessDryRun => "unless-dry-run",
    }
}

fn tools() -> Result<Output, CtlError> {
    let mut human = String::from("tools\n");
    for t in TOOLS {
        human.push_str(&format!(
            "  {:<28} tier {}  gate {:<14} {}\n",
            t.name,
            t.tier,
            gate_name(t.gate),
            t.description
        ));
    }
    human.push_str("resources\n");
    for r in RESOURCES {
        human.push_str(&format!("  {:<28} {}\n", r.uri, r.description));
    }
    let data: Value = json!({
        "server": selfmcp::SERVER_NAME,
        "tools": TOOLS.iter().map(|t| json!({
            "name": t.name, "tier": t.tier, "gate": gate_name(t.gate), "description": t.description,
            "dryRunDefault": selfmcp::previews_by_default(t),
            "params": selfmcp::tool_params(t),
        })).collect::<Vec<_>>(),
        "resources": RESOURCES.iter().map(|r| json!({
            "uri": r.uri, "name": r.name, "mimeType": r.mime, "description": r.description,
        })).collect::<Vec<_>>(),
    });
    Ok(Output::new(data, human))
}
