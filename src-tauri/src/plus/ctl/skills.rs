//! `toolportctl skills sync|ls|lint|diff`: thin renderers over the `plus.skills.*` handlers.
//! `init|add|audit|bundle|unbundle` live in `skills_repo.rs`.

use super::output::{CtlError, Output};
use serde_json::{json, Value};

const USAGE: &str = "usage: skills init|add|ls|lint|audit|bundle|unbundle|sync|diff \
     (sync|ls|lint|diff: [--repo <dir>] [--home <dir>]; sync: [--client <key>]... [--project] \
     [--dry-run]; lint: [--name <skill>]...; init: [--path <dir>] [--name <name>] [--dry-run]; \
     add: <name> [--type skill|rule] [--path <dir>] [--with-progressive] [--dry-run]; \
     audit: [--path <dir>]; bundle: [--output <zip>] [--path <dir>] [--skills <a,b>] [--dry-run]; \
     unbundle: <bundle.zip> [--path <dir>] [--dry-run])";

#[derive(Default)]
struct Flags {
    repo: Option<String>,
    home: Option<String>,
    clients: Vec<String>,
    names: Vec<String>,
    project: bool,
    dry_run: bool,
}

fn parse(rest: &[String], allowed: &[&str]) -> Result<Flags, CtlError> {
    let mut flags = Flags::default();
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        if !allowed.contains(&key) {
            return Err(CtlError::usage(format!("unexpected argument: {arg}")));
        }
        if matches!(key, "--project" | "--dry-run") {
            if inline.is_some() {
                return Err(CtlError::usage(format!("{key} takes no value")));
            }
            if key == "--project" {
                flags.project = true;
            } else {
                flags.dry_run = true;
            }
            continue;
        }
        let value = inline
            .or_else(|| iter.next().cloned())
            .ok_or_else(|| CtlError::usage(format!("{key} requires a value")))?;
        match key {
            "--repo" => flags.repo = Some(value),
            "--home" => flags.home = Some(value),
            "--client" => flags.clients.push(value),
            _ => flags.names.push(value),
        }
    }
    Ok(flags)
}

impl Flags {
    fn apply_home(&self) {
        if let Some(home) = &self.home {
            std::env::set_var("HOME", home);
            std::env::set_var("USERPROFILE", home);
        }
    }

    fn args(&self) -> Value {
        let mut args = json!({});
        if let Some(repo) = &self.repo {
            args["repo_path"] = json!(repo);
        }
        args
    }
}

fn call(command: &str, args: Value) -> Result<Value, CtlError> {
    crate::plus::dispatch(command, args).map_err(|e| CtlError::new("skills", e))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(USAGE))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let flags = parse(
        rest,
        &["--repo", "--home", "--client", "--project", "--dry-run"],
    )?;
    flags.apply_home();
    let mut args = flags.args();
    args["dry_run"] = json!(flags.dry_run);
    args["global_mode"] = json!(!flags.project);
    if !flags.clients.is_empty() {
        args["client_keys"] = json!(flags.clients);
    }
    let data = call("plus.skills.sync", args)?;
    let cleaned = data["cleaned"].as_array().map_or(0, Vec::len);
    let human = format!(
        "{} {} skill(s), {} rule(s) into {} ({} stale removed)",
        if flags.dry_run {
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
    let flags = parse(rest, &["--repo", "--home"])?;
    flags.apply_home();
    let data = call("plus.skills.list", flags.args())?;
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
    let flags = parse(rest, &["--repo", "--home", "--name"])?;
    flags.apply_home();
    let mut args = flags.args();
    if !flags.names.is_empty() {
        args["names"] = json!(flags.names);
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
    let flags = parse(rest, &["--repo", "--home"])?;
    flags.apply_home();
    let data = call("plus.skills.diff", flags.args())?;
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
