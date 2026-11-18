//! `toolportctl library select ls|set|plan|apply|undo|check`: choose single items of a tap or
//! plugin and keep the choice across updates. Thin renderers over `crate::selective`, which the
//! self-MCP tools `library_select_ls`, `library_select_plan` and `library_select_check` call too.

use super::context::plan_text;
use super::flags::{switch, value, Spec};
use super::output::{CtlError, Output};
use super::skills::enter_paths;
use super::skills_repo::{no_operands, operand, spec};
use crate::selective::handlers;
use crate::selective::ops::{Options, SetArgs};
use serde_json::Value;
use toolport_plus_api::Host;

const LS_USAGE: &str = "usage: library select ls [<source>]";
const SET_USAGE: &str =
    "usage: library select set <source> [--on a,b] [--off c] [--tick-hook h] [--dry-run]";
const PLAN_USAGE: &str = "usage: library select plan <source> [--no-fetch] [--switch]";
const APPLY_USAGE: &str =
    "usage: library select apply <source> --plan-hash <hash> [--no-fetch] [--switch]";
const UNDO_USAGE: &str = "usage: library select undo <source>";
const CHECK_USAGE: &str = "usage: library select check [--no-fetch]";

pub(super) const LS: Spec = spec(&[], LS_USAGE);
pub(super) const SET: Spec = spec(
    &[
        value("--on"),
        value("--off"),
        value("--tick-hook"),
        switch("--dry-run"),
    ],
    SET_USAGE,
);
pub(super) const PLAN: Spec = spec(&[switch("--no-fetch"), switch("--switch")], PLAN_USAGE);
pub(super) const APPLY: Spec = spec(
    &[
        value("--plan-hash").alias(&["--plan"]),
        switch("--no-fetch"),
        switch("--switch"),
    ],
    APPLY_USAGE,
);
pub(super) const UNDO: Spec = spec(&[], UNDO_USAGE);
pub(super) const CHECK: Spec = spec(&[switch("--no-fetch")], CHECK_USAGE);

fn csv(texts: Vec<String>) -> Vec<String> {
    texts
        .iter()
        .flat_map(|t| t.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn options(flags: &super::flags::Flags) -> Options {
    Options {
        fetch: !flags.on("--no-fetch"),
        switch: flags.on("--switch"),
    }
}

fn item_line(item: &Value) -> String {
    let mut line = format!(
        "  {:<7} {}:{}",
        item["state"].as_str().unwrap_or(""),
        item["kind"].as_str().unwrap_or(""),
        item["id"].as_str().unwrap_or("")
    );
    if let Some(command) = item["pending"]["command"].as_str() {
        line.push_str(&format!("  (needs a tick: command changed to {command})"));
    }
    line
}

fn ls_text(data: &Value) -> String {
    if data["items"].is_array() {
        let mut out = format!(
            "{}  {} of {} on, {} new, {} removed upstream (pinned {})\n",
            data["source"].as_str().unwrap_or(""),
            data["on"],
            data["total"],
            data["new"],
            data["removed"],
            data["ref"].as_str().unwrap_or("not yet")
        );
        for item in data["items"].as_array().into_iter().flatten() {
            out.push_str(&item_line(item));
            out.push('\n');
        }
        if let Some(text) = data["offer"]["text"].as_str() {
            out.push_str(&format!("\n{text}\n"));
        }
        return out;
    }
    let mut out = String::new();
    for source in data["sources"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}  {} of {} on, {} new (via {})\n",
            source["source"].as_str().unwrap_or(""),
            source["on"],
            source["total"],
            source["new"],
            source["slice"].as_str().unwrap_or("")
        ));
    }
    for offer in data["offers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}  installed whole as {}; `library select ls {}` to choose items\n",
            offer["source"].as_str().unwrap_or(""),
            offer["offer"]["whole"].as_str().unwrap_or(""),
            offer["source"].as_str().unwrap_or("")
        ));
    }
    if out.is_empty() {
        out.push_str("No source has a selection yet. `library select set <source> --on <item>` starts one.\n");
    }
    out
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let flags = LS.parse(rest)?;
    let source = match flags.operands() {
        [] => None,
        _ => Some(operand(&flags, "source", LS_USAGE)?),
    };
    let data = handlers::ls(host, source)?;
    let human = ls_text(&data);
    Ok(Output::new(data, human))
}

pub fn set(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let flags = SET.parse(rest)?;
    let source = operand(&flags, "source", SET_USAGE)?;
    let args = SetArgs {
        on: csv(flags.all("--on")),
        off: csv(flags.all("--off")),
        tick_hook: csv(flags.all("--tick-hook")),
    };
    if args.on.is_empty() && args.off.is_empty() && args.tick_hook.is_empty() {
        return Err(CtlError::usage(format!(
            "nothing to set: pass --on, --off or --tick-hook\n{SET_USAGE}"
        )));
    }
    let data = handlers::set(host, source, &args, flags.on("--dry-run"))?;
    let mut human = String::new();
    for line in data["changed"].as_array().into_iter().flatten() {
        human.push_str(&format!("  {}\n", line.as_str().unwrap_or("")));
    }
    human.push_str(&format!(
        "{} of {} on. Nothing is loaded until `{}` is applied.\n",
        data["on"],
        data["total"],
        data["next"].as_str().unwrap_or("")
    ));
    Ok(Output::new(data, human))
}

pub fn plan(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let flags = PLAN.parse(rest)?;
    let source = operand(&flags, "source", PLAN_USAGE)?;
    let data = handlers::plan(host, source, options(&flags))?;
    let human = format!(
        "{}plan: {}\napply: toolportctl library select apply {source} --plan-hash {}\n",
        plan_text(&data),
        data["planHash"].as_str().unwrap_or(""),
        data["planHash"].as_str().unwrap_or("")
    );
    Ok(Output::new(data, human))
}

pub fn apply(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let flags = APPLY.parse(rest)?;
    let source = operand(&flags, "source", APPLY_USAGE)?;
    let hash = flags
        .one("--plan-hash")
        .ok_or_else(|| CtlError::usage(format!("--plan-hash <hash> is required\n{APPLY_USAGE}")))?;
    let data = handlers::apply(host, source, hash, options(&flags))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let flags = UNDO.parse(rest)?;
    let source = operand(&flags, "source", UNDO_USAGE)?;
    let data = handlers::undo(host, source)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn check(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let flags = CHECK.parse(rest)?;
    no_operands(&flags, CHECK_USAGE)?;
    let data = handlers::check(host, !flags.on("--no-fetch"))?;
    let mut human = format!("{} source(s) have a plan waiting\n", data["waiting"]);
    for row in data["sources"].as_array().into_iter().flatten() {
        let mark = if row["waiting"] == Value::Bool(true) {
            "waiting"
        } else {
            "current"
        };
        human.push_str(&format!(
            "  {mark:<8} {}\n",
            row["source"].as_str().unwrap_or("")
        ));
    }
    Ok(Output::new(data, human))
}
