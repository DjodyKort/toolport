//! `toolportctl sync status|setup|push|pull|plan|undo`: the setup file kept in step between
//! machines (S12). Thin renderers over `crate::sync`, which the `sync_*` self-management tools
//! call as well. The passphrase comes in on stdin once, goes to the keychain and is never an
//! argument; nothing here prints it.

use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::PathBuf;

use serde_json::Value;
use toolport_plus_api::Host;

use super::flags::{greedy, switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use super::setup::{read_passphrase, rows_text};
use super::skills::enter_paths;
use crate::setup::{parse_parts, Ctx};
use crate::skills::git::SystemGit;
use crate::sync::engine::{PullOptions, PushOptions, SetupOptions};

const NONE: Spec = Spec {
    flags: &[],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const STATUS: Spec = NONE;

pub(super) const SETUP: Spec = Spec {
    flags: &[
        value("--git").needs("a repository URL"),
        value("--branch").needs("a branch name"),
        value("--folder").needs("a folder"),
        value("--parts").needs("a list of parts"),
        switch("--passphrase-stdin"),
        switch("--forget-passphrase"),
        switch("--enable"),
        switch("--disable"),
        value("--schedule").needs("on or off"),
        switch("--dry-run"),
    ],
    ..NONE
};

pub(super) const PUSH: Spec = Spec {
    flags: &[switch("--dry-run"), switch("--keep-mine")],
    ..NONE
};

pub(super) const PULL: Spec = Spec {
    flags: &[
        greedy("--take").needs("a row id"),
        switch("--apply"),
        value("--plan").needs("the plan hash of the dry run"),
        switch("--unattended"),
    ],
    ..NONE
};

pub(super) const PLAN: Spec = Spec {
    flags: &[greedy("--take").needs("a row id")],
    ..NONE
};

pub(super) const UNDO: Spec = Spec {
    flags: &[switch("--dry-run")],
    ..NONE
};

fn sync_error(why: impl std::fmt::Display) -> CtlError {
    CtlError::failed("sync", why.to_string())
}

fn status_text(data: &Value) -> String {
    if data["configured"] != true {
        return "sync is not set up: toolportctl sync setup --git <url> | --folder <path> --passphrase-stdin\n".into();
    }
    let transport = &data["transport"];
    let place = transport["url"]
        .as_str()
        .or(transport["path"].as_str())
        .unwrap_or("");
    let mut out = format!(
        "{} sync ({}) {}\n",
        if data["enabled"] == true {
            "on:"
        } else {
            "off:"
        },
        transport["kind"].as_str().unwrap_or(""),
        place
    );
    out.push_str(&format!(
        "  passphrase {}{}\n",
        data["passphrase"].as_str().unwrap_or(""),
        if data["paused"] == true {
            ", sync paused while the keyring is locked"
        } else {
            ""
        }
    ));
    for part in data["parts"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<26} {}\n",
            part["label"].as_str().unwrap_or(""),
            match (part["on"] == true, part["handledBy"].as_str()) {
                (_, Some(by)) => format!("handled by {by} Sync"),
                (true, _) => "on".into(),
                _ => "off".into(),
            }
        ));
    }
    if let Some(last) = data["lastSync"].as_object() {
        out.push_str(&format!(
            "  last {}: {} on {}\n",
            last["direction"].as_str().unwrap_or(""),
            last["at"].as_str().unwrap_or(""),
            last["machine"].as_str().unwrap_or("")
        ));
    }
    if let Some(pending) = data["pending"].as_array().filter(|p| !p.is_empty()) {
        out.push_str(&format!(
            "  {} rows wait for you: {}\n",
            pending.len(),
            pending
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

pub fn status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    STATUS.parse(rest)?;
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(sync_error)?;
    let data = crate::sync::status(&ctx)?;
    let human = status_text(&data);
    Ok(Output::new(data, human))
}

pub fn setup(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    setup_from(host, rest, &mut std::io::stdin().lock())
}

pub fn setup_from(
    host: &dyn Host,
    rest: &[String],
    input: &mut dyn BufRead,
) -> Result<Output, CtlError> {
    let flags = SETUP.parse(rest)?;
    if flags.on("--enable") && flags.on("--disable") {
        return Err(CtlError::usage("--enable and --disable exclude each other"));
    }
    let schedule = match flags.one("--schedule") {
        None => None,
        Some("on") => Some(true),
        Some("off") => Some(false),
        Some(other) => {
            return Err(CtlError::usage(format!(
                "--schedule is on or off, not {other:?}"
            )))
        }
    };
    let parts: Option<BTreeSet<_>> = match flags.one("--parts") {
        Some(text) => Some(parse_parts(text).map_err(CtlError::usage)?),
        None => None,
    };
    let passphrase = if flags.on("--passphrase-stdin") {
        Some(read_passphrase(input)?)
    } else {
        None
    };
    let opts = SetupOptions {
        git_url: flags.one("--git").map(String::from),
        branch: flags.one("--branch").map(String::from),
        folder: flags.one("--folder").map(PathBuf::from),
        parts,
        passphrase,
        enabled: match (flags.on("--enable"), flags.on("--disable")) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        },
        schedule,
        forget_passphrase: flags.on("--forget-passphrase"),
        dry_run: flags.on("--dry-run"),
    };
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(sync_error)?;
    let data = crate::sync::setup(&ctx, &opts)?;
    let human = status_text(&data);
    Ok(Output::new(data, human))
}

pub fn push(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PUSH.parse(rest)?;
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(sync_error)?;
    let data = crate::sync::push(
        &ctx,
        &PushOptions {
            dry_run: flags.on("--dry-run"),
            keep_mine: flags.on("--keep-mine"),
        },
        &SystemGit,
    )?;
    let human = if data["pushed"] == true {
        format!(
            "pushed ({} bytes), revision {}\n",
            data["fileSize"],
            data["revision"].as_str().unwrap_or("")
        )
    } else {
        "(dry run, nothing pushed)\n".to_string()
    };
    Ok(Output::new(data, human))
}

fn pull_with(host: &dyn Host, opts: &PullOptions) -> Result<Output, CtlError> {
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(sync_error)?;
    let data = crate::sync::pull(&ctx, opts, &SystemGit)?;
    let human = if data["skipped"] == true {
        format!("skipped: {}\n", data["reason"].as_str().unwrap_or(""))
    } else if data["upToDate"] == true {
        "nothing new on the remote\n".to_string()
    } else if data["empty"] == true {
        format!("{}\n", data["note"].as_str().unwrap_or(""))
    } else {
        rows_text(&data)
    };
    Ok(Output::new(data, human))
}

pub fn pull(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PULL.parse(rest)?;
    pull_with(
        host,
        &PullOptions {
            take: flags.all("--take").into_iter().collect(),
            apply: flags.on("--apply"),
            plan: flags.one("--plan").map(String::from),
            unattended: flags.on("--unattended"),
            no_prompt: false,
        },
    )
}

pub fn plan(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PLAN.parse(rest)?;
    pull_with(
        host,
        &PullOptions {
            take: flags.all("--take").into_iter().collect(),
            ..Default::default()
        },
    )
}

pub fn undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNDO.parse(rest)?;
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(sync_error)?;
    let data = crate::sync::undo(&ctx, flags.on("--dry-run"))?;
    let human = serde_json::to_string_pretty(&data).unwrap_or_default() + "\n";
    Ok(Output::new(data, human))
}
