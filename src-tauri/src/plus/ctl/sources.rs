//! `toolportctl sources ls|root`: where every skill, command, agent, rule and CLAUDE.md comes from
//! (D-063). Thin renderers over `plus::sources`.

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::context::{ContextConfig, Roots};
use crate::plus::sources::model::KINDS;
use crate::plus::sources::{self, roots as source_roots, ScanOptions};
use serde_json::Value;
use std::path::PathBuf;

const GROUP_USAGE: &str = "usage: sources ls|root (ls: [--source <id>] [--kind skill|command|agent|rule|memory] [--items] [--cwd <dir>] [--root <dir>]... [--deep] [--refresh]; root: ls | add <dir> [--dry-run] | rm <dir> [--dry-run])";
const ROOT_USAGE: &str = "usage: sources root ls|add|rm (add|rm: <dir> [--dry-run])";
const CHANGE_USAGE: &str = "usage: sources root add|rm <dir> [--dry-run]";

pub(super) const LS: Spec = Spec {
    flags: &[
        value("--source"),
        value("--kind"),
        switch("--items"),
        value("--cwd"),
        value("--root"),
        switch("--deep"),
        switch("--refresh"),
        value("--budget"),
    ],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const ROOT_LS: Spec = Spec { flags: &[], ..LS };

pub(super) const ROOT_CHANGE: Spec = Spec {
    flags: &[switch("--dry-run")],
    operands: Operands::Max(1, CHANGE_USAGE),
    ..LS
};

pub(super) fn world() -> Result<(Roots, ContextConfig), CtlError> {
    sources::host_world()
        .ok_or_else(|| CtlError::failed("no_home", "home directory could not be resolved"))
}

fn budgets(raw: &[String]) -> Result<Vec<(String, u64)>, CtlError> {
    raw.iter()
        .map(|entry| {
            entry
                .split_once('=')
                .and_then(|(name, ms)| Some((name.to_string(), ms.parse::<u64>().ok()?)))
                .ok_or_else(|| CtlError::usage("--budget needs <detector>=<milliseconds>"))
        })
        .collect()
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let kind = flags.one("--kind").map(str::to_string);
    if let Some(kind) = &kind {
        if !KINDS.contains(&kind.as_str()) {
            return Err(CtlError::usage(format!(
                "--kind must be one of {}",
                KINDS.join(", ")
            )));
        }
    }
    let opts = ScanOptions {
        source: flags.one("--source").map(str::to_string),
        kind,
        items: flags.on("--items"),
        cwd: flags.one("--cwd").map(PathBuf::from),
        roots: flags.all("--root").into_iter().map(PathBuf::from).collect(),
        deep: flags.on("--deep"),
        refresh: flags.on("--refresh"),
        budgets_ms: budgets(&flags.all("--budget"))?,
        max_entries: None,
    };
    let report = sources::scan_host(&opts)
        .ok_or_else(|| CtlError::failed("no_home", "home directory could not be resolved"))?;
    Ok(Output::new(report.to_json(opts.items), report.to_text()))
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn root_group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(ROOT_USAGE))
}

pub fn root_ls(rest: &[String]) -> Result<Output, CtlError> {
    ROOT_LS.parse(rest)?;
    let (roots, config) = world()?;
    let data = source_roots::list(&roots, &config);
    let mut human = String::new();
    for row in data["roots"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{:<8} {}{}\n",
            row["origin"].as_str().unwrap_or(""),
            row["path"].as_str().unwrap_or(""),
            if row["exists"] == Value::Bool(true) {
                ""
            } else {
                "  (missing)"
            }
        ));
    }
    Ok(Output::new(data, human))
}

fn change(rest: &[String], kind: source_roots::Change) -> Result<Output, CtlError> {
    let flags = ROOT_CHANGE.parse(rest)?;
    let dir = flags.single(CHANGE_USAGE)?;
    let (roots, _) = world()?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| roots.home.clone());
    let data = source_roots::change(&roots, kind, dir, &cwd, flags.on("--dry-run"))?;
    let mut human = format!("{}\n", data["plan"]["summary"].as_str().unwrap_or(""));
    if data["dryRun"] == Value::Bool(true) {
        human.push_str("(dry run, nothing written)\n");
    } else {
        human.push_str(&format!(
            "undo: {}\n",
            data["result"]["undo"].as_str().unwrap_or("")
        ));
    }
    Ok(Output::new(data, human))
}

pub fn root_add(rest: &[String]) -> Result<Output, CtlError> {
    change(rest, source_roots::Change::Add)
}

pub fn root_rm(rest: &[String]) -> Result<Output, CtlError> {
    change(rest, source_roots::Change::Remove)
}
