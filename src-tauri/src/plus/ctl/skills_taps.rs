//! `toolportctl skills tap add|ls|remove|update`, `skills search` and `skills install`:
//! renderers over the `plus.skills.tap*`, `search` and `install` handlers. mcpm has these as
//! `mcpm sync tap|search|install`; the texts follow it with `toolportctl` in the hints (D-042).
//! Errors go to stderr with exit 1 where mcpm prints them and exits 0, and `--dry-run` is new.

use super::flags::{switch, value, Spec};
use super::output::{table, table_min, wrap_cell, CtlError, Output};
use super::skills_repo::{no_operands, operand, spec, str_of, strings, PATH};
use serde_json::{json, Value};

const TAP_ADD_USAGE: &str = "usage: skills tap add <user/repo|url> [--name <alias>] [--dry-run]";
const TAP_LS_USAGE: &str = "usage: skills tap ls";
const TAP_REMOVE_USAGE: &str = "usage: skills tap remove <name> [--dry-run]";
const TAP_UPDATE_USAGE: &str = "usage: skills tap update [<name>] [--dry-run]";
const SEARCH_USAGE: &str = "usage: skills search <query>";
const INSTALL_USAGE: &str =
    "usage: skills install <@user/repo[/skill][@version]> [--path <dir>] [--no-audit] [--dry-run]";
const TAP_USAGE: &str = "usage: skills tap add|ls|remove|update \
     (add: <user/repo|url> [--name <alias>] [--dry-run]; ls; remove: <name> [--dry-run]; \
     update: [<name>] [--dry-run])";

pub(super) const TAP_ADD: Spec = spec(&[value("--name"), switch("--dry-run")], TAP_ADD_USAGE);
pub(super) const TAP_LS: Spec = spec(&[], TAP_LS_USAGE);
pub(super) const TAP_REMOVE: Spec = spec(&[switch("--dry-run")], TAP_REMOVE_USAGE);
pub(super) const TAP_UPDATE: Spec = spec(&[switch("--dry-run")], TAP_UPDATE_USAGE);
pub(super) const SEARCH: Spec = spec(&[], SEARCH_USAGE);
pub(super) const INSTALL: Spec = spec(
    &[PATH, switch("--no-audit"), switch("--dry-run")],
    INSTALL_USAGE,
);

const DESCRIPTION_WIDTH: usize = 60;
const NO_TAPS: &str = "No taps registered. Run 'toolportctl skills tap add user/repo' to add one.";

fn call(command: &str, args: Value) -> Result<Value, CtlError> {
    crate::plus::dispatch(command, args).map_err(|e| CtlError::failed("skills", e))
}

/// The lines mcpm logs while it reads a tap come first, where its stderr lands.
fn with_warnings(data: &Value, body: String) -> String {
    let warnings = strings(data, "discoveryWarnings");
    if warnings.is_empty() {
        body
    } else {
        format!("{}\n{body}", warnings.join("\n"))
    }
}

pub fn tap_group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(TAP_USAGE))
}

pub fn tap_add(rest: &[String]) -> Result<Output, CtlError> {
    let args = TAP_ADD.parse(rest)?;
    let repo = operand(&args, "tap source", TAP_ADD_USAGE)?;
    let dry_run = args.on("--dry-run");
    let mut request = json!({"repo": repo, "dry_run": dry_run});
    if let Some(name) = args.one("--name") {
        request["name"] = json!(name);
    }
    let data = call("plus.skills.tapAdd", request)?;
    let name = str_of(&data, "name");
    let human = if dry_run {
        format!(
            "Would clone {repo} into {}\nTap '{name}' would be added.",
            str_of(&data, "path")
        )
    } else {
        format!("Cloning {repo}...\nTap '{name}' added successfully.")
    };
    Ok(Output::new(data, human))
}

