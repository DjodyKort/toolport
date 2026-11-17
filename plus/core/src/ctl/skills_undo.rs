//! `toolportctl skills undo|verify` and `skills auto status|pause|resume`: renderers over
//! `skills::api` and `skills::auto`, which the self-MCP tools `skills_undo`, `skills_verify` and
//! `skills_auto_*` call as well.

use super::flags::{switch, value, Spec};
use super::output::{CtlError, Output};
use super::skills::{enter_paths, served, BASE};
use super::skills_repo::{str_of, strings};
use super::skills_state::global_mode;
use crate::redact;
use crate::skills::api::{self, Args};
use crate::skills::auto;
use crate::skills::SystemClock;
use serde_json::{json, Value};
use std::path::PathBuf;
use toolport_plus_api::Host;

pub(super) const UNDO: Spec = Spec {
    flags: &[
        value("--repo"),
        value("--home"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
    ],
    ..BASE
};
pub(super) const VERIFY: Spec = Spec {
    flags: &[
        value("--repo"),
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
    ],
    ..BASE
};

fn count_line(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn undo_text(data: &Value) -> String {
    let dry = data["dryRun"] == json!(true);
    let snapshot = &data["snapshot"];
    let mut lines = vec![format!(
        "{}snapshot {} of {}: {}, {}",
        if dry {
            "(dry run) Would restore "
        } else {
            "Restored "
        },
        str_of(snapshot, "id"),
        str_of(snapshot, "createdAt"),
        count_line(snapshot["files"].as_u64().unwrap_or(0), "file", "files"),
        count_line(
            snapshot["hooks"].as_u64().unwrap_or(0),
            "settings file",
            "settings files"
        ),
    )];
    for row in data["files"].as_array().into_iter().flatten() {
        let reason = row["reason"]
            .as_str()
            .map_or(String::new(), |r| format!(": {r}"));
        lines.push(format!(
            "  {:<8} {}{reason}",
            str_of(row, "action"),
            str_of(row, "path")
        ));
    }
    for row in data["hooks"].as_array().into_iter().flatten() {
        lines.push(format!(
            "  {:<8} hooks in {}",
            str_of(row, "action"),
            str_of(row, "path")
        ));
    }
    lines.push(format!(
        "  {:<8} {}",
        str_of(&data["lock"], "action"),
        str_of(&data["lock"], "path")
    ));
    if let Some(refused) = data["refused"].as_str() {
        lines.push(format!("\nRefused: {refused}"));
    } else if dry {
        lines.push(
            "\nAuto-sync would be paused; resume it with `toolportctl skills auto resume`.".into(),
        );
    } else {
        lines
            .push("\nAuto-sync is paused; resume it with `toolportctl skills auto resume`.".into());
    }
    lines.join("\n")
}

pub fn undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNDO.parse(rest)?;
    let _paths = enter_paths(host, flags.one("--home"));
    let args = Args {
        repo: flags.one("--repo").map(PathBuf::from),
        global: global_mode(&flags)?,
        dry_run: flags.on("--dry-run"),
        ..Args::default()
    };
    let data = api::undo(&args, Some(host)).map(redact::scrub)?;
    let mut out = Output::new(data.clone(), undo_text(&data));
    out.failed = data["refused"].is_string();
    Ok(out)
}

fn verify_text(data: &Value) -> String {
    if data["lockfilePresent"] == json!(false) {
        return "No lockfile found. Run 'toolportctl skills sync' first.".to_string();
    }
    let (matched, total) = (&data["matched"], &data["total"]);
    let mut lines = vec![format!("{matched} of {total} files match")];
    for row in data["mismatched"].as_array().into_iter().flatten() {
        lines.push(format!(
            "  {:<8} {} ({}, {})",
            str_of(row, "reason"),
            str_of(row, "path"),
            str_of(row, "skill"),
            str_of(row, "client")
        ));
    }
    for warning in strings(data, "warnings") {
        lines.push(format!("\nWarning: {warning}"));
    }
    lines.join("\n")
}

pub fn verify(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = VERIFY.parse(rest)?;
    let _paths = enter_paths(host, flags.one("--home"));
    let clients = flags.all("--client");
    let args = Args {
        repo: flags.one("--repo").map(PathBuf::from),
        clients: (!clients.is_empty()).then_some(clients),
        global: global_mode(&flags)?,
        ..Args::default()
    };
    let data = served(api::verify(&args))?;
    let mut out = Output::new(data.clone(), verify_text(&data));
    out.failed = data["clean"] != json!(true);
    Ok(out)
}

fn auto_text(data: &Value) -> String {
    if data["paused"] == json!(true) {
        format!(
            "Auto-sync is paused since {} (reason: {}).",
            str_of(data, "since"),
            str_of(data, "reason")
        )
    } else {
        "Auto-sync is running.".to_string()
    }
}

const NONE: Spec = Spec { flags: &[], ..BASE };

fn auto_run(
    host: &dyn Host,
    rest: &[String],
    run: impl FnOnce() -> Result<Value, crate::op::OpError>,
    changed: (&str, &str),
) -> Result<Output, CtlError> {
    NONE.parse(rest)?;
    let _paths = enter_paths(host, None);
    let data = served(run())?;
    let mut human = auto_text(&data);
    if let Some(did) = data["changed"].as_bool() {
        human = format!("{} {human}", if did { changed.0 } else { changed.1 });
    }
    Ok(Output::new(data, human))
}

pub fn auto_status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    auto_run(host, rest, auto::status_op, ("", ""))
}

pub fn auto_pause(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    auto_run(
        host,
        rest,
        || auto::pause_op(&SystemClock),
        ("Paused.", "Already paused."),
    )
}

pub fn auto_resume(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    auto_run(host, rest, auto::resume_op, ("Resumed.", "Was not paused."))
}
