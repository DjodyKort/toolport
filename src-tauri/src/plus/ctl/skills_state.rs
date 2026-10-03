//! `toolportctl skills status|clean|uninstall|resolve`: renderers over the `plus.skills.*`
//! handlers. Texts follow mcpm's with `toolportctl` in the hints (D-042). Scope is user level by
//! default like `skills sync`; `--project` works inside the repository the way mcpm does.

use super::flags::{switch, value, Flags, Spec};
use super::output::{table, CtlError, Output};
use super::skills::apply_home;
use super::skills_repo::{call, no_operands, operand, spec, str_of, strings, with_path, PATH};
use crate::plus::skills::LOCKFILE_NAME;
use serde_json::{json, Value};
use std::path::Path;

const STATUS_USAGE: &str =
    "usage: skills status [--repo <dir>] [--home <dir>] [--client <key>]... [--strict]";
const CLEAN_USAGE: &str = "usage: skills clean [--repo <dir>] [--home <dir>] [--client <key>] \
     [--project] [--dry-run]";
const UNINSTALL_USAGE: &str =
    "usage: skills uninstall <name> [--repo <dir>] [--home <dir>] [--project] [--dry-run]";
const RESOLVE_USAGE: &str = "usage: skills resolve [--repo <dir>] [--home <dir>] [--client <key>] \
     [--project] [--dry-run] [--migrate|--no-migrate]";

