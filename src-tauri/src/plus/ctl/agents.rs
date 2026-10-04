//! `toolportctl agents add|ls|lint|audit|diff|status|clean|uninstall|sync`: renderers over the
//! `plus.agents.*` handlers. Texts follow mcpm's with `toolportctl` in the hints (D-042). Scope is
//! user level by default like `skills sync`; `--project` works inside the repository the way
//! mcpm does. `--path` is the repository root, `--repo` its alias.

use super::flags::{switch, value, Spec};
use super::output::{table, table_min, wrap_cell, CtlError, Output};
use super::skills::apply_home;
use super::skills_repo::{findings_text, no_operands, operand, spec, str_of, strings, with_path, PATH};
use super::skills_state::{global_mode, shown};
use serde_json::{json, Value};

const ADD_USAGE: &str = "usage: agents add <name> [--path <dir>] [--dry-run]";
const LS_USAGE: &str = "usage: agents ls [--path <dir>]";
const LINT_USAGE: &str = "usage: agents lint [--path <dir>]";
const AUDIT_USAGE: &str = "usage: agents audit [--path <dir>]";
const DIFF_USAGE: &str = "usage: agents diff [--path <dir>]";
const STATUS_USAGE: &str = "usage: agents status [--path <dir>] [--home <dir>] [--strict]";
const CLEAN_USAGE: &str = "usage: agents clean [--path <dir>] [--home <dir>] [--client <key>] \
     [--project] [--dry-run]";
const UNINSTALL_USAGE: &str =
    "usage: agents uninstall <name> [--path <dir>] [--home <dir>] [--project] [--dry-run]";
const SYNC_USAGE: &str = "usage: agents sync [--path <dir>] [--home <dir>] [--client <key>]... \
     [--project] [--dry-run]";
const USAGE: &str = "usage: agents add|ls|lint|audit|diff|status|clean|uninstall|sync \
     (add: <name> [--path <dir>] [--dry-run]; ls|lint|audit|diff: [--path <dir>]; \
     status: [--path <dir>] [--home <dir>] [--strict]; \
     clean: [--path <dir>] [--home <dir>] [--client <key>] [--project] [--dry-run]; \
     uninstall: <name> [--path <dir>] [--home <dir>] [--project] [--dry-run]; \
     sync: [--path <dir>] [--home <dir>] [--client <key>]... [--project] [--dry-run])";

pub(super) const ADD: Spec = spec(&[PATH, switch("--dry-run")], ADD_USAGE);
pub(super) const LS: Spec = spec(&[PATH], LS_USAGE);
pub(super) const LINT: Spec = spec(&[PATH], LINT_USAGE);
pub(super) const AUDIT: Spec = spec(&[PATH], AUDIT_USAGE);
pub(super) const DIFF: Spec = spec(&[PATH], DIFF_USAGE);
pub(super) const STATUS: Spec = spec(&[PATH, value("--home"), switch("--strict")], STATUS_USAGE);
pub(super) const CLEAN: Spec = spec(
    &[
        PATH,
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
    ],
    CLEAN_USAGE,
);
pub(super) const UNINSTALL: Spec = spec(
    &[
        PATH,
        value("--home"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
    ],
    UNINSTALL_USAGE,
);
pub(super) const SYNC: Spec = spec(
    &[
        PATH,
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
    ],
    SYNC_USAGE,
);

fn call(command: &str, args: Value) -> Result<Value, CtlError> {
    crate::plus::dispatch(command, args).map_err(|e| CtlError::failed("agents", e))
}

/// The agent files that failed to parse come first, where mcpm's log lines land.
fn with_warnings(data: &Value, body: String) -> String {
    let warnings = strings(data, "discoveryWarnings");
    if warnings.is_empty() {
        body
    } else {
        format!("{}\n{body}", warnings.join("\n"))
    }
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(USAGE))
}

