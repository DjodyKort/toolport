//! `toolportctl skills sync|ls|lint|diff`: thin renderers over the `plus.skills.*` handlers.
//! `init|add|audit|bundle|unbundle` live in `skills_repo.rs`, `status|clean|uninstall|resolve` in
//! `skills_state.rs`.

use super::flags::{switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use serde_json::{json, Value};

const USAGE: &str = "usage: skills init|add|ls|lint|audit|bundle|unbundle|sync|diff|status|clean|uninstall|resolve \
     (sync|ls|lint|diff: [--repo <dir>] [--home <dir>]; sync: [--client <key>]... [--project] \
     [--dry-run]; lint: [--name <skill>]...; init: [--path <dir>] [--name <name>] [--dry-run]; \
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
};
const SYNC: Spec = Spec {
    flags: &[
        value("--repo"),
        value("--home"),
        value("--client"),
        switch("--project"),
        switch("--global"),
        switch("--dry-run"),
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
    crate::plus::dispatch(command, args).map_err(|e| CtlError::new("skills", e))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(USAGE))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SYNC.parse(rest)?;
    if flags.on("--project") && flags.on("--global") {
        return Err(CtlError::usage("--global and --project are mutually exclusive"));
    }
    apply_home(flags.one("--home"));
    let mut args = repo_args(&flags);
    args["dry_run"] = json!(flags.on("--dry-run"));
    args["global_mode"] = json!(!flags.on("--project"));
    let clients = flags.all("--client");
    if !clients.is_empty() {
        args["client_keys"] = json!(clients);
    }
    let data = call("plus.skills.sync", args)?;
    let cleaned = data["cleaned"].as_array().map_or(0, Vec::len);
    let human = format!(
        "{} {} skill(s), {} rule(s) into {} ({} stale removed)",
        if flags.on("--dry-run") {
            "would sync"
        } else {
            "synced"
        },
        data["skillCount"],
        data["ruleCount"],
        data["outputRoot"].as_str().unwrap_or(""),
        cleaned
    );
    Ok(Output::new(data, human))
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
