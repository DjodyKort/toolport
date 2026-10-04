//! `toolportctl styles add|ls|lint|diff|status|sync|apply|remove|clean`: renderers over the
//! `plus.styles.*` handlers. Texts follow mcpm's with `toolportctl` in the hints (D-042). Scope
//! is user level by default like `skills sync`; `--project` works inside the repository the way
//! mcpm does. `--path` is the repository root, `--repo` its alias.

use super::flags::{switch, value, Spec};
use super::output::{table, CtlError, Output};
use super::skills::apply_home;
use super::skills_repo::{no_operands, operand, spec, str_of, strings, with_path, PATH};
use super::skills_state::global_mode;
use serde_json::{json, Value};

const ADD_USAGE: &str = "usage: styles add <name> [--path <dir>] [--dry-run]";
const LS_USAGE: &str = "usage: styles ls [--path <dir>]";
const LINT_USAGE: &str = "usage: styles lint [--path <dir>]";
const DIFF_USAGE: &str = "usage: styles diff [--path <dir>]";
const STATUS_USAGE: &str = "usage: styles status [--path <dir>]";
const SYNC_USAGE: &str = "usage: styles sync [--path <dir>] [--home <dir>] [--client <key>]... \
     [--project] [--dry-run]";
const APPLY_USAGE: &str = "usage: styles apply <name> [--path <dir>] [--home <dir>] \
     [--client <key>]... [--project] [--dry-run]";
const REMOVE_USAGE: &str = "usage: styles remove [--path <dir>] [--home <dir>] \
     [--client <key>]... [--project] [--dry-run]";
const CLEAN_USAGE: &str = "usage: styles clean [--path <dir>] [--home <dir>] [--project] \
     [--dry-run]";
const USAGE: &str = "usage: styles add|ls|lint|diff|status|sync|apply|remove|clean \
     (add: <name> [--path <dir>] [--dry-run]; ls|lint|diff|status: [--path <dir>]; \
     sync: [--path <dir>] [--home <dir>] [--client <key>]... [--project] [--dry-run]; \
     apply: <name> [--path <dir>] [--home <dir>] [--client <key>]... [--project] [--dry-run]; \
     remove: [--path <dir>] [--home <dir>] [--client <key>]... [--project] [--dry-run]; \
     clean: [--path <dir>] [--home <dir>] [--project] [--dry-run])";

pub(super) const ADD: Spec = spec(&[PATH, switch("--dry-run")], ADD_USAGE);
pub(super) const LS: Spec = spec(&[PATH], LS_USAGE);
pub(super) const LINT: Spec = spec(&[PATH], LINT_USAGE);
pub(super) const DIFF: Spec = spec(&[PATH], DIFF_USAGE);
pub(super) const STATUS: Spec = spec(&[PATH], STATUS_USAGE);
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
pub(super) const APPLY: Spec = spec(
    &[
        PATH,
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
    ],
    APPLY_USAGE,
);
pub(super) const REMOVE: Spec = spec(
    &[
        PATH,
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
    ],
    REMOVE_USAGE,
);
pub(super) const CLEAN: Spec = spec(
    &[
        PATH,
        value("--home"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
    ],
    CLEAN_USAGE,
);

const GLOBAL_NOTE: &str = "Global mode -- writing to user-level paths.";
const DESCRIPTION_WIDTH: usize = 60;

fn call(command: &str, args: Value) -> Result<Value, CtlError> {
    crate::plus::dispatch(command, args).map_err(|e| CtlError::failed("styles", e))
}

/// The style files that failed to parse come first, where mcpm's log lines land.
fn with_warnings(data: &Value, body: String) -> String {
    let warnings = strings(data, "discoveryWarnings");
    if warnings.is_empty() {
        body
    } else {
        format!("{}\n{body}", warnings.join("\n"))
    }
}

/// mcpm cuts the description at 60 characters and appends `...`.
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

fn joined(items: &[String], sep: &str, empty: &str) -> String {
    if items.is_empty() {
        empty.to_string()
    } else {
        items.join(sep)
    }
}

fn client_args(args: &super::flags::Flags, mut request: Value) -> Value {
    let clients = args.all("--client");
    if !clients.is_empty() {
        request["client_keys"] = json!(clients);
    }
    request
}

/// The `{client, style}` pairs of `data[key]` sorted by client, the order mcpm prints them in.
fn sorted_pairs(data: &Value, key: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = data[key]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| (str_of(p, "client").to_string(), str_of(p, "style").to_string()))
        .collect();
    pairs.sort();
    pairs
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(USAGE))
}