pub fn tap_ls(rest: &[String]) -> Result<Output, CtlError> {
    let args = TAP_LS.parse(rest)?;
    no_operands(&args, TAP_LS_USAGE)?;
    let data = call("plus.skills.tapList", json!({}))?;
    let taps = data["taps"].as_array().map_or(&[][..], Vec::as_slice);
    let human = if taps.is_empty() {
        NO_TAPS.to_string()
    } else {
        let rows: Vec<Vec<String>> = taps
            .iter()
            .map(|t| {
                vec![
                    str_of(t, "name").to_string(),
                    str_of(t, "repo").to_string(),
                    str_of(t, "path").to_string(),
                ]
            })
            .collect();
        table(&["Name", "Repository", "Path"], &rows)
    };
    Ok(Output::new(data, human))
}

pub fn tap_remove(rest: &[String]) -> Result<Output, CtlError> {
    let args = TAP_REMOVE.parse(rest)?;
    let name = operand(&args, "tap name", TAP_REMOVE_USAGE)?;
    let dry_run = args.on("--dry-run");
    let data = call(
        "plus.skills.tapRemove",
        json!({"name": name, "dry_run": dry_run}),
    )?;
    let human = if dry_run {
        let clone = if data["hadClone"] == json!(true) {
            format!(" and delete {}", str_of(&data, "path"))
        } else {
            String::new()
        };
        format!("Would remove tap '{name}'{clone}.")
    } else {
        format!("Tap '{name}' removed.")
    };
    Ok(Output::new(data, human))
}