const STATUS: Spec = spec(
    &[PATH, value("--home"), value("--client"), switch("--strict")],
    STATUS_USAGE,
);
const CLEAN: Spec = spec(
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
const UNINSTALL: Spec = spec(
    &[
        PATH,
        value("--home"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
    ],
    UNINSTALL_USAGE,
);
const RESOLVE: Spec = spec(
    &[
        PATH,
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
        switch("--migrate"),
        switch("--no-migrate"),
    ],
    RESOLVE_USAGE,
);

fn global_mode(args: &Flags) -> Result<bool, CtlError> {
    if args.on("--global") && args.on("--project") {
        return Err(CtlError::usage(
            "--global and --project are mutually exclusive",
        ));
    }
    Ok(!args.on("--project"))
}

fn shown(path: &str, base: &str) -> String {
    Path::new(path)
        .strip_prefix(base)
        .map_or_else(|_| path.to_string(), |rel| rel.display().to_string())
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    let args = STATUS.parse(rest)?;
    no_operands(&args, STATUS_USAGE)?;
    apply_home(args.one("--home"));
    let mut request = with_path(&args, json!({}));
    let clients = args.all("--client");
    if !clients.is_empty() {
        request["client_keys"] = json!(clients);
    }
    let data = call("plus.skills.status", request)?;
    let missing_lock = data["lockfilePresent"] != json!(true);
    let drift = data["drift"] == json!(true);
    let human = if missing_lock {
        "No lockfile found. Run 'toolportctl skills sync' first.".to_string()
    } else if data["lockedCount"] == json!(0) {
        "No skills in lockfile.".to_string()
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
            "Drift detected. Run 'toolportctl skills sync' to update."
        } else {
            "All output files in sync."
        };
        format!(
            "{}\n\n{verdict}",
            table(&["Skill", "Client", "Status"], &rows)
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
        json!({
            "global_mode": global_mode(&args)?,
            "dry_run": dry_run,
        }),
    );
    if let Some(client) = args.one("--client") {
        request["client"] = json!(client);
    }
    let data = call("plus.skills.clean", request)?;
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
        lines.push("No managed skills in lockfile.".to_string());
    } else {
        let paths = strings(&data, "removed");
        lines.extend(
            paths
                .iter()
                .map(|p| format!("  {removed} {}", shown(p, base))),
        );
        if data["lockfileRemoved"] == json!(true) {
            lines.push(format!("  {removed} {LOCKFILE_NAME}"));
        }
        for skipped in data["skipped"].as_array().into_iter().flatten() {
            lines.push(format!(
                "  Skipped {}: {}",
                str_of(skipped, "client"),
                str_of(skipped, "error")
            ));
        }
        let total = paths.len() + usize::from(data["lockfileRemoved"] == json!(true));
        if total > 0 {
            lines.push(format!("\n{cleaned} {total} file(s)."));
        } else {
            lines.push("No managed files found to clean.".to_string());
        }
    }
    for name in strings(&data, "ignored") {
        lines.push(format!("Ignored lock entry with invalid name: {name}"));
    }
    Ok(Output::new(data, lines.join("\n")))
}

pub fn uninstall(rest: &[String]) -> Result<Output, CtlError> {
    let args = UNINSTALL.parse(rest)?;
    let name = operand(&args, "skill name", UNINSTALL_USAGE)?;
    apply_home(args.one("--home"));
    let dry_run = args.on("--dry-run");
    let request = with_path(
        &args,
        json!({
            "name": name,
            "global_mode": global_mode(&args)?,
            "dry_run": dry_run,
        }),
    );
    let data = call("plus.skills.uninstall", request)?;
    let (removed, done) = if dry_run {
        ("Would remove", "Would uninstall")
    } else {
        ("Removed", "Uninstalled")
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
    let verb = if dry_run { "remove" } else { "removed" };
    lines.push(format!(
        "\n{done} '{name}' and {verb} {} output file(s).",
        outputs.len()
    ));
    Ok(Output::new(data, lines.join("\n")))
}

pub fn resolve(rest: &[String]) -> Result<Output, CtlError> {
    let args = RESOLVE.parse(rest)?;
    no_operands(&args, RESOLVE_USAGE)?;
    if args.on("--migrate") && args.on("--no-migrate") {
        return Err(CtlError::usage(
            "--migrate and --no-migrate are mutually exclusive",
        ));
    }
    apply_home(args.one("--home"));
    let mut request = with_path(
        &args,
        json!({
            "global_mode": global_mode(&args)?,
            "dry_run": args.on("--dry-run"),
        }),
    );
    if args.on("--migrate") || args.on("--no-migrate") {
        request["migrate"] = json!(args.on("--migrate"));
    }
    if let Some(client) = args.one("--client") {
        request["client"] = json!(client);
    }
    let data = call("plus.skills.resolve", request)?;
    let collisions: &[Value] = data["collisions"].as_array().map_or(&[], Vec::as_slice);
    let mut lines = Vec::new();
    if data["skillCount"] == json!(0) {
        lines.push("No skills found in repository.".to_string());
    } else if collisions.is_empty() {
        lines.push("No collisions found.".to_string());
    } else {
        lines.push(format!("Found {} collision(s).", collisions.len()));
        for c in collisions {
            let (skill, path) = (str_of(c, "skill"), str_of(c, "collisionPath"));
            lines.push(match str_of(c, "action") {
                "replaced" => format!("  Replaced {path} → backup at {}", str_of(c, "backupPath")),
                "skipped-dry-run" => {
                    format!("  (dry run) would replace {path} (skill: {skill})")
                }
                _ => format!(
                    "  ! collision: {skill} ({}) — existing file at {path} shadows synced skill",
                    str_of(c, "client")
                ),
            });
        }
        let (replaced, kept) = (data["replaced"].as_u64(), data["kept"].as_u64());
        if replaced.unwrap_or(0) > 0 {
            lines.push(format!(
                "\nReplaced {} file(s) (backed up under {}).",
                data["replaced"],
                str_of(&data, "backupRoot")
            ));
        }
        if kept.unwrap_or(0) > 0 {
            lines.push(format!("Left {} file(s) in place.", data["kept"]));
        }
    }
    Ok(Output::new(data, lines.join("\n")))
}

#[cfg(test)]
#[path = "skills_state_tests.rs"]
mod tests;