pub fn add(rest: &[String]) -> Result<Output, CtlError> {
    let args = ADD.parse(rest)?;
    let name = operand(&args, "style name", ADD_USAGE)?;
    let dry_run = args.on("--dry-run");
    let request = with_path(&args, json!({"name": name, "dry_run": dry_run}));
    let data = call("plus.styles.add", request)?;
    let path = str_of(&data, "path");
    let human = if dry_run {
        format!("Would create style '{name}':\n  {path}")
    } else {
        format!(
            "Created style '{name}' at {path}\nEdit the file to add your style instructions, \
             then run 'toolportctl styles sync'."
        )
    };
    Ok(Output::new(data, human))
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let args = LS.parse(rest)?;
    no_operands(&args, LS_USAGE)?;
    let data = call("plus.styles.list", with_path(&args, json!({})))?;
    let styles = data["styles"].as_array().map_or(&[][..], Vec::as_slice);
    let body = if styles.is_empty() {
        "No styles found. Run 'toolportctl styles add <name>' to create one.".to_string()
    } else {
        let rows: Vec<Vec<String>> = styles
            .iter()
            .map(|s| {
                let synced = if s["synced"] == json!(true) {
                    joined(&strings(s, "clientsSynced"), ", ", "--")
                } else {
                    "not synced".to_string()
                };
                vec![
                    str_of(s, "name").to_string(),
                    cut_description(str_of(s, "description")),
                    if s["keepCodingInstructions"] == json!(true) {
                        "yes"
                    } else {
                        "no"
                    }
                    .to_string(),
                    synced,
                ]
            })
            .collect();
        let mut text = table(
            &["Style", "Description", "Keep coding instructions", "Synced to"],
            &rows,
        );
        let active = sorted_pairs(&data, "active");
        if !active.is_empty() {
            text.push_str("\n\nActive styles (Tier 2 clients):");
            for (client, style) in active {
                text.push_str(&format!("\n  {client}: {style}"));
            }
        }
        text
    };
    let human = with_warnings(&data, body);
    Ok(Output::new(data, human))
}

pub fn lint(rest: &[String]) -> Result<Output, CtlError> {
    let args = LINT.parse(rest)?;
    no_operands(&args, LINT_USAGE)?;
    let data = call("plus.styles.lint", with_path(&args, json!({})))?;
    let count = |key: &str| data[key].as_u64().unwrap_or(0);
    let body = if data["styleCount"] == json!(0) {
        "No styles found to lint.".to_string()
    } else if data["messages"].as_array().is_none_or(Vec::is_empty) {
        format!("All {} style(s) passed lint checks.", data["styleCount"])
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

pub fn diff(rest: &[String]) -> Result<Output, CtlError> {
    let args = DIFF.parse(rest)?;
    no_operands(&args, DIFF_USAGE)?;
    let data = call("plus.styles.diff", with_path(&args, json!({})))?;
    let (new, modified, removed) = (
        strings(&data, "new"),
        strings(&data, "modified"),
        strings(&data, "removed"),
    );
    let body = if data["noLockfile"] == json!(true) {
        let mut text = "No lockfile found. All styles are new.\n".to_string();
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
    let data = call("plus.styles.status", with_path(&args, json!({})))?;
    let human = if data["lockfilePresent"] != json!(true) {
        "No lockfile found. Run 'toolportctl styles sync' first.".to_string()
    } else {
        let native: Vec<Vec<String>> = data["native"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                vec![
                    str_of(row, "name").to_string(),
                    joined(&strings(row, "styles"), ", ", "none"),
                ]
            })
            .collect();
        let apply: Vec<Vec<String>> = data["applyRemove"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                vec![
                    str_of(row, "name").to_string(),
                    row["active"].as_str().unwrap_or("none").to_string(),
                ]
            })
            .collect();
        format!(
            "Tier 1 -- Native toggle (all styles available):\n\n{}\n\n\
             Tier 2 -- Apply/Remove (one active style):\n\n{}",
            table(&["Client", "Styles synced"], &native),
            table(&["Client", "Active style"], &apply)
        )
    };
    Ok(Output::new(data, human))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let args = SYNC.parse(rest)?;
    no_operands(&args, SYNC_USAGE)?;
    apply_home(args.one("--home"));
    let dry_run = args.on("--dry-run");
    let request = client_args(
        &args,
        with_path(
            &args,
            json!({"global_mode": global_mode(&args)?, "dry_run": dry_run}),
        ),
    );
    let data = call("plus.styles.sync", request)?;
    let human = if data["foundCount"] == json!(0) {
        "No styles found in repository. Run 'toolportctl styles add <name>' first.".to_string()
    } else {
        let mut text = String::new();
        if dry_run {
            text.push_str("Dry run -- no files will be written.\n\n");
        }
        if str_of(&data, "scope") == "global" {
            text.push_str(GLOBAL_NOTE);
            text.push_str("\n\n");
        }
        let rows: Vec<Vec<String>> = data["styles"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                vec![
                    str_of(s, "name").to_string(),
                    cut_description(str_of(s, "description")),
                    joined(&strings(s, "clientsSynced"), ", ", "none"),
                    joined(&strings(s, "warnings"), "; ", "--"),
                ]
            })
            .collect();
        text.push_str(&format!(
            "\n{}Synced {} style(s) to {} native client(s).\n\n{}\n\n\
             These clients support native toggling. For other clients, use \
             'toolportctl styles apply <name>'.",
            if dry_run { "(dry run) " } else { "" },
            data["styleCount"],
            data["clientCount"],
            table(&["Style", "Description", "Clients synced", "Warnings"], &rows)
        ));
        text
    };
    Ok(Output::new(data.clone(), with_warnings(&data, human)))
}

