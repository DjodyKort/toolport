//! `toolportctl mcp install|uninstall|doctor|tools`: lifecycle of the self-management server.

use super::flags::{switch, value, Inline, Operands, Spec};
use super::output::{CtlError, Output};
use crate::plus::redact;
use crate::plus::registry_ro;
use crate::plus::selfmcp::{self, register, Gate, RESOURCES, TOOLS};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

const USAGE: &str =
    "usage: mcp <install [--profile <id>] | uninstall | doctor | tools | call <tool> [--args <json> | --args-stdin]>";
const CALL_USAGE: &str = "usage: mcp call <tool> [--args <json> | --args-stdin]";

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let Some((sub, args)) = rest.split_first() else {
        return Err(CtlError::usage(USAGE));
    };
    match sub.as_str() {
        "install" => install(args),
        "uninstall" => Spec::NONE.parse(args).and_then(|_| uninstall()),
        "doctor" => Spec::NONE.parse(args).and_then(|_| doctor()),
        "tools" => Spec::NONE.parse(args).and_then(|_| tools()),
        "call" => call(args),
        _ => Err(CtlError::usage(USAGE)),
    }
}

pub(super) const INSTALL: Spec = Spec {
    flags: &[value("--profile").needs("a profile id")],
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
    "sk-", "ghp_", "gho_", "github_pat_", "xoxb-", "xoxp-", "xoxa-", "AKIA", "Bearer ", "eyJ",
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
            (if near { 0 } else { edit_distance(asked, t.name) }, t.name)
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
            if redact::sensitive_key(key)
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

fn call(args: &[String]) -> Result<Output, CtlError> {
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
    let result = selfmcp::call_tool(tool.name, &arguments)
        .map_err(|e| CtlError::failed(e.kind, e.message))?;
    let human = serde_json::to_string_pretty(&result).unwrap_or_default();
    Ok(Output::new(
        json!({"tool": tool.name, "tier": tool.tier, "isError": false, "result": result}),
        human,
    ))
}

fn install(args: &[String]) -> Result<Output, CtlError> {
    let flags = INSTALL.parse(args)?;
    let profile = flags.one("--profile").map(String::from);
    let installed = register::install(profile.as_deref(), register::Intent::Install)
        .map_err(|e| CtlError::failed("mcp", e))?;
    let action = installed.outcome.as_str();
    let command = register::binary_path();
    let mut human = format!("self server '{}' {action}; command {command}", installed.id);
    if !installed.enabled.is_empty() {
        human.push_str(&format!("; enabled in {}", installed.enabled.join(", ")));
    }
    Ok(Output::new(
        json!({
            "id": installed.id, "action": action, "command": command,
            "profile": profile, "enabled": installed.enabled,
        }),
        human,
    ))
}

fn uninstall() -> Result<Output, CtlError> {
    let removed = register::uninstall_self_server().map_err(|e| CtlError::failed("mcp", e))?;
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

fn handshake_detail() -> (bool, String) {
    let reply = selfmcp::handle_message(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    }));
    let name = reply
        .as_ref()
        .and_then(|r| r["result"]["serverInfo"]["name"].as_str());
    (
        name == Some(selfmcp::SERVER_NAME),
        name.unwrap_or("no initialize reply").to_string(),
    )
}

fn profile_list(profiles: &[register::ProfileState]) -> String {
    let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
    ids.join(", ")
}