pub fn add(rest: &[String]) -> Result<Output, CtlError> {
    let args = ADD.parse(rest)?;
    let name = operand(&args, "agent name", ADD_USAGE)?;
    let dry_run = args.on("--dry-run");
    let request = with_path(&args, json!({"name": name, "dry_run": dry_run}));
    let data = call("plus.agents.add", request)?;
    let path = str_of(&data, "path");
    let human = if dry_run {
        format!("Would create agent '{name}':\n  {path}")
    } else {
        format!(
            "\nAgent '{name}' created at {path}\n\nEdit the AGENT.md file, then run \
             'toolportctl agents sync' to transpile to all clients."
        )
    };
    Ok(Output::new(data, human))
}

fn cell(text: Option<&str>, fallback: &str) -> String {
    text.filter(|t| !t.is_empty()).unwrap_or(fallback).to_string()
}

const DESCRIPTION_WIDTH: usize = 50;

/// mcpm cuts the description at 50 characters and appends `...`, then lets rich wrap it.
fn cut_description(text: &str) -> String {
    if text.chars().count() > DESCRIPTION_WIDTH {
        text.chars()
            .take(DESCRIPTION_WIDTH)
            .chain("...".chars())
            .collect()
    } else {
        text.to_string()
    }
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let args = LS.parse(rest)?;
    no_operands(&args, LS_USAGE)?;
    let data = call("plus.agents.list", with_path(&args, json!({})))?;
    let agents = data["agents"].as_array().map_or(&[][..], Vec::as_slice);
    let body = if agents.is_empty() {
        "No agents found.".to_string()
    } else {
        let cuts: Vec<String> = agents
            .iter()
            .map(|a| cut_description(str_of(a, "description")))
            .collect();
        let widest = cuts
            .iter()
            .flat_map(|c| c.split('\n'))
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        let rows: Vec<Vec<String>> = agents
            .iter()
            .zip(&cuts)
            .map(|(a, cut)| {
                let tools = strings(a, "tools").join(", ");
                vec![
                    str_of(a, "name").to_string(),
                    cell(a["model"].as_str(), "default"),
                    cell(Some(&tools), "all"),
                    wrap_cell(cut, DESCRIPTION_WIDTH),
                ]
            })
            .collect();
        format!(
            "\nFound {} agent(s) in {}\n\n{}",
            agents.len(),
            str_of(&data, "repo"),
            table_min(
                &["Name", "Model", "Tools", "Description"],
                &rows,
                &[0, 0, 0, widest.min(DESCRIPTION_WIDTH)]
            )
        )
    };
    let human = with_warnings(&data, body);
    Ok(Output::new(data, human))
}

pub fn lint(rest: &[String]) -> Result<Output, CtlError> {
    let args = LINT.parse(rest)?;
    no_operands(&args, LINT_USAGE)?;
    let data = call("plus.agents.lint", with_path(&args, json!({})))?;
    let count = |key: &str| data[key].as_u64().unwrap_or(0);
    let body = if data["agentCount"] == json!(0) {
        "No agents found to lint.".to_string()
    } else if data["messages"].as_array().is_none_or(Vec::is_empty) {
        format!("All {} agent(s) passed lint checks.", data["agentCount"])
    } else {
        let mut text = String::new();
        for m in data["messages"].as_array().into_iter().flatten() {
            let icon = match str_of(m, "level") {
                "error" => "E",
                "warning" => "W",
                _ => "I",
            };
            text.push_str(&format!(
                "  {icon} {}: {}\n",
                str_of(m, "name"),
                str_of(m, "message")
            ));
        }
        let parts: Vec<String> = [
            ("errors", "error(s)"),
            ("warnings", "warning(s)"),
            ("infos", "info(s)"),
        ]
        .iter()
        .filter(|(key, _)| count(key) > 0)
        .map(|(key, label)| format!("{} {label}", count(key)))
        .collect();
        text.push_str(&format!("\n  {}", parts.join(", ")));
        text
    };
    let mut out = Output::new(data.clone(), with_warnings(&data, body));
    out.failed = count("errors") > 0;
    Ok(out)
}