pub fn apply(rest: &[String]) -> Result<Output, CtlError> {
    let args = APPLY.parse(rest)?;
    let name = operand(&args, "style name", APPLY_USAGE)?;
    apply_home(args.one("--home"));
    let dry_run = args.on("--dry-run");
    let request = client_args(
        &args,
        with_path(
            &args,
            json!({"name": name, "global_mode": global_mode(&args)?, "dry_run": dry_run}),
        ),
    );
    let data = call("plus.styles.apply", request)?;
    let mut text = String::new();
    for client in strings(&data, "nativeClients") {
        text.push_str(&format!(
            "Note: '{client}' supports native style toggling. Use 'toolportctl styles sync' \
             instead and toggle in the client UI.\n\n"
        ));
    }
    if dry_run {
        text.push_str("Dry run -- no files will be written.\n\n");
    }
    if str_of(&data, "scope") == "global" {
        text.push_str(GLOBAL_NOTE);
        text.push_str("\n\n");
    }
    for (client, old) in sorted_replaced(&data) {
        text.push_str(&format!("Replacing active style '{old}' on {client}\n"));
    }
    let rows: Vec<Vec<String>> = sorted_pairs(&data, "active")
        .into_iter()
        .map(|(client, style)| vec![client, style])
        .collect();
    text.push_str(&format!(
        "\n{}Applied style '{name}' to {} client(s).\n\n{}",
        if dry_run { "(dry run) " } else { "" },
        data["appliedCount"],
        table(&["Client", "Active style"], &rows)
    ));
    Ok(Output::new(data.clone(), with_warnings(&data, text)))
}

/// The replaced styles in the order the lock lists them, which is the order mcpm prints them in.
fn sorted_replaced(data: &Value) -> Vec<(String, String)> {
    data["replaced"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| (str_of(p, "client").to_string(), str_of(p, "style").to_string()))
        .collect()
}

pub fn remove(rest: &[String]) -> Result<Output, CtlError> {
    let args = REMOVE.parse(rest)?;
    no_operands(&args, REMOVE_USAGE)?;
    apply_home(args.one("--home"));
    let dry_run = args.on("--dry-run");
    let request = client_args(
        &args,
        with_path(
            &args,
            json!({"global_mode": global_mode(&args)?, "dry_run": dry_run}),
        ),
    );
    let data = call("plus.styles.remove", request)?;
    let removed = data["removed"].as_array().map_or(&[][..], Vec::as_slice);
    let human = if data["hadActive"] != json!(true) {
        "No active styles to remove.".to_string()
    } else if removed.is_empty() {
        format!(
            "No active style on client '{}'.",
            strings(&data, "clientKeys").join(", ")
        )
    } else {
        let mut text = String::new();
        if dry_run {
            text.push_str("Dry run -- no files will be removed.\n\n");
        }
        if str_of(&data, "scope") == "global" {
            text.push_str(GLOBAL_NOTE);
            text.push_str("\n\n");
        }
        let prefix = if dry_run { "(dry run) " } else { "" };
        for target in removed {
            text.push_str(&format!(
                "{prefix}Removing style '{}' from {}\n",
                str_of(target, "style"),
                str_of(target, "client")
            ));
        }
        let verb = if dry_run { "Would remove" } else { "Removed" };
        text.push_str(&format!(
            "\nDone. {verb} active style from {} client(s).",
            removed.len()
        ));
        text
    };
    Ok(Output::new(data, human))
}

pub fn clean(rest: &[String]) -> Result<Output, CtlError> {
    let args = CLEAN.parse(rest)?;
    no_operands(&args, CLEAN_USAGE)?;
    apply_home(args.one("--home"));
    let dry_run = args.on("--dry-run");
    let request = with_path(
        &args,
        json!({"global_mode": global_mode(&args)?, "dry_run": dry_run}),
    );
    let data = call("plus.styles.clean", request)?;
    let (removed, cleaned) = if dry_run {
        ("Would remove", "Would clean")
    } else {
        ("Removed", "Cleaned")
    };
    let mut lines: Vec<String> = Vec::new();
    let paths = strings(&data, "removed");
    lines.extend(paths.iter().map(|p| format!("  {removed}: {p}")));
    for skipped in data["skipped"].as_array().into_iter().flatten() {
        lines.push(format!(
            "  Error cleaning {}: {}",
            str_of(skipped, "client"),
            str_of(skipped, "error")
        ));
    }
    if paths.is_empty() {
        lines.push("No style files found to clean.".to_string());
    } else {
        lines.push(format!(
            "\n{cleaned} {} style file(s) from clients.",
            paths.len()
        ));
    }
    for name in strings(&data, "ignored") {
        lines.push(format!("Ignored lock entry with invalid name: {name}"));
    }
    Ok(Output::new(data, lines.join("\n")))
}
