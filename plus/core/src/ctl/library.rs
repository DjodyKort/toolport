//! `toolportctl library status|pull|push|connect|commit|remote add`: the skills repository as a
//! source. Thin renderers over `crate::library`, which the self-MCP tools `library_status`,
//! `library_pull`, `skills_git_push`, `library_connect`, `library_commit` and
//! `library_remote_add` call as well. `library open` only prints the folder to open.

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use super::skills::enter_paths;
use crate::library::{self, connect, remote, sync};
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

const CONNECT_USAGE: &str = "usage: library connect <url> [--clone-to <dir>] [--dry-run]";
const REMOTE_ADD_USAGE: &str = "usage: library remote add <url> [--dry-run]";
const OPEN_USAGE: &str = "usage: library open [<skill>]";
const COMMIT_USAGE: &str = "usage: library commit -m <message> [--dry-run]";

pub(super) const CONNECT: Spec = Spec {
    flags: &[value("--clone-to"), switch("--dry-run")],
    unknown: Unknown::ArgumentUsage(CONNECT_USAGE),
    operands: Operands::Max(1, CONNECT_USAGE),
    ..STATUS
};

pub(super) const REMOTE_ADD: Spec = Spec {
    flags: &[switch("--dry-run")],
    unknown: Unknown::ArgumentUsage(REMOTE_ADD_USAGE),
    operands: Operands::Max(1, REMOTE_ADD_USAGE),
    ..STATUS
};

pub(super) const OPEN: Spec = Spec {
    flags: &[],
    unknown: Unknown::ArgumentUsage(OPEN_USAGE),
    operands: Operands::Max(1, OPEN_USAGE),
    ..STATUS
};

pub(super) const COMMIT: Spec = Spec {
    flags: &[value("--message").alias(&["-m"]), switch("--dry-run")],
    unknown: Unknown::ArgumentUsage(COMMIT_USAGE),
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

pub fn open(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = OPEN.parse(rest)?;
    let _paths = enter_paths(host, None);
    let repo = remote::locate()?;
    let data = remote::open_target(&repo, flags.operands().first().map(String::as_str))?;
    let human = format!("{}\n", data["path"].as_str().unwrap_or(""));
    Ok(Output::new(data, human))
}

pub fn connect(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CONNECT.parse(rest)?;
    let [url] = flags.operands() else {
        return Err(CtlError::usage(format!("missing url\n{CONNECT_USAGE}")));
    };
    let _paths = enter_paths(host, None);
    let data = connect::connect(url, flags.one("--clone-to"), flags.on("--dry-run"))?;
    let human = if data["dryRun"] == Value::Bool(true) {
        plan_text(&data)
    } else {
        format!(
            "Connected {} as the library at {}\n",
            data["remote"].as_str().unwrap_or(""),
            data["repo"].as_str().unwrap_or("")
        )
    };
    Ok(Output::new(data, human))
}

pub fn remote_add(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = REMOTE_ADD.parse(rest)?;
    let [url] = flags.operands() else {
        return Err(CtlError::usage(format!("missing url\n{REMOTE_ADD_USAGE}")));
    };
    let _paths = enter_paths(host, None);
    let repo = remote::locate()?;
    let data = connect::remote_add(&repo, url, flags.on("--dry-run"))?;
    let human = if data["dryRun"] == Value::Bool(true) {
        plan_text(&data)
    } else {
        format!(
            "Added {} as origin; `library push` sends the library there\n",
            data["remote"].as_str().unwrap_or("")
        )
    };
    Ok(Output::new(data, human))
}

pub fn commit(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = COMMIT.parse(rest)?;
    if let Some(extra) = flags.operands().first() {
        return Err(CtlError::usage(format!(
            "unexpected argument: {extra}\n{COMMIT_USAGE}"
        )));
    }
    let Some(message) = flags.one("--message") else {
        return Err(CtlError::usage(format!(
            "the commit needs a message\n{COMMIT_USAGE}"
        )));
    };
    let _paths = enter_paths(host, None);
    let repo = remote::locate()?;
    let data = connect::commit(&repo, message, flags.on("--dry-run"))?;
    let human = if data["dryRun"] == Value::Bool(true) && data["plan"].is_object() {
        plan_text(&data)
    } else if data["committed"] == Value::Bool(true) {
        format!(
            "Committed {} file(s) as {}\nundo: {}\n",
            data["files"].as_array().map_or(0, Vec::len),
            data["commitSha"].as_str().unwrap_or(""),
            data["result"]["undo"].as_str().unwrap_or("")
        )
    } else {
        format!(
            "{}\n",
            data["message"].as_str().unwrap_or("Nothing to commit")
        )
    };
    Ok(Output::new(data, human))
}
