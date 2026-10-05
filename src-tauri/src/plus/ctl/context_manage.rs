//! `toolportctl context init|status|client|profile|disable`: renderers over the
//! `plus.context.*` handlers in `context/manage.rs`. Texts follow mcpm's `context` group with
//! `toolportctl` in the hints (D-042); warnings share stdout with the actions because the ctl
//! has one text stream.

use super::flags::{switch, value, Flag, Spec};
use super::output::{CtlError, ErrorKind, Output};
use super::skills_repo::{no_operands, operand, spec};
use crate::plus::context::{layer_manage, layers, manage};
use serde_json::{json, Value};

const INIT_USAGE: &str = "usage: context init [--home <dir>] [--dry-run] [--yes] [--rewrite-zshrc]";
const STATUS_USAGE: &str = "usage: context status [--home <dir>]";
const CLIENT_ADD_USAGE: &str = "usage: context client add <name> [--glob <pattern>] \
     [--scope global|glob|folder] [--folder <dir>]... [--import <path-or-layer>]... \
     [--delivery import|copy] [--home <dir>] [--dry-run]";
const CLIENT_EDIT_USAGE: &str = "usage: context client edit <name> [--glob <pattern>] \
     [--scope global|glob|folder] [--folder <dir>]... [--import <path-or-layer>]... \
     [--delivery import|copy] [--home <dir>] [--dry-run]";
const CLIENT_RM_USAGE: &str = "usage: context client rm <name> [--home <dir>] [--dry-run]";
const COMPOSE_USAGE: &str = "usage: context compose [--cwd <dir>]";
const CLIENT_LIST_USAGE: &str = "usage: context client list [--home <dir>]";
const PROFILE_ADD_USAGE: &str = "usage: context profile add <name> [--no-org] \
     [--org-mode import|copy] [--rules inherit|none|<a,b>] [--servers inherit|none|<a,b>] \
     [--no-commands] [--no-skills] [--home <dir>] [--dry-run]";
const PROFILE_LIST_USAGE: &str = "usage: context profile list [--home <dir>]";
const PROFILE_REMOVE_USAGE: &str =
    "usage: context profile remove <name> [--purge] [--home <dir>] [--dry-run]";
const DISABLE_USAGE: &str = "usage: context disable [--purge-profiles] [--home <dir>] [--dry-run]";

pub const CLIENT_GROUP_USAGE: &str = "usage: context client add|edit|rm|list (add|edit: <name> \
     [--glob <pattern>] [--scope global|glob|folder] [--folder <dir>]... [--import <path-or-layer>]... \
     [--delivery import|copy] [--home <dir>] [--dry-run]; rm: <name> [--home <dir>] [--dry-run]; \
     list: [--home <dir>])";
pub const PROFILE_GROUP_USAGE: &str = "usage: context profile add|list|remove (add: <name> \
     [--no-org] [--org-mode import|copy] [--rules inherit|none|<a,b>] \
     [--servers inherit|none|<a,b>] [--no-commands] [--no-skills] [--home <dir>] [--dry-run]; \
     list: [--home <dir>]; remove: <name> [--purge] [--home <dir>] [--dry-run])";

pub(super) const INIT: Spec = spec(
    &[
        value("--home"),
        switch("--dry-run"),
        switch("--yes"),
        switch("--rewrite-zshrc"),
    ],
    INIT_USAGE,
);
pub(super) const STATUS: Spec = spec(&[value("--home")], STATUS_USAGE);
const LAYER_FLAGS: &[Flag] = &[
    value("--home"),
    value("--glob"),
    value("--scope"),
    value("--folder"),
    value("--import"),
    value("--delivery"),
    switch("--dry-run"),
];
pub(super) const CLIENT_ADD: Spec = spec(LAYER_FLAGS, CLIENT_ADD_USAGE);
pub(super) const CLIENT_EDIT: Spec = spec(LAYER_FLAGS, CLIENT_EDIT_USAGE);
pub(super) const CLIENT_RM: Spec = spec(
    &[value("--home"), switch("--dry-run")],
    CLIENT_RM_USAGE,
);
pub(super) const COMPOSE: Spec = spec(&[value("--cwd")], COMPOSE_USAGE);
pub(super) const CLIENT_LIST: Spec = spec(&[value("--home")], CLIENT_LIST_USAGE);
pub(super) const PROFILE_ADD: Spec = spec(
    &[
        value("--home"),
        value("--org-mode"),
        value("--rules"),
        value("--servers"),
        switch("--no-org"),
        switch("--no-commands"),
        switch("--no-skills"),
        switch("--dry-run"),
    ],
    PROFILE_ADD_USAGE,
);
pub(super) const PROFILE_LIST: Spec = spec(&[value("--home")], PROFILE_LIST_USAGE);
pub(super) const PROFILE_REMOVE: Spec = spec(
    &[value("--home"), switch("--purge"), switch("--dry-run")],
    PROFILE_REMOVE_USAGE,
);
pub(super) const DISABLE: Spec = spec(
    &[
        value("--home"),
        switch("--purge-profiles"),
        switch("--dry-run"),
    ],
    DISABLE_USAGE,
);