pub fn audit(rest: &[String]) -> Result<Output, CtlError> {
    let args = AUDIT.parse(rest)?;
    no_operands(&args, AUDIT_USAGE)?;
    let data = call("plus.agents.audit", with_path(&args, json!({})))?;
    let body = if data["agentCount"] == json!(0) {
        "No agents found to audit.".to_string()
    } else if data["clean"] == json!(true) {
        format!("All {} agent(s) passed security audit.", data["agentCount"])
    } else {
        findings_text(&data, "agent")
    };
    let mut out = Output::new(data.clone(), with_warnings(&data, body));
    out.failed = data["high"].as_u64().unwrap_or(0) > 0;
    Ok(out)
}

pub fn diff(rest: &[String]) -> Result<Output, CtlError> {
    let args = DIFF.parse(rest)?;
    no_operands(&args, DIFF_USAGE)?;
    let data = call("plus.agents.diff", with_path(&args, json!({})))?;
    let (new, modified, removed) = (
        strings(&data, "new"),
        strings(&data, "modified"),
        strings(&data, "removed"),
    );
    let body = if data["noLockfile"] == json!(true) {
        let mut text = "No lockfile found. All agents are new.\n".to_string();
        for name in &new {
            text.push_str(&format!("\n  + {name}"));
        }
        text
    } else if data["clean"] == json!(true) {
        "No changes since last sync.".to_string()
    } else {
        let mut lines = vec![String::new()];
        lines.extend(new.iter().map(|n| format!("  + {n} (new)")));
        lines.extend(modified.iter().map(|n| format!("  ~ {n} (modified)")));
        lines.extend(removed.iter().map(|n| format!("  - {n} (removed)")));
        if data["unchanged"].as_u64().unwrap_or(0) > 0 {
            lines.push(format!("  {} unchanged", data["unchanged"]));
        }
        lines.join("\n")
    };
    let human = with_warnings(&data, body);
    Ok(Output::new(data, human))
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    let args = STATUS.parse(rest)?;
    no_operands(&args, STATUS_USAGE)?;
    apply_home(args.one("--home"));
    let data = call("plus.agents.status", with_path(&args, json!({})))?;
    let missing_lock = data["lockfilePresent"] != json!(true);
    let drift = data["drift"] == json!(true);
    let human = if missing_lock {
        "No lockfile found. Run 'toolportctl agents sync' first.".to_string()
    } else if data["lockedCount"] == json!(0) {
        "No agents in lockfile.".to_string()
    } else {
        let rows: Vec<Vec<String>> = data["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                vec![
                    str_of(row, "name").to_string(),
                    str_of(row, "client").to_string(),
                    if row["present"] == json!(true) {
                        "ok"
                    } else {
                        "missing"
                    }
                    .to_string(),
                ]
            })
            .collect();
        let verdict = if drift {
            "Drift detected. Run 'toolportctl agents sync' to update."
        } else {
            "All output files in sync."
        };
        format!(
            "{}\n\n{verdict}",
            table(&["Agent", "Client", "Status"], &rows)
        )
    };
    let mut out = Output::new(data, human);
    out.failed = args.on("--strict") && (missing_lock || drift);
    Ok(out)
}

