//! `toolportctl library status|pull|push`: the skills repository as a source (MIG-SRC-3). Thin
//! renderers over `plus::sources::{library_remote, library_sync}`, which the self-MCP tools
//! `library_status`, `library_pull` and `skills_git_push` call as well.

use super::flags::{switch, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::sources::library_remote as remote;
use crate::plus::sources::library_sync as sync;
use serde_json::Value;

const GROUP_USAGE: &str =
    "usage: library status|pull|push (status: [--fetch]; pull|push: [--dry-run])";

pub(super) const STATUS: Spec = Spec {
    flags: &[switch("--fetch")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const CHANGE: Spec = Spec {
    flags: &[switch("--dry-run")],
    ..STATUS
};

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    let data = remote::status_host(flags.on("--fetch"))?;
    let human = remote::human(&data);
    Ok(Output::new(data, human))
}

fn plan_text(data: &Value) -> String {
    let plan = &data["plan"];
    let mut out = format!("{}\n", plan["summary"].as_str().unwrap_or(""));
    for step in plan["steps"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<5} {}\n",
            step["op"].as_str().unwrap_or(""),
            step["detail"].as_str().unwrap_or("")
        ));
    }
    for warning in plan["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!("  warning: {}\n", warning.as_str().unwrap_or("")));
    }
    out.push_str("(dry run, nothing written)\n");
    out
}

fn render(data: Value, applied: &str) -> Output {
    let human = if data["dryRun"] == Value::Bool(true) {
        plan_text(&data)
    } else {
        let undo = data["result"]["undo"].as_str().unwrap_or("");
        let tail = if undo.is_empty() {
            String::new()
        } else {
            format!("undo: {undo}\n")
        };
        format!("{}\n{tail}", data["message"].as_str().unwrap_or(applied))
    };
    Output::new(data, human)
}

pub fn pull(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CHANGE.parse(rest)?;
    let repo = remote::host_repo()?;
    let data = sync::pull(&repo, flags.on("--dry-run"))?;
    let text = if data["pulled"] == Value::Bool(true) {
        "Fast-forwarded the library"
    } else {
        "The library is up to date"
    };
    Ok(render(data, text))
}

pub fn push(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CHANGE.parse(rest)?;
    let repo = remote::host_repo()?;
    let data = sync::push(&repo, None, flags.on("--dry-run"))?;
    Ok(render(data, "Pushed the library"))
}
