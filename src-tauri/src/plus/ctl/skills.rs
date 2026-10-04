//! `toolportctl skills sync|ls|lint|diff`: thin renderers over the `plus.skills.*` handlers.
//! `init|add|audit|bundle|unbundle` live in `skills_repo.rs`, `status|clean|uninstall|resolve` in
//! `skills_state.rs`.

use super::flags::{switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{table, CtlError, Output};
use super::skills_repo::{str_of, strings};
use super::skills_state::{collision_line, shown};
use serde_json::{json, Value};

const USAGE: &str = "usage: skills init|add|ls|lint|audit|bundle|unbundle|sync|diff|status|clean|uninstall|resolve \
     (sync|ls|lint|diff: [--repo <dir>] [--home <dir>]; sync: [--client <key>]... [--project] \
     [--dry-run] [--migrate|--no-migrate]; lint: [--name <skill>]...; init: [--path <dir>] [--name <name>] [--dry-run]; \
     add: <name> [--type skill|rule] [--path <dir>] [--with-progressive] [--dry-run]; \
     audit: [--path <dir>]; bundle: [--output <zip>] [--path <dir>] [--skills <a,b>] [--dry-run]; \
     unbundle: <bundle.zip> [--path <dir>] [--dry-run]; \
     status: [--repo <dir>] [--client <key>]... [--strict]; \
     clean: [--repo <dir>] [--client <key>] [--project] [--dry-run]; \
     uninstall: <name> [--repo <dir>] [--project] [--dry-run]; \
     resolve: [--repo <dir>] [--client <key>] [--project] [--dry-run] [--migrate|--no-migrate])";

const BASE: Spec = Spec {
    flags: &[],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};
const SYNC: Spec = Spec {
    flags: &[
        value("--repo"),
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
        switch("--migrate"),
        switch("--no-migrate"),
    ],
    ..BASE
};
const LS: Spec = Spec {
    flags: &[value("--repo"), value("--home")],
    ..BASE
};
const LINT: Spec = Spec {
    flags: &[value("--repo"), value("--home"), value("--name")],
    ..BASE
};

pub(super) fn apply_home(home: Option<&str>) {
    if let Some(home) = home {
        std::env::set_var("HOME", home);
        std::env::set_var("USERPROFILE", home);
    }
}

fn repo_args(flags: &Flags) -> Value {
    let mut args = json!({});
    if let Some(repo) = flags.one("--repo") {
        args["repo_path"] = json!(repo);
    }
    args
}

fn call(command: &str, args: Value) -> Result<Value, CtlError> {
    crate::plus::dispatch(command, args).map_err(|e| CtlError::failed("skills", e))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(USAGE))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SYNC.parse(rest)?;
    if flags.on("--project") && flags.on("--global") {
        return Err(CtlError::usage("--global and --project are mutually exclusive"));
    }
    if flags.on("--migrate") && flags.on("--no-migrate") {
        return Err(CtlError::usage(
            "--migrate and --no-migrate are mutually exclusive",
        ));
    }
    apply_home(flags.one("--home"));
    let mut args = repo_args(&flags);
    args["dry_run"] = json!(flags.on("--dry-run"));
    args["global_mode"] = json!(!flags.on("--project"));
    if flags.on("--migrate") || flags.on("--no-migrate") {
        args["migrate"] = json!(flags.on("--migrate"));
    }
    let clients = flags.all("--client");
    if !clients.is_empty() {
        args["client_keys"] = json!(clients);
    }
    let data = call("plus.skills.sync", args)?;
    let human = sync_text(&data);
    Ok(Output::new(data, human))
}

fn cell(text: String, fallback: &str) -> String {
    if text.is_empty() {
        fallback.to_string()
    } else {
        text
    }
}

fn stale_lines(data: &Value, prefix: &str) -> Vec<String> {
    let cleaned = strings(data, "cleaned");
    if cleaned.is_empty() {
        return Vec::new();
    }
    let root = str_of(data, "outputRoot");
    let mut lines = vec![format!(
        "\n{prefix}Removed {} stale file(s) from prior syncs (renames or deletions):",
        cleaned.len()
    )];
    lines.extend(cleaned.iter().map(|p| format!("  - {}", shown(p, root))));
    lines
}