fn doctor() -> Result<Output, CtlError> {
    let reg = registry_ro::read_opt();
    let entry = reg.as_ref().and_then(register::find_self);
    let wanted = register::binary_path();
    let status = reg.as_ref().map(register::status);
    let standing = status
        .as_ref()
        .map_or(register::Standing::Missing, |s| s.standing);
    let opted_out = standing == register::Standing::OptedOut;
    let mut checks = vec![check(
        "registry_entry",
        entry.is_some() || opted_out,
        match entry {
            Some(e) => e.id.clone(),
            None if opted_out => "opted out; `toolportctl mcp install` brings it back".into(),
            None => "run `toolportctl mcp install`".into(),
        },
    )];
    if opted_out {
        checks.push(check("command_matches_binary", true, "skipped: opted out"));
        checks.push(check("binary_present", true, "skipped: opted out"));
        checks.push(check(
            "enabled_in_active_profile",
            true,
            "skipped: opted out",
        ));
        checks.push(check(
            "enabled_in_client_profiles",
            true,
            "skipped: opted out",
        ));
    } else {
        let registered = entry.and_then(|e| e.command.clone());
        checks.push(check(
            "command_matches_binary",
            registered.as_deref() == Some(wanted.as_str()),
            registered.unwrap_or_else(|| wanted.clone()),
        ));
        checks.push(check("binary_present", runnable(&wanted), wanted));
        let active = status.as_ref().and_then(|s| s.active.as_ref());
        let (ok, detail) = match active {
            Some(p) if p.enabled => (true, p.id.clone()),
            Some(p) if p.opted_out => (true, format!("{}: disabled on purpose", p.id)),
            Some(p) => (false, format!("{}: run `toolportctl mcp install`", p.id)),
            None => (false, "-".into()),
        };
        checks.push(check("enabled_in_active_profile", ok, detail));
        let clients = status.as_ref().map(|s| s.clients.as_slice()).unwrap_or(&[]);
        let unset: Vec<register::ProfileState> = clients
            .iter()
            .filter(|p| !p.enabled && !p.opted_out)
            .cloned()
            .collect();
        let off: Vec<register::ProfileState> =
            clients.iter().filter(|p| p.opted_out).cloned().collect();
        let detail = if !unset.is_empty() {
            format!(
                "not enabled in {}: run `toolportctl mcp install`",
                profile_list(&unset)
            )
        } else if !off.is_empty() {
            format!("disabled on purpose in {}", profile_list(&off))
        } else if clients.is_empty() {
            "no client is scoped to a profile".into()
        } else {
            profile_list(clients)
        };
        checks.push(check(
            "enabled_in_client_profiles",
            unset.is_empty(),
            detail,
        ));
    }
    let (shaken, who) = handshake_detail();
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
            "{:<28} {:<4} {}\n",
            c.name,
            if c.ok { "ok" } else { "FAIL" },
            c.detail
        ));
    }
    let profile_json = |p: &register::ProfileState| json!({"id": p.id, "enabled": p.enabled, "optedOut": p.opted_out});
    let data = json!({
        "state": standing.as_str(),
        "activeProfile": status.as_ref().and_then(|s| s.active.as_ref()).map(profile_json),
        "clientProfiles": status
            .as_ref()
            .map(|s| s.clients.iter().map(profile_json).collect::<Vec<_>>())
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

#[cfg(test)]
mod tests {
    use crate::plus::ctl::run_with;
    use crate::plus::selfmcp::register::SELF_SOURCE;
    use crate::plus::testutil::DataDirFx;
    use serde_json::{json, Value};

    fn run(list: &[&str]) -> (i32, Value) {
        let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&args, &mut out, &mut err);
        let out = String::from_utf8(out).unwrap();
        (code, serde_json::from_str(out.trim()).expect(&out))
    }

    fn registry_with_profile(fx: &DataDirFx) {
        fx.write_registry(&json!({
            "version": 1,
            "servers": [],
            "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
            "activeProfileId": "default"
        }));
    }

    fn registry_json(fx: &DataDirFx) -> Value {
        serde_json::from_str(&std::fs::read_to_string(fx.dir.join("registry.json")).unwrap())
            .unwrap()
    }

    fn check_of<'a>(value: &'a Value, name: &str) -> &'a Value {
        value["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("no check {name}"))
    }

    #[test]
    fn install_registers_once_and_is_idempotent() {
        let fx = DataDirFx::new("toolportctl-mcp", "install");
        let (code, first) = run(&["--json", "mcp", "install"]);
        assert_eq!(code, 0, "{first}");
        assert_eq!(first["data"]["action"], "created");
        let (_, second) = run(&["--json", "mcp", "install"]);
        assert_eq!(second["data"]["action"], "unchanged");
        assert_eq!(first["data"]["id"], second["data"]["id"]);
        let servers = registry_json(&fx)["servers"].as_array().unwrap().clone();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0]["source"], SELF_SOURCE);
    }

    #[test]
    fn install_with_a_profile_enables_the_server_there() {
        let fx = DataDirFx::new("toolportctl-mcp", "profile");
        registry_with_profile(&fx);
        let (code, done) = run(&["--json", "mcp", "install", "--profile", "default"]);
        assert_eq!(code, 0, "{done}");
        let id = done["data"]["id"].as_str().unwrap().to_string();
        assert_eq!(
            registry_json(&fx)["profiles"][0]["enabledServerIds"],
            json!([id])
        );
        let (_, again) = run(&["--json", "mcp", "install", "--profile", "default"]);
        assert_eq!(
            registry_json(&fx)["profiles"][0]["enabledServerIds"],
            json!([id])
        );
        assert_eq!(again["data"]["action"], "unchanged");
    }

    #[test]
    fn install_with_an_unknown_profile_writes_nothing() {
        let fx = DataDirFx::new("toolportctl-mcp", "badprofile");
        registry_with_profile(&fx);
        let (code, value) = run(&["--json", "mcp", "install", "--profile", "nope"]);
        assert_eq!(code, 1, "{value}");
        assert_eq!(value["error"]["code"], "mcp");
        assert!(registry_json(&fx)["servers"].as_array().unwrap().is_empty());
    }

    #[test]
    fn uninstall_removes_the_entry_and_its_profile_membership() {
        let fx = DataDirFx::new("toolportctl-mcp", "uninstall");
        registry_with_profile(&fx);
        run(&["--json", "mcp", "install", "--profile", "default"]);
        let (code, gone) = run(&["--json", "mcp", "uninstall"]);
        assert_eq!(code, 0, "{gone}");
        assert_eq!(gone["data"]["removed"], true);
        let reg = registry_json(&fx);
        assert!(reg["servers"].as_array().unwrap().is_empty());
        assert_eq!(reg["profiles"][0]["enabledServerIds"], json!([]));
        let (code, again) = run(&["--json", "mcp", "uninstall"]);
        assert_eq!(code, 0);
        assert_eq!(again["data"]["removed"], false);
    }

    #[test]
    fn doctor_fails_until_installed_enabled_and_the_binary_exists() {
        let fx = DataDirFx::new("toolportctl-mcp", "doctor");
        registry_with_profile(&fx);
        let (code, before) = run(&["--json", "mcp", "doctor"]);
        assert_eq!(code, 1, "{before}");
        assert_eq!(check_of(&before, "registry_entry")["ok"], false);
        assert_eq!(check_of(&before, "handshake")["ok"], true);
        assert_eq!(
            check_of(&before, "catalog")["detail"],
            "97 tools, 11 resources"
        );

        run(&["--json", "mcp", "install", "--profile", "default"]);
        let (code, missing_binary) = run(&["--json", "mcp", "doctor"]);
        assert_eq!(code, 1);
        assert_eq!(check_of(&missing_binary, "registry_entry")["ok"], true);
        assert_eq!(
            check_of(&missing_binary, "command_matches_binary")["ok"],
            true
        );
        assert_eq!(
            check_of(&missing_binary, "enabled_in_active_profile")["ok"],
            true
        );
        assert_eq!(check_of(&missing_binary, "binary_present")["ok"], false);

        std::fs::create_dir_all(fx.dir.join("bin")).unwrap();
        let binary = format!("toolport-selfmcp{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(fx.dir.join("bin").join(&binary), "").unwrap();
        run(&["--json", "mcp", "install"]);
        let (code, healthy) = run(&["--json", "mcp", "doctor"]);
        assert_eq!(code, 0, "{healthy}");
        assert!(healthy["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["ok"] == true));
    }

    #[test]
    fn doctor_leaves_the_data_directory_untouched() {
        let fx = DataDirFx::new("toolportctl-mcp", "readonly");
        let (code, _) = run(&["--json", "mcp", "doctor"]);
        assert_eq!(code, 1);
        assert!(!fx.dir.join("registry.json").exists());
    }

    #[test]
    fn tools_lists_the_whole_catalog_with_gates() {
        let _fx = DataDirFx::new("toolportctl-mcp", "tools");
        let (code, value) = run(&["--json", "mcp", "tools"]);
        assert_eq!(code, 0, "{value}");
        let data = &value["data"];
        assert_eq!(data["server"], "toolport-plus-self");
        let tools = data["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 97);
        assert_eq!(data["resources"].as_array().unwrap().len(), 11);
        for tool in tools {
            assert!(
                ["none", "always", "unless-dry-run"].contains(&tool["gate"].as_str().unwrap()),
                "{tool}"
            );
            let tier = tool["tier"].as_u64().unwrap();
            if tier >= 3 {
                assert_ne!(tool["gate"], "none", "{tool}");
            }
        }
    }

    #[test]
    fn tools_carry_the_fields_a_form_is_built_from() {
        let _fx = DataDirFx::new("toolportctl-mcp", "toolfields");
        let (_, value) = run(&["--json", "mcp", "tools"]);
        let tools = value["data"]["tools"].as_array().unwrap();
        let find = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap();
        let drops = find("skills_clean");
        assert_eq!(drops["dryRunDefault"], true);
        let params = drops["params"].as_array().unwrap();
        assert!(params
            .iter()
            .any(|p| p["name"] == "dry_run" && p["type"] == "boolean" && p["required"] == false));
        assert!(params.iter().any(|p| p["name"] == "confirm"));
        let edit = find("skills_edit_body");
        assert_eq!(edit["dryRunDefault"], false);
        let body = edit["params"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "new_body")
            .unwrap();
        assert_eq!((body["type"].as_str(), body["required"].as_bool()), (Some("string"), Some(true)));
        for tool in tools {
            assert!(tool["dryRunDefault"].is_boolean(), "{tool}");
            for param in tool["params"].as_array().unwrap() {
                assert!(param["description"].is_string() && param["type"].is_string(), "{tool}");
            }
        }
    }

    #[test]
    fn call_runs_a_tool_through_the_self_mcp_dispatch() {
        let _fx = DataDirFx::new("toolportctl-mcp", "call");
        let (code, value) = run(&["--json", "mcp", "call", "where_am_i"]);
        assert_eq!(code, 0, "{value}");
        let data = &value["data"];
        assert_eq!(data["tool"], "where_am_i");
        assert_eq!(data["tier"], 1);
        assert_eq!(data["isError"], false);
        assert_eq!(
            data["result"],
            crate::plus::selfmcp::call_tool("where_am_i", &json!({})).unwrap()
        );
        let (code, same) = run(&["--json", "mcp", "call", "where_am_i", "--args", "{}"]);
        assert_eq!(code, 0);
        assert_eq!(same["data"], data.clone());
    }

    #[test]
    fn call_reports_the_tools_own_error_kind_and_message() {
        let _fx = DataDirFx::new("toolportctl-mcp", "callerr");
        let (code, refused) = run(&[
            "--json", "mcp", "call", "skills_delete", "--args", r#"{"name":"demo"}"#,
        ]);
        assert_eq!(code, 1, "{refused}");
        assert_eq!(refused["ok"], false);
        assert_eq!(refused["error"]["code"], "refused");
        assert!(refused["error"]["message"].as_str().unwrap().contains("confirm=true"));
        let (code, bad) = run(&["--json", "mcp", "call", "skills_get", "--args", "{}"]);
        assert_eq!(code, 1, "{bad}");
        assert_eq!(bad["error"]["code"], "invalid_arguments");
    }

    #[test]
    fn call_names_the_closest_tools_for_an_unknown_one() {
        let _fx = DataDirFx::new("toolportctl-mcp", "callunknown");
        let (code, value) = run(&["--json", "mcp", "call", "skils_get"]);
        assert_eq!(code, 2, "{value}");
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("closest: skills_get"), "{message}");
        let (code, far) = run(&["--json", "mcp", "call", "zzzzzzzzzzzz"]);
        assert_eq!(code, 2);
        assert!(far["error"]["message"].as_str().unwrap().contains("mcp tools"));
    }

    #[test]
    fn call_refuses_a_secret_looking_inline_value_and_points_to_stdin() {
        let _fx = DataDirFx::new("toolportctl-mcp", "callsecret");
        for args in [
            r#"{"patch":{"api_key":"FAKE-inline-value-0001"}}"#,
            r#"{"config":{"env":{"LOG":"FAKE-inline-value-0002"}}}"#,
            r#"{"name":"sk-FAKE-inline-value-0003"}"#,
        ] {
            let (code, value) = run(&["--json", "mcp", "call", "servers_install", "--args", args]);
            assert_eq!(code, 2, "{value}");
            let message = value["error"]["message"].as_str().unwrap();
            assert!(message.contains("--args-stdin"), "{message}");
            assert!(!message.contains("FAKE-inline-value"), "{message}");
        }
    }

    #[test]
    fn call_rejects_bad_arguments_before_running_anything() {
        let _fx = DataDirFx::new("toolportctl-mcp", "callusage");
        for argv in [
            vec!["mcp", "call"],
            vec!["mcp", "call", "where_am_i", "extra"],
            vec!["mcp", "call", "where_am_i", "--args"],
            vec!["mcp", "call", "where_am_i", "--args", "[1]"],
            vec!["mcp", "call", "where_am_i", "--args", "{"],
            vec!["mcp", "call", "where_am_i", "--args", "{}", "--args-stdin"],
            vec!["mcp", "call", "where_am_i", "--bogus"],
        ] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let args: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
            assert_eq!(run_with(&args, &mut out, &mut err), 2, "{argv:?}");
        }
    }

    #[test]
    fn mcp_rejects_unknown_subcommands_and_arguments() {
        let _fx = DataDirFx::new("toolportctl-mcp", "usage");
        for argv in [
            vec!["mcp"],
            vec!["mcp", "serve"],
            vec!["mcp", "install", "--bogus"],
            vec!["mcp", "install", "--profile"],
            vec!["mcp", "tools", "extra"],
            vec!["mcp", "doctor", "extra"],
            vec!["mcp", "uninstall", "extra"],
        ] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let args: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
            assert_eq!(run_with(&args, &mut out, &mut err), 2, "{argv:?}");
        }
    }
}
