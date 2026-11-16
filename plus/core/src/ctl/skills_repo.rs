//! `toolportctl skills init|add|audit`: repository management over the
//! `plus.skills.*` handlers. `--path` is the repository root; `--repo` is accepted as its alias
//! to match `skills ls|lint|diff|sync`.

use super::flags::{switch, value, Flag, Flags, Inline, Spec, Unknown};
use super::output::{CtlError, Output};
use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::skills::enter_paths;

const INIT_USAGE: &str = "usage: skills init [--path <dir>] [--name <name>] [--dry-run]";
const ADD_USAGE: &str =
    "usage: skills add <name> [--type skill|rule] [--path <dir>] [--with-progressive] [--dry-run]";
const AUDIT_USAGE: &str = "usage: skills audit [--path <dir>]";

pub(super) const PATH: Flag = value("--path").alias(&["--repo"]);

pub(super) const fn spec(flags: &'static [Flag], usage: &'static str) -> Spec {
    Spec {
        flags,
        inline: Inline::Strict,
        unknown: Unknown::ArgumentUsage(usage),
        ..Spec::PLAIN
    }
}

pub(super) const INIT: Spec = spec(&[PATH, value("--name"), switch("--dry-run")], INIT_USAGE);
pub(super) const ADD: Spec = spec(
    &[
        PATH,
        value("--type"),
        switch("--with-progressive"),
        switch("--dry-run"),
    ],
    ADD_USAGE,
);
pub(super) const AUDIT: Spec = spec(&[PATH], AUDIT_USAGE);

pub(super) fn no_operands(flags: &Flags, usage: &str) -> Result<(), CtlError> {
    match flags.operands().first() {
        Some(extra) => Err(CtlError::usage(format!(
            "unexpected argument: {extra}\n{usage}"
        ))),
        None => Ok(()),
    }
}

pub(super) fn operand<'a>(flags: &'a Flags, what: &str, usage: &str) -> Result<&'a str, CtlError> {
    match flags.operands() {
        [one] => Ok(one),
        [] => Err(CtlError::usage(format!("missing {what}\n{usage}"))),
        [_, extra, ..] => Err(CtlError::usage(format!(
            "unexpected argument: {extra}\n{usage}"
        ))),
    }
}

pub(super) fn with_path(flags: &Flags, mut args: Value) -> Value {
    if let Some(path) = flags.one("--path") {
        args["repo_path"] = json!(path);
    }
    args
}

pub(super) fn call(command: &str, args: Value) -> Result<Value, CtlError> {
    use crate::skills::{repo_handlers as repo, state};
    let handled = match command {
        "plus.skills.init" => repo::init_handler(args),
        "plus.skills.add" => repo::add_handler(args),
        "plus.skills.audit" => repo::audit_handler(args),
        "plus.skills.clean" => state::clean_handler(args),
        "plus.skills.uninstall" => state::uninstall_handler(args),
        "plus.skills.resolve" => state::resolve_handler(args),
        other => Err(format!("unknown skills operation: {other}")),
    };
    handled.map_err(|e| CtlError::failed("skills", e))
}

pub(super) fn strings(data: &Value, key: &str) -> Vec<String> {
    data[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
}

pub(super) fn str_of<'a>(data: &'a Value, key: &str) -> &'a str {
    data[key].as_str().unwrap_or("")
}

pub fn init(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let args = INIT.parse(rest)?;
    no_operands(&args, INIT_USAGE)?;
    let mut request = with_path(&args, json!({"dry_run": args.on("--dry-run")}));
    if let Some(name) = args.one("--name") {
        request["name"] = json!(name);
    }
    let data = call("plus.skills.init", request)?;
    let repo = str_of(&data, "repo");
    let human = if data["alreadyExists"] == json!(true) {
        format!("Skills repository already exists at {repo}")
    } else {
        let (head, label) = if args.on("--dry-run") {
            ("Would initialize a skills repository at", "Would create")
        } else {
            ("Skills repository initialized at", "Created")
        };
        let mut text = format!("{head} {repo}\n\n{label}:\n");
        for item in strings(&data, "created") {
            text.push_str(&format!("  {item}\n"));
        }
        text.push_str("\nNext: run 'toolportctl skills add <name>' to create your first skill.");
        text
    };
    Ok(Output::new(data, human))
}

pub fn add(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let args = ADD.parse(rest)?;
    let name = operand(&args, "skill name", ADD_USAGE)?;
    let mut request = with_path(
        &args,
        json!({
            "name": name,
            "dry_run": args.on("--dry-run"),
            "with_progressive": args.on("--with-progressive"),
        }),
    );
    if let Some(kind) = args.one("--type") {
        request["skill_type"] = json!(kind);
    }
    let data = call("plus.skills.add", request)?;
    let kind = str_of(&data, "type");
    let mut human = if args.on("--dry-run") {
        let mut text = format!("Would create {kind} '{name}':\n");
        for file in strings(&data, "files") {
            text.push_str(&format!("  {file}\n"));
        }
        text
    } else {
        let label = if kind == "rule" { "Rule" } else { "Skill" };
        format!("{label} '{name}' created at {}\n", str_of(&data, "path"))
    };
    if args.on("--with-progressive") && !args.on("--dry-run") {
        human.push_str(
            "Progressive-disclosure scaffolding added: modules/, reference/, templates/.\n",
        );
    }
    human.push_str(
        "Edit the SKILL.md file, then run 'toolportctl skills sync' to transpile to all clients.",
    );
    Ok(Output::new(data, human))
}

/// The findings and the severity totals of an audit the way mcpm prints them; `name_key` is the
/// data key holding the name of the audited item.
pub(super) fn findings_text(data: &Value, name_key: &str) -> String {
    let mut text = String::new();
    for f in data["findings"].as_array().into_iter().flatten() {
        let label = match f["severity"].as_str().unwrap_or("") {
            "high" => "HIGH",
            "medium" => "MED ",
            _ => "LOW ",
        };
        let line = match f["line"].as_u64().unwrap_or(0) {
            0 => String::new(),
            n => format!(" (line {n})"),
        };
        text.push_str(&format!(
            "  {label} {}{line}: {}\n",
            str_of(f, name_key),
            str_of(f, "message")
        ));
    }
    let parts: Vec<String> = ["high", "medium", "low"]
        .iter()
        .filter(|k| data[**k].as_u64().unwrap_or(0) > 0)
        .map(|k| format!("{} {k}", data[*k]))
        .collect();
    text.push_str(&format!("\n  {}", parts.join(", ")));
    text
}

pub fn audit(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let args = AUDIT.parse(rest)?;
    no_operands(&args, AUDIT_USAGE)?;
    let data = call("plus.skills.audit", with_path(&args, json!({})))?;
    let human = if data["skillCount"] == json!(0) {
        "No skills found to audit.".to_string()
    } else if data["clean"] == json!(true) {
        format!("All {} skill(s) passed security audit.", data["skillCount"])
    } else {
        findings_text(&data, "skill")
    };
    let mut out = Output::new(data.clone(), human);
    out.failed = data["high"].as_u64().unwrap_or(0) > 0;
    Ok(out)
}