/// mcpm's `skills sync` report: the collisions as they are resolved, the entries with their
/// warnings, the append-mode notes, the stale files and the collision summary.
fn sync_text(data: &Value) -> String {
    let prefix = if data["dryRun"] == json!(true) {
        "(dry run) "
    } else {
        ""
    };
    let entries: &[Value] = data["entries"].as_array().map_or(&[], Vec::as_slice);
    if entries.is_empty() {
        let mut lines = vec!["No skills found in repository.".to_string()];
        lines.extend(stale_lines(data, prefix));
        return lines.join("\n");
    }
    let mut lines = Vec::new();
    if !prefix.is_empty() {
        lines.push("Dry run -- no files will be written.\n".to_string());
    }
    if data["globalMode"] == json!(true) {
        lines.push("Global mode -- writing to user-level paths.\n".to_string());
    }
    let collisions: &[Value] = data["collisions"].as_array().map_or(&[], Vec::as_slice);
    lines.extend(collisions.iter().map(collision_line));
    lines.push(format!(
        "\n{prefix}Synced {} skill(s) to {} client(s).\n",
        entries.len(),
        data["clientCount"]
    ));
    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|e| {
            vec![
                str_of(e, "name").to_string(),
                str_of(e, "type").to_string(),
                cell(strings(e, "clientsSynced").join(", "), "none"),
                cell(strings(e, "warnings").join("; "), "--"),
            ]
        })
        .collect();
    lines.push(table(&["Skill", "Type", "Clients synced", "Warnings"], &rows));
    let synced_to = |client: &str| {
        entries
            .iter()
            .any(|e| strings(e, "clientsSynced").iter().any(|c| c == client))
    };
    if synced_to("agents-md") {
        lines.push(format!(
            "\n{prefix}Updated AGENTS.md with <available_skills> block."
        ));
    }
    if synced_to("zed") {
        lines.push(format!(
            "{prefix}Updated .rules with concatenated skills for Zed."
        ));
    }
    lines.extend(stale_lines(data, prefix));
    let (replaced, kept) = (
        data["replaced"].as_u64().unwrap_or(0),
        data["kept"].as_u64().unwrap_or(0),
    );
    if replaced > 0 {
        lines.push(format!(
            "\n{prefix}Replaced {replaced} colliding file(s) (backed up)."
        ));
    }
    if kept > 0 {
        lines.push(format!(
            "\n{prefix}{kept} unresolved collision(s). Run toolportctl skills resolve to \
             handle them, or re-run with --migrate to auto-replace."
        ));
    }
    lines.join("\n")
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    apply_home(flags.one("--home"));
    let data = call("plus.skills.list", repo_args(&flags))?;
    let mut human = String::new();
    for row in data["skills"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{:<6} {:<28} {}\n",
            row["type"].as_str().unwrap_or(""),
            row["name"].as_str().unwrap_or(""),
            row["description"].as_str().unwrap_or("")
        ));
    }
    if human.is_empty() {
        human.push_str("no skills found");
    }
    Ok(Output::new(data, human))
}

pub fn lint(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LINT.parse(rest)?;
    apply_home(flags.one("--home"));
    let mut args = repo_args(&flags);
    let names = flags.all("--name");
    if !names.is_empty() {
        args["names"] = json!(names);
    }
    let data = call("plus.skills.lint", args)?;
    let mut human = String::new();
    for m in data["messages"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{} {}: {}\n",
            m["level"].as_str().unwrap_or(""),
            m["name"].as_str().unwrap_or(""),
            m["message"].as_str().unwrap_or("")
        ));
    }
    human.push_str(&format!(
        "{} error(s), {} warning(s)",
        data["errors"], data["warnings"]
    ));
    let mut out = Output::new(data.clone(), human);
    out.failed = data["errors"].as_u64().unwrap_or(0) > 0;
    Ok(out)
}

pub fn diff(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    apply_home(flags.one("--home"));
    let data = call("plus.skills.diff", repo_args(&flags))?;
    let mut human = String::new();
    for (key, mark) in [("new", "+"), ("modified", "~"), ("removed", "-")] {
        for name in data[key].as_array().into_iter().flatten() {
            human.push_str(&format!("{mark} {}\n", name.as_str().unwrap_or("")));
        }
    }
    human.push_str(&format!("{} unchanged", data["unchanged"]));
    if data["noLockfile"] == json!(true) {
        human.push_str(" (no lockfile)");
    }
    let mut out = Output::new(data.clone(), human);
    out.failed = data["clean"] != json!(true);
    Ok(out)
}
