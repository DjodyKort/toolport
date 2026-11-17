//! `toolportctl library status|pull|push`: the skills repository as a source. Thin renderers over
//! `crate::library`, which the self-MCP tools `library_status`, `library_pull` and
//! `skills_git_push` call as well.

use super::flags::{switch, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use super::skills::enter_paths;
use crate::library::{self, remote, sync};
use serde_json::Value;
use toolport_plus_api::Host;

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

pub(super) const PULL: Spec = Spec {
    flags: &[switch("--dry-run"), switch("--review")],
    ..STATUS
};

pub fn status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    let _paths = enter_paths(host, None);
    let data = remote::status(flags.on("--fetch"))?;
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

pub fn pull(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PULL.parse(rest)?;
    let _paths = enter_paths(host, None);
    let repo = remote::locate()?;
    let data = library::pull(&repo, flags.on("--dry-run"), flags.on("--review"))?;
    let text = if data["pulled"] == Value::Bool(true) {
        "Fast-forwarded the library"
    } else {
        "The library is up to date"
    };
    Ok(render(data, text))
}

pub fn push(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CHANGE.parse(rest)?;
    let _paths = enter_paths(host, None);
    let repo = remote::locate()?;
    let data = sync::push(&repo, None, flags.on("--dry-run"))?;
    Ok(render(data, "Pushed the library"))
}