pub fn clean(rest: &[String]) -> Result<Output, CtlError> {
    let args = CLEAN.parse(rest)?;
    no_operands(&args, CLEAN_USAGE)?;
    apply_home(args.one("--home"));
    let dry_run = args.on("--dry-run");
    let mut request = with_path(
        &args,
        json!({"global_mode": global_mode(&args)?, "dry_run": dry_run}),
    );
    if let Some(client) = args.one("--client") {
        request["client"] = json!(client);
    }
    let data = call("plus.agents.clean", request)?;
    let (removed, cleaned) = if dry_run {
        ("Would remove", "Would clean")
    } else {
        ("Removed", "Cleaned")
    };
    let base = str_of(&data, "lockDir");
    let mut lines = Vec::new();
    if data["lockfilePresent"] != json!(true) {
        lines.push("No lockfile found. Nothing to clean.".to_string());
    } else if data["managed"].as_array().is_none_or(Vec::is_empty) {
        lines.push("No managed agents in lockfile.".to_string());
    } else {
        let paths = strings(&data, "removed");
        lines.extend(
            paths
                .iter()
                .map(|p| format!("  {removed} {}", shown(p, base))),
        );
        for skipped in data["skipped"].as_array().into_iter().flatten() {
            lines.push(format!(
                "  Error cleaning {}: {}",
                str_of(skipped, "client"),
                str_of(skipped, "error")
            ));
        }
        if paths.is_empty() {
            lines.push("No managed files found to clean.".to_string());
        } else {
            lines.push(format!("\n{cleaned} {} file(s).", paths.len()));
        }
    }
    for name in strings(&data, "ignored") {
        lines.push(format!("Ignored lock entry with invalid name: {name}"));
    }
    Ok(Output::new(data, lines.join("\n")))
}

pub fn uninstall(rest: &[String]) -> Result<Output, CtlError> {
    let args = UNINSTALL.parse(rest)?;
    let name = operand(&args, "agent name", UNINSTALL_USAGE)?;
    apply_home(args.one("--home"));
    let dry_run = args.on("--dry-run");
    let request = with_path(
        &args,
        json!({"name": name, "global_mode": global_mode(&args)?, "dry_run": dry_run}),
    );
    let data = call("plus.agents.uninstall", request)?;
    let (removed, done, verb) = if dry_run {
        ("Would remove", "Would uninstall", "remove")
    } else {
        ("Removed", "Uninstalled", "removed")
    };
    let repo = str_of(&data, "repo");
    let outputs = strings(&data, "outputs");
    let mut lines: Vec<String> = outputs
        .iter()
        .map(|p| format!("  {removed} {}", shown(p, repo)))
        .collect();
    lines.push(format!(
        "  {removed} {}",
        shown(str_of(&data, "sourcePath"), repo)
    ));
    lines.push(format!(
        "\n{done} '{name}' and {verb} {} output file(s).",
        outputs.len()
    ));
    Ok(Output::new(data, lines.join("\n")))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let args = SYNC.parse(rest)?;
    no_operands(&args, SYNC_USAGE)?;
    apply_home(args.one("--home"));
    let dry_run = args.on("--dry-run");
    let mut request = with_path(
        &args,
        json!({"global_mode": global_mode(&args)?, "dry_run": dry_run}),
    );
    let clients = args.all("--client");
    if !clients.is_empty() {
        request["client_keys"] = json!(clients);
    }
    let data = call("plus.agents.sync", request)?;
    let human = if data["foundCount"] == json!(0) {
        "No agents found in repository.".to_string()
    } else {
        let mut text = String::new();
        if dry_run {
            text.push_str("Dry run -- no files will be written.\n\n");
        }
        if str_of(&data, "scope") == "global" {
            text.push_str("Global mode -- writing to user-level paths.\n\n");
        }
        let rows: Vec<Vec<String>> = data["agents"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                let model = if a["found"] == json!(true) {
                    cell(a["model"].as_str(), "default")
                } else {
                    "--".to_string()
                };
                vec![
                    str_of(a, "name").to_string(),
                    model,
                    cell(Some(&strings(a, "clientsSynced").join(", ")), "none"),
                    cell(Some(&strings(a, "warnings").join("; ")), "--"),
                ]
            })
            .collect();
        text.push_str(&format!(
            "\n{}Synced {} agent(s) to {} client(s).\n\n{}",
            if dry_run { "(dry run) " } else { "" },
            data["agentCount"],
            data["clientCount"],
            table(&["Agent", "Model", "Clients synced", "Warnings"], &rows)
        ));
        text
    };
    Ok(Output::new(data.clone(), with_warnings(&data, human)))
}