pub fn tap_update(rest: &[String]) -> Result<Output, CtlError> {
    let args = TAP_UPDATE.parse(rest)?;
    let dry_run = args.on("--dry-run");
    let mut request = json!({"dry_run": dry_run});
    match args.operands() {
        [] => {}
        [name] => request["name"] = json!(name),
        [_, extra, ..] => {
            return Err(CtlError::usage(format!(
                "unexpected argument: {extra}\n{TAP_UPDATE_USAGE}"
            )))
        }
    }
    let data = call("plus.skills.tapUpdate", request)?;
    let results = data["results"].as_array().map_or(&[][..], Vec::as_slice);
    let human = if results.is_empty() {
        "No taps registered.".to_string()
    } else {
        results
            .iter()
            .map(|r| {
                let name = str_of(r, "name");
                match (r["ok"] == json!(true), dry_run) {
                    (true, true) => format!("  Would update {name}"),
                    (true, false) => format!("  Updated {name}"),
                    (false, _) => format!("  Failed to update {name}: {}", str_of(r, "error")),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut out = Output::new(data.clone(), human);
    out.failed = data["failed"].as_u64().unwrap_or(0) > 0;
    Ok(out)
}

/// mcpm cuts the description at 60 characters and appends `...`, then lets rich wrap it.
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

pub fn search(rest: &[String]) -> Result<Output, CtlError> {
    let args = SEARCH.parse(rest)?;
    let query = operand(&args, "query", SEARCH_USAGE)?;
    let data = call("plus.skills.search", json!({"query": query}))?;
    let hits = data["results"].as_array().map_or(&[][..], Vec::as_slice);
    let body = if hits.is_empty() {
        let mut text = format!("No skills found matching '{query}'.");
        if data["tapCount"] == json!(0) {
            text.push('\n');
            text.push_str(NO_TAPS);
        }
        text
    } else {
        let cuts: Vec<String> = hits
            .iter()
            .map(|h| cut_description(str_of(h, "description")))
            .collect();
        let widest = cuts
            .iter()
            .flat_map(|c| c.split('\n'))
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        let rows: Vec<Vec<String>> = hits
            .iter()
            .zip(&cuts)
            .map(|(h, cut)| {
                vec![
                    str_of(h, "name").to_string(),
                    str_of(h, "tap").to_string(),
                    wrap_cell(cut, DESCRIPTION_WIDTH),
                ]
            })
            .collect();
        format!(
            "{}\n\n{} result(s). Install with: toolportctl skills install @<repo>/<skill-name>",
            table_min(
                &["Skill", "Tap", "Description"],
                &rows,
                &[0, 0, widest.min(DESCRIPTION_WIDTH)]
            ),
            hits.len()
        )
    };
    Ok(Output::new(data.clone(), with_warnings(&data, body)))
}

fn audit_section(data: &Value) -> String {
    let audit = &data["audit"];
    let findings = audit["findings"].as_array().map_or(&[][..], Vec::as_slice);
    if audit["ran"] != json!(true) || findings.is_empty() {
        return String::new();
    }
    if data["blocked"] == json!(true) {
        let mut text = String::from("\nSecurity audit found high-severity issues:\n\n");
        for f in findings.iter().filter(|f| str_of(f, "severity") == "high") {
            text.push_str(&format!(
                "  HIGH {}: {}\n",
                str_of(f, "skill"),
                str_of(f, "message")
            ));
        }
        text.push_str("\nUse --no-audit to skip security checks.");
        text
    } else {
        format!(
            "\nAudit: {} finding(s) (none high severity)",
            findings.len()
        )
    }
}

pub fn install(rest: &[String]) -> Result<Output, CtlError> {
    let args = INSTALL.parse(rest)?;
    let spec_text = operand(&args, "install spec", INSTALL_USAGE)?;
    let dry_run = args.on("--dry-run");
    let mut request = json!({
        "spec": spec_text,
        "dry_run": dry_run,
        "no_audit": args.on("--no-audit"),
    });
    if let Some(path) = args.one("--path") {
        request["repo_path"] = json!(path);
    }
    let data = call("plus.skills.install", request)?;
    let mut lines = vec![format!("Resolving {spec_text}...")];
    if let Some(version) = data["version"].as_str() {
        lines.push(format!(
            "Version '{version}' is ignored: a tap installs its checked-out head."
        ));
    }
    if data["tapMissing"] == json!(true) {
        lines.push(format!(
            "Tap '{}' is not registered; would clone {} first.\nNothing was written.",
            str_of(&data, "tap"),
            str_of(&data, "cloneUrl")
        ));
        return Ok(Output::new(data, lines.join("\n")));
    }
    if data["tapAdded"] == json!(true) {
        lines.push(format!(
            "Added tap '{}' ({}).",
            str_of(&data, "tap"),
            str_of(&data, "cloneUrl")
        ));
    }
    lines.extend(strings(&data, "discoveryWarnings"));
    lines.push(format!("Found {} skill(s)", data["foundCount"]));
    let audit = audit_section(&data);
    if !audit.is_empty() {
        lines.push(audit);
    }
    let blocked = data["blocked"] == json!(true);
    if !blocked {
        let (install, skip) = if dry_run {
            ("Would install", "Would skip")
        } else {
            ("Installed", "Skipping")
        };
        for row in data["skills"].as_array().into_iter().flatten() {
            let (name, kind) = (str_of(row, "name"), str_of(row, "type"));
            lines.push(if row["status"] == json!("installed") {
                format!("  {install} {name} ({kind})")
            } else {
                format!("  {skip} {name} (already exists)")
            });
        }
        for link in strings(&data, "symlinksSkipped") {
            lines.push(format!("  Skipped symlink {link}"));
        }
        let count = data["installedCount"].as_u64().unwrap_or(0);
        lines.push(match (count, dry_run) {
            (0, false) => "No new skills installed.".to_string(),
            (0, true) => "No new skills would be installed.".to_string(),
            (n, false) => format!(
                "\nInstalled {n} skill(s). Run 'toolportctl skills sync' to transpile to clients."
            ),
            (n, true) => format!(
                "\nWould install {n} skill(s) into {}.",
                str_of(&data, "target")
            ),
        });
    }
    let mut out = Output::new(data, lines.join("\n"));
    out.failed = blocked;
    Ok(out)
}
