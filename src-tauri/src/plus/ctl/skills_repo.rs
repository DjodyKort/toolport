//! `toolportctl skills init|add|audit|bundle|unbundle`: repository management over the
//! `plus.skills.*` handlers. `--path` is the repository root; `--repo` is accepted as its alias
//! to match `skills ls|lint|diff|sync`.

use super::flags::{switch, value, Flag, Flags, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use serde_json::{json, Value};

const INIT_USAGE: &str = "usage: skills init [--path <dir>] [--name <name>] [--dry-run]";
const ADD_USAGE: &str =
    "usage: skills add <name> [--type skill|rule] [--path <dir>] [--with-progressive] [--dry-run]";
const AUDIT_USAGE: &str = "usage: skills audit [--path <dir>]";
const BUNDLE_USAGE: &str =
    "usage: skills bundle [--output <zip>] [--path <dir>] [--skills <a,b>] [--dry-run]";
const UNBUNDLE_USAGE: &str = "usage: skills unbundle <bundle.zip> [--path <dir>] [--dry-run]";

pub(super) const PATH: Flag = value("--path").alias(&["--repo"]);

pub(super) const fn spec(flags: &'static [Flag], usage: &'static str) -> Spec {
    Spec {
        flags,
        inline: Inline::Strict,
        unknown: Unknown::ArgumentUsage(usage),
        operands: Operands::Collect,
    }
}

const INIT: Spec = spec(&[PATH, value("--name"), switch("--dry-run")], INIT_USAGE);
const ADD: Spec = spec(
    &[
        PATH,
        value("--type"),
        switch("--with-progressive"),
        switch("--dry-run"),
    ],
    ADD_USAGE,
);
const AUDIT: Spec = spec(&[PATH], AUDIT_USAGE);
const BUNDLE: Spec = spec(
    &[PATH, value("--output"), value("--skills"), switch("--dry-run")],
    BUNDLE_USAGE,
);
const UNBUNDLE: Spec = spec(&[PATH, switch("--dry-run")], UNBUNDLE_USAGE);

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
    crate::plus::dispatch(command, args).map_err(|e| CtlError::new("skills", e))
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

pub fn init(rest: &[String]) -> Result<Output, CtlError> {
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

pub fn add(rest: &[String]) -> Result<Output, CtlError> {
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

pub fn audit(rest: &[String]) -> Result<Output, CtlError> {
    let args = AUDIT.parse(rest)?;
    no_operands(&args, AUDIT_USAGE)?;
    let data = call("plus.skills.audit", with_path(&args, json!({})))?;
    let mut human = String::new();
    if data["skillCount"] == json!(0) {
        human.push_str("No skills found to audit.");
    } else if data["clean"] == json!(true) {
        human.push_str(&format!(
            "All {} skill(s) passed security audit.",
            data["skillCount"]
        ));
    } else {
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
            human.push_str(&format!(
                "  {label} {}{line}: {}\n",
                str_of(f, "skill"),
                str_of(f, "message")
            ));
        }
        let parts: Vec<String> = ["high", "medium", "low"]
            .iter()
            .filter(|k| data[**k].as_u64().unwrap_or(0) > 0)
            .map(|k| format!("{} {k}", data[*k]))
            .collect();
        human.push_str(&format!("\n  {}", parts.join(", ")));
    }
    let mut out = Output::new(data.clone(), human);
    out.failed = data["high"].as_u64().unwrap_or(0) > 0;
    Ok(out)
}

pub fn bundle(rest: &[String]) -> Result<Output, CtlError> {
    let args = BUNDLE.parse(rest)?;
    no_operands(&args, BUNDLE_USAGE)?;
    let mut request = with_path(&args, json!({"dry_run": args.on("--dry-run")}));
    if let Some(output) = args.one("--output") {
        request["output"] = json!(output);
    }
    let skills: Vec<String> = args
        .all("--skills")
        .iter()
        .flat_map(|s| s.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if !skills.is_empty() {
        request["skills"] = json!(skills);
    }
    let data = call("plus.skills.bundle", request)?;
    let output = str_of(&data, "output");
    let human = if args.on("--dry-run") {
        let mut text = format!(
            "Would create bundle {output} with {} file(s):\n",
            data["fileCount"]
        );
        for row in data["skills"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                "  {:<6} {} ({} file(s))\n",
                str_of(row, "type"),
                str_of(row, "name"),
                row["files"]
            ));
        }
        text.trim_end().to_string()
    } else {
        let kb = data["bundleBytes"].as_u64().unwrap_or(0) as f64 / 1024.0;
        format!("Bundle created: {output} ({kb:.1} KB)")
    };
    Ok(Output::new(data, human))
}

pub fn unbundle(rest: &[String]) -> Result<Output, CtlError> {
    let args = UNBUNDLE.parse(rest)?;
    let bundle = operand(&args, "bundle path", UNBUNDLE_USAGE)?;
    let request = with_path(
        &args,
        json!({
            "bundle_path": bundle,
            "dry_run": args.on("--dry-run"),
        }),
    );
    let data = call("plus.skills.unbundle", request)?;
    let names = strings(&data, "names");
    let mut human = if args.on("--dry-run") {
        let mut text = format!(
            "Would extract {} skill(s) into {}: {}\n",
            names.len(),
            str_of(&data, "target"),
            names.join(", ")
        );
        for file in strings(&data, "files") {
            let overwrite = strings(&data, "overwritten").contains(&file);
            text.push_str(&format!(
                "  {file}{}\n",
                if overwrite { " (overwrites)" } else { "" }
            ));
        }
        text
    } else {
        format!(
            "Extracted {} skill(s): {}\nRun 'toolportctl skills sync' to transpile to clients.\n",
            names.len(),
            names.join(", ")
        )
    };
    for skipped in strings(&data, "skipped") {
        human.push_str(&format!("skipped unsafe path: {skipped}\n"));
    }
    Ok(Output::new(data, human.trim_end().to_string()))
}

#[cfg(test)]
#[path = "skills_repo_tests.rs"]
mod tests;
