//! `toolportctl skills undo|verify` and `skills auto status|on|off|pause|resume`: renderers over
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

fn paused_line(data: &Value) -> String {
    format!(
        "Auto-sync is paused since {} (reason: {}).",
        str_of(data, "since"),
        str_of(data, "reason")
    )
}

fn schedule_line(schedule: &Value) -> String {
    let when = schedule["when"].as_str().unwrap_or("no schedule");
    if schedule["present"] == json!(false) {
        return format!(
            "The task {} is not available, so nothing runs the sync.",
            str_of(schedule, "task")
        );
    }
    if schedule["enabled"] == json!(true) {
        let next = schedule["nextRun"]
            .as_str()
            .map_or(String::new(), |at| format!(", next run {at}"));
        format!("Auto-sync is on: {when}{next}.")
    } else {
        format!("Auto-sync is off ({when}); turn it on with `toolportctl skills auto on`.")
    }
}

fn last_run_line(run: &Value) -> String {
    let held = run["held"].as_array().map_or(0, Vec::len);
    let mut line = format!(
        "Last run {}: {} applied, {held} held, {} skipped.",
        str_of(run, "at"),
        run["applied"],
        run["skipped"]
    );
    if let Some(error) = run["error"].as_str() {
        line.push_str(&format!(" It failed: {error}"));
    }
    line
}

fn auto_text(data: &Value) -> String {
    let mut lines = Vec::new();
    if data["schedule"].is_object() {
        lines.push(schedule_line(&data["schedule"]));
    }
    if data["paused"] == json!(true) {
        lines.push(paused_line(data));
    } else if !data["schedule"].is_object() {
        lines.push("Auto-sync is not paused.".to_string());
    }
    if data["lastRun"].is_object() {
        lines.push(last_run_line(&data["lastRun"]));
    }
    lines.join("\n")
}

const NONE: Spec = Spec { flags: &[], ..BASE };
pub(super) const SWITCH: Spec = Spec {
    flags: &[switch("--dry-run")],
    ..BASE
};

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
    auto_run(host, rest, || auto::full_status_op(host), ("", ""))
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

fn auto_switch(host: &dyn Host, rest: &[String], enabled: bool) -> Result<Output, CtlError> {
    let flags = SWITCH.parse(rest)?;
    let _paths = enter_paths(host, None);
    let data = served(auto::switch_op(host, enabled, flags.on("--dry-run")))?;
    let mut human = data["plan"]["summary"].as_str().unwrap_or("").to_string();
    for warning in strings(&data["plan"], "warnings") {
        human.push_str(&format!("\nwarning: {warning}"));
    }
    if data["dryRun"] == json!(true) {
        human.push_str("\n(dry run, nothing changed)");
    } else if let Some(undo) = data["result"]["undo"].as_str() {
        human.push_str(&format!("\nundo: {undo}"));
    }
    human.push_str(&format!("\n{}", schedule_line(&data["schedule"])));
    Ok(Output::new(data, human))
}

pub fn auto_on(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    auto_switch(host, rest, true)
}

pub fn auto_off(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    auto_switch(host, rest, false)
}