const DRY_NOTE: &str = "  dry run: nothing was written";

fn args_with(home: Option<&str>, dry_run: bool, mut args: Value) -> Value {
    if let Some(home) = home {
        args["home"] = json!(home);
    }
    if dry_run {
        args["dryRun"] = json!(true);
    }
    args
}

fn call(command: &str, args: Value) -> Result<Value, CtlError> {
    crate::plus::dispatch(command, args).map_err(|e| CtlError::failed("context_invalid", e))
}

fn text<'a>(data: &'a Value, key: &str) -> &'a str {
    data[key].as_str().unwrap_or("")
}

/// Python's `repr` of a string, which mcpm puts in its messages.
fn py_repr(raw: &str) -> String {
    let quote = if raw.contains('\'') && !raw.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in raw.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn report_lines(data: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    for (key, mark) in [("actions", "✓"), ("warnings", "!")] {
        for line in data[key].as_array().into_iter().flatten() {
            lines.push(format!("  {mark} {}", line.as_str().unwrap_or("")));
        }
    }
    lines
}

fn zshrc_lines(zshrc: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    for (key, mark) in [("actions", ""), ("warnings", "! ")] {
        for line in zshrc[key].as_array().into_iter().flatten() {
            lines.push(format!("  {mark}{}", line.as_str().unwrap_or("")));
        }
    }
    lines
}

fn finish(data: Value, mut lines: Vec<String>) -> Output {
    if data["dryRun"] == json!(true) {
        lines.push(DRY_NOTE.to_string());
    }
    Output::new(data, lines.join("\n"))
}

fn selection_text(value: &Value, count_lists: bool) -> String {
    match value {
        Value::Array(items) if count_lists => format!("{} selected", items.len()),
        Value::Array(items) => items
            .iter()
            .map(|v| v.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(","),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn org_text(profile: &Value) -> String {
    format!(
        "org={}({})",
        if profile["org"] == json!(true) {
            "on"
        } else {
            "off"
        },
        text(profile, "orgMode")
    )
}

fn scope_text(layer: &Value) -> String {
    let globs: Vec<&str> = layer["globs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if globs.is_empty() {
        "always-on".to_string()
    } else {
        globs.join(", ")
    }
}

pub fn init(rest: &[String]) -> Result<Output, CtlError> {
    let flags = INIT.parse(rest)?;
    no_operands(&flags, INIT_USAGE)?;
    let dry = flags.on("--dry-run");
    let mut request = json!({});
    if flags.on("--rewrite-zshrc") {
        request["rewriteZshrc"] = json!(true);
    }
    let data = call(
        "plus.context.init",
        args_with(flags.one("--home"), dry, request),
    )?;
    let mut lines = Vec::new();
    let personal = &data["personal"];
    lines.push(match (personal["created"] == json!(true), dry) {
        (true, true) => format!(
            "  would scaffold personal layer: {}",
            text(personal, "path")
        ),
        (true, false) => format!("  ✓ scaffolded personal layer: {}", text(personal, "path")),
        (false, _) => "  personal layer already scaffolded".to_string(),
    });
    let migration = &data["migration"];
    if migration.is_object() {
        let path = text(migration, "path");
        if migration["unparseable"] == json!(true) {
            lines.push(format!("  ! {path} unparseable — skipping migration"));
        } else {
            let migratable = migration["migratable"].as_u64().unwrap_or(0);
            lines.push(String::new());
            lines.push(format!(
                "  ! {path} is not read by Claude Code at user level (unsupported)."
            ));
            lines.push(format!(
                "      {} allow entr(y/ies) found; {migratable} migratable.",
                migration["found"]
            ));
            let credentials: Vec<&str> = migration["credentials"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if !credentials.is_empty() {
                lines.push(format!(
                    "      {} entr(y/ies) embed credentials — NOT migrating; rotate + re-add manually:",
                    credentials.len()
                ));
                lines.extend(credentials.iter().map(|e| format!("        {e}")));
            }
            if migratable > 0 {
                lines.push(if dry {
                    format!("  would migrate {migratable} entries into ensure_allow")
                } else {
                    format!("  ✓ migrated {migratable} entries into ensure_allow")
                });
            }
        }
    }
    let config = &data["config"];
    if let Some(kept) = config["keptUnreadable"].as_str() {
        lines.push(format!(
            "  ! context.json was unreadable and is replaced by the effective config; kept a copy at {kept}"
        ));
    }
    lines.push(if dry {
        "  would save context.json".to_string()
    } else {
        "  ✓ saved context.json".to_string()
    });
    if data["zshrc"].is_object() {
        lines.push(String::new());
        lines.extend(zshrc_lines(&data["zshrc"]));
    }
    lines.push(String::new());
    lines.push("Next steps:".to_string());
    for (n, step) in data["nextSteps"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        lines.push(format!(
            "  {}. {:<33}{}",
            n + 1,
            text(step, "label"),
            text(step, "command")
        ));
    }
    Ok(finish(data, lines))
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    no_operands(&flags, STATUS_USAGE)?;
    let data = call(
        "plus.context.status",
        args_with(flags.one("--home"), false, json!({})),
    )?;
    let layers: Vec<String> = data["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|layer| {
            let globs: Vec<&str> = layer["globs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if globs.is_empty() {
                text(layer, "name").to_string()
            } else {
                format!("{} [{}]", text(layer, "name"), globs.join(","))
            }
        })
        .collect();
    let mut rows: Vec<(String, String)> = vec![(
        "layers".into(),
        if layers.is_empty() {
            "none — run `toolportctl context init`".into()
        } else {
            layers.join(", ")
        },
    )];
    let profiles = data["profiles"].as_array().map_or(&[][..], Vec::as_slice);
    if profiles.is_empty() {
        rows.push(("profiles".into(), "none".into()));
    }
    for profile in profiles {
        let generated = if profile["generated"] == json!(true) {
            ""
        } else {
            " not generated"
        };
        rows.push((
            format!("profile {}", text(profile, "name")),
            format!(
                "{} rules={} servers={}{generated}",
                org_text(profile),
                selection_text(&profile["rules"], false),
                selection_text(&profile["servers"], true),
            ),
        ));
    }
    let dupes: Vec<&str> = data["legacyDupes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    rows.push((
        "legacy MCP dupes".into(),
        if dupes.is_empty() {
            "none".into()
        } else {
            dupes.join(", ")
        },
    ));
    let shims = &data["shims"];
    rows.push((
        "shims".into(),
        if shims["exists"] == json!(true) {
            text(shims, "path").to_string()
        } else {
            "none".into()
        },
    ));
    let zshrc = &data["zshrc"];
    let pointing: Vec<String> = zshrc["legacyLines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| format!("line {} {}", p["line"], text(p, "file")))
        .collect();
    if !pointing.is_empty() {
        rows.push((
            "zshrc".into(),
            format!(
                "{} line(s) still point into mcpm's config directory: {} — `toolportctl context sync --rewrite-zshrc --dry-run` previews the new paths",
                pointing.len(),
                pointing.join(", ")
            ),
        ));
    }
    for alias in zshrc["deadAliases"].as_array().into_iter().flatten() {
        rows.push((
            "mcpm alias".into(),
            format!(
                "{} ({}:{}) runs `{}`; remove it when mcpm is gone",
                text(alias, "name"),
                text(alias, "file"),
                alias["line"],
                text(alias, "command")
            ),
        ));
    }
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let human = rows
        .iter()
        .map(|(label, value)| format!("{label:<width$}  {value}").trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Output::new(data, human))
}

pub fn client_group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(CLIENT_GROUP_USAGE))
}

fn layer_args(flags: &super::flags::Flags, name: &str) -> Result<Value, CtlError> {
    if name.trim().is_empty() {
        return Err(CtlError::usage("client name must not be empty"));
    }
    let glob = flags.one("--glob").filter(|g| !g.is_empty());
    layers::check_client_rule(name, glob).map_err(CtlError::usage)?;
    let mut args = json!({"name": name});
    if let Some(glob) = glob {
        args["glob"] = json!(glob);
    }
    for (flag, key, allowed) in [
        ("--scope", "scope", &["global", "glob", "folder"][..]),
        ("--delivery", "delivery", &["import", "copy"][..]),
    ] {
        if let Some(value) = flags.one(flag) {
            if !allowed.contains(&value) {
                return Err(CtlError::usage(format!(
                    "{flag} must be one of {}, not {value}",
                    allowed.join(", ")
                )));
            }
            args[key] = json!(value);
        }
    }
    for (flag, key) in [("--folder", "folders"), ("--import", "imports")] {
        let items = flags.all(flag);
        if !items.is_empty() {
            args[key] = json!(items);
        }
    }
    Ok(args)
}

fn plan_lines(data: &Value, skip_first: bool) -> Vec<String> {
    let dry = data["dryRun"] == json!(true);
    let mut lines = Vec::new();
    let steps = data["plan"]["steps"].as_array().map_or(&[][..], Vec::as_slice);
    for step in steps.iter().skip(usize::from(skip_first)) {
        let path = step["path"].as_str().unwrap_or("");
        let detail = step["detail"].as_str().unwrap_or("");
        lines.push(if dry {
            format!("  would {detail}: {path}")
        } else {
            format!("  ✓ {detail}: {path}")
        });
    }
    for warning in data["plan"]["warnings"].as_array().into_iter().flatten() {
        lines.push(format!("  ! {}", warning.as_str().unwrap_or("")));
    }
    lines
}

pub fn client_add(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CLIENT_ADD.parse(rest)?;
    let name = operand(&flags, "client name", CLIENT_ADD_USAGE)?;
    let args = layer_args(&flags, name)?;
    let dry = flags.on("--dry-run");
    let data = call(
        "plus.context.clientAdd",
        args_with(flags.one("--home"), dry, args),
    )?;
    let path = text(&data, "path");
    let mut lines = match (data["created"] == json!(true), dry) {
        (true, true) => vec![format!("  would scaffold {path}")],
        (true, false) => vec![
            format!("  ✓ scaffolded {path}"),
            "  fill in the body, then:  toolportctl skills sync".to_string(),
        ],
        (false, _) => vec![format!("  client layer {} already exists", py_repr(name))],
    };
    lines.extend(plan_lines(&data, true));
    Ok(finish(data, lines))
}

pub fn client_edit(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CLIENT_EDIT.parse(rest)?;
    let name = operand(&flags, "client name", CLIENT_EDIT_USAGE)?;
    let args = layer_args(&flags, name)?;
    let dry = flags.on("--dry-run");
    let data = layer_manage::edit(&args_with(flags.one("--home"), dry, args))?;
    let mut lines = vec![if data["changed"] == json!(true) {
        format!("  {} {}", if dry { "would change" } else { "✓ changed" }, text(&data, "path"))
    } else {
        format!("  nothing to change in {}", text(&data, "name"))
    }];
    lines.extend(plan_lines(&data, true));
    Ok(finish(data, lines))
}

pub fn client_rm(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CLIENT_RM.parse(rest)?;
    let name = operand(&flags, "client name", CLIENT_RM_USAGE)?;
    let dry = flags.on("--dry-run");
    let data = layer_manage::rm(&args_with(flags.one("--home"), dry, json!({"name": name})))?;
    let mut lines = plan_lines(&data, false);
    if !dry {
        lines.push(format!("  undo: {}", data["result"]["undo"].as_str().unwrap_or("")));
    }
    Ok(finish(data, lines))
}

pub fn client_list(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CLIENT_LIST.parse(rest)?;
    no_operands(&flags, CLIENT_LIST_USAGE)?;
    let data = call(
        "plus.context.clientList",
        args_with(flags.one("--home"), false, json!({})),
    )?;
    let mut lines: Vec<String> = data["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|layer| format!("  {}  {}{}", text(layer, "name"), scope_text(layer), delivery_text(layer)))
        .collect();
    if lines.is_empty() {
        lines.push("  no layers — run `toolportctl context init`".to_string());
    }
    Ok(Output::new(data, lines.join("\n")))
}

fn delivery_text(layer: &Value) -> String {
    let mut parts = Vec::new();
    if layer["scope"] != json!("glob") {
        let folders = layer["folders"].as_array().map_or(0, Vec::len);
        parts.push(match layer["scope"].as_str() {
            Some("folder") => format!("folder scope, {folders} folder(s)"),
            other => format!("{} scope", other.unwrap_or("")),
        });
    }
    let imports = layer["imports"].as_array().map_or(0, Vec::len);
    if imports > 0 {
        parts.push(format!("{imports} import(s) by {}", text(layer, "delivery")));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("  [{}]", parts.join(", "))
    }
}

pub fn compose(rest: &[String]) -> Result<Output, CtlError> {
    let flags = COMPOSE.parse(rest)?;
    no_operands(&flags, COMPOSE_USAGE)?;
    let cwd = match flags.one("--cwd") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::env::current_dir().unwrap_or_default(),
    };
    let data = crate::plus::context::compose::compose_here(&cwd).map_err(|e| {
        if e.kind == ErrorKind::Usage {
            CtlError::usage(format!("--cwd {}", e.message))
        } else {
            e
        }
    })?;
    let mut human = String::new();
    for part in data["parts"].as_array().into_iter().flatten() {
        let lazy = if part["lazy"] == json!(true) { "  (when a matching file is read)" } else { "" };
        let layers: Vec<&str> = part["layers"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let via = if part["via"].as_array().is_some_and(|v| !v.is_empty()) { "  (imported)" } else { "" };
        let carried = if layers.is_empty() { String::new() } else { format!("  [layers: {}]", layers.join(", ")) };
        human.push_str(&format!(
            "{:>7} tokens  {}  {}{via}{lazy}{carried}\n",
            part["tokens"]["value"], text(&part["origin"], "kind"), text(part, "path"),
        ));
    }
    if human.is_empty() {
        human.push_str("nothing loads here\n");
    }
    human.push_str(&format!("{:>7} tokens in total (estimate)\n", data["total"]["value"]));
    Ok(Output::new(data, human))
}

pub fn profile_group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(PROFILE_GROUP_USAGE))
}

pub fn profile_add(rest: &[String]) -> Result<Output, CtlError> {
    let flags = PROFILE_ADD.parse(rest)?;
    let name = operand(&flags, "profile name", PROFILE_ADD_USAGE)?;
    manage::check_profile_name(name).map_err(CtlError::usage)?;
    let org_mode = flags.one("--org-mode").unwrap_or("import");
    if !["import", "copy"].contains(&org_mode) {
        return Err(CtlError::usage(format!(
            "--org-mode must be import or copy, not {org_mode}"
        )));
    }
    let dry = flags.on("--dry-run");
    let args = json!({
        "name": name,
        "org": !flags.on("--no-org"),
        "orgMode": org_mode,
        "rules": flags.one("--rules").unwrap_or("inherit"),
        "servers": flags.one("--servers").unwrap_or("inherit"),
        "commands": !flags.on("--no-commands"),
        "skills": !flags.on("--no-skills"),
    });
    let data = call(
        "plus.context.profileAdd",
        args_with(flags.one("--home"), dry, args),
    )?;
    let lines = report_lines(&data);
    Ok(finish(data, lines))
}

pub fn profile_list(rest: &[String]) -> Result<Output, CtlError> {
    let flags = PROFILE_LIST.parse(rest)?;
    no_operands(&flags, PROFILE_LIST_USAGE)?;
    let data = call(
        "plus.context.profileList",
        args_with(flags.one("--home"), false, json!({})),
    )?;
    let mut lines: Vec<String> = data["profiles"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|profile| {
            format!(
                "  {}  {}  rules={}  servers={}",
                text(profile, "shim"),
                org_text(profile),
                selection_text(&profile["rules"], false),
                selection_text(&profile["servers"], false),
            )
        })
        .collect();
    if lines.is_empty() {
        lines.push("  no profiles — `toolportctl context profile add <name>`".to_string());
    }
    Ok(Output::new(data, lines.join("\n")))
}

pub fn profile_remove(rest: &[String]) -> Result<Output, CtlError> {
    let flags = PROFILE_REMOVE.parse(rest)?;
    let name = operand(&flags, "profile name", PROFILE_REMOVE_USAGE)?;
    let purge = flags.on("--purge");
    if purge {
        manage::check_profile_dir_name(name).map_err(CtlError::usage)?;
    }
    let dry = flags.on("--dry-run");
    let args = json!({"name": name, "purge": purge});
    let data = call(
        "plus.context.profileRemove",
        args_with(flags.one("--home"), dry, args),
    )?;
    let mut lines = Vec::new();
    if data["inConfig"] != json!(true) {
        lines.push(format!("  ! no profile {} in config", py_repr(name)));
    }
    lines.extend(report_lines(&data));
    Ok(finish(data, lines))
}

pub fn disable(rest: &[String]) -> Result<Output, CtlError> {
    let flags = DISABLE.parse(rest)?;
    no_operands(&flags, DISABLE_USAGE)?;
    let dry = flags.on("--dry-run");
    let args = json!({"purgeProfiles": flags.on("--purge-profiles")});
    let data = call(
        "plus.context.disable",
        args_with(flags.one("--home"), dry, args),
    )?;
    let mut lines = report_lines(&data);
    if lines.is_empty() {
        lines.push("  nothing to remove".to_string());
    }
    Ok(finish(data, lines))
}

#[cfg(test)]
#[path = "context_manage_tests.rs"]
mod tests;
