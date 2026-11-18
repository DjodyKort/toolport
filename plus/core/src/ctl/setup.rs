//! `toolportctl setup parts|export|inspect|restore|undo|check`: the setup file (D-144). Thin
//! renderers over `crate::setup`, which the `setup_*` self-management tools call as well. The
//! passphrase of a sealed part comes in on stdin and nowhere else.

use std::io::BufRead;
use std::path::PathBuf;

use serde_json::Value;
use toolport_plus_api::Host;

use super::flags::{greedy, switch, value, Flags, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use super::skills::enter_paths;
use crate::setup::export::{self, Options};
use crate::setup::{check, inspect, restore};
use crate::setup::{parse_parts, Ctx, PartId};

pub(super) const PARTS: Spec = Spec {
    flags: &[greedy("--roots").needs("a folder")],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const EXPORT: Spec = Spec {
    flags: &[
        value("--parts").needs("a list of parts"),
        value("--out").needs("a file or a folder"),
        value("--name").needs("a file name"),
        switch("--secrets"),
        switch("--sessions"),
        switch("--passphrase-stdin"),
        greedy("--roots").needs("a folder"),
        switch("--no-worktrees"),
        switch("--skip-no-remote"),
        switch("--library-zip"),
        value("--without").needs("a list of sub-choices"),
        value("--session-repo").needs("a repository name"),
        value("--peek").needs("a part"),
        switch("--dry-run"),
    ],
    ..PARTS
};

const INSPECT_USAGE: &str = "usage: setup inspect <file>";
const RESTORE_USAGE: &str = "usage: setup restore <file> [--parts <list>] [--take <row>] [--passphrase-stdin] [--apply --plan <hash>]";
const UNDO_USAGE: &str = "usage: setup undo [<run>] [--dry-run]";

pub(super) const INSPECT: Spec = Spec {
    flags: &[],
    operands: Operands::Max(1, INSPECT_USAGE),
    ..PARTS
};

pub(super) const RESTORE: Spec = Spec {
    flags: &[
        value("--parts").needs("a list of parts"),
        greedy("--take").needs("a row id"),
        switch("--passphrase-stdin"),
        switch("--apply"),
        value("--plan").needs("the plan hash of the dry run"),
    ],
    operands: Operands::Max(1, RESTORE_USAGE),
    ..PARTS
};

pub(super) const UNDO: Spec = Spec {
    flags: &[switch("--dry-run")],
    operands: Operands::Max(1, UNDO_USAGE),
    ..PARTS
};

pub(super) const CHECK: Spec = Spec {
    flags: &[
        greedy("--roots").needs("a folder"),
        greedy("--tick").needs("an item"),
        greedy("--untick").needs("an item"),
        greedy("--ack").needs("a finding id"),
    ],
    ..PARTS
};

fn setup_error(why: impl std::fmt::Display) -> CtlError {
    CtlError::failed("setup", why.to_string())
}

fn kb(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{bytes} B")
    }
}

fn roots_of(flags: &Flags) -> Vec<PathBuf> {
    flags
        .all("--roots")
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

pub fn parts(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PARTS.parse(rest)?;
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(setup_error)?;
    let data = export::list(&ctx, &roots_of(&flags))?;
    let mut human = format!(
        "{} repositories under {}\n",
        data["repos"].as_array().map_or(0, Vec::len),
        data["roots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    );
    for part in data["parts"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "  {:<24} {:>5}  {:>9}  {}\n",
            part["label"].as_str().unwrap_or(""),
            part["count"],
            kb(part["fileSize"].as_u64().unwrap_or(0)),
            if part["sealed"] == true {
                "sealed, off by default"
            } else {
                ""
            },
        ));
    }
    Ok(Output::new(data, human))
}

fn read_passphrase(input: &mut dyn BufRead) -> Result<String, CtlError> {
    let mut line = String::new();
    input
        .read_line(&mut line)
        .map_err(|e| CtlError::failed("read", format!("could not read the passphrase: {e}")))?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

fn options_of(flags: &Flags) -> Result<Options, CtlError> {
    let mut opts = Options::default();
    if let Some(text) = flags.one("--parts") {
        opts.parts = parse_parts(text).map_err(CtlError::usage)?;
    }
    if flags.on("--secrets") {
        opts.parts.insert(PartId::Secrets);
    }
    if flags.on("--sessions") {
        opts.parts.insert(PartId::History);
    }
    opts.roots = roots_of(flags);
    opts.worktrees = !flags.on("--no-worktrees");
    opts.skip_no_remote = flags.on("--skip-no-remote");
    opts.library_zip = flags.on("--library-zip");
    opts.dry_run = flags.on("--dry-run");
    opts.out = flags.one("--out").map(PathBuf::from);
    opts.name = flags.one("--name").map(String::from);
    for list in flags.all("--without") {
        for sub in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if !export::valid_sub(sub) {
                return Err(CtlError::usage(format!(
                    "--without does not know {sub:?}: use {}",
                    [export::SUBS.as_slice(), export::SUBS_EXTRA.as_slice()]
                        .concat()
                        .join(", ")
                )));
            }
            opts.without.insert(sub.to_string());
        }
    }
    opts.session_repos = flags.all("--session-repo");
    if let Some(text) = flags.one("--peek") {
        let id = PartId::parse(text)
            .ok_or_else(|| CtlError::usage(format!("--peek does not know {text:?}")))?;
        if !opts.dry_run {
            return Err(CtlError::usage(
                "--peek goes with --dry-run: it shows a part and writes nothing",
            ));
        }
        opts.parts.insert(id);
        opts.peek = Some(id);
    }
    Ok(opts)
}

pub fn export(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    export_from(host, rest, &mut std::io::stdin().lock())
}

pub fn export_from(
    host: &dyn Host,
    rest: &[String],
    input: &mut dyn BufRead,
) -> Result<Output, CtlError> {
    let flags = EXPORT.parse(rest)?;
    let opts = options_of(&flags)?;
    let passphrase = if flags.on("--passphrase-stdin") {
        Some(read_passphrase(input)?)
    } else {
        None
    };
    if opts.needs_passphrase() && !opts.dry_run && passphrase.is_none() {
        return Err(CtlError::usage(
            "secrets and sessions are sealed with a passphrase: pass --passphrase-stdin and write it on stdin",
        ));
    }
    if !opts.dry_run && opts.out.is_none() {
        return Err(CtlError::usage(
            "--out <file or folder> says where to write the file",
        ));
    }
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(setup_error)?;
    let data = export::run(&ctx, &opts, passphrase.as_deref())?;
    let human = export_text(&data);
    Ok(Output::new(data, human))
}

fn export_text(data: &Value) -> String {
    if data.get("peek").is_some() {
        return serde_json::to_string_pretty(&data["peek"]).unwrap_or_default() + "\n";
    }
    let mut out = String::new();
    for row in data["tree"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<22} {:>9}{}\n",
            row["name"].as_str().unwrap_or(""),
            kb(row["fileSize"].as_u64().unwrap_or(0)),
            if row["sealed"] == true {
                "  sealed"
            } else {
                ""
            },
        ));
    }
    if data["dryRun"] == true {
        out.push_str("(dry run, nothing written)\n");
    } else {
        out.push_str(&format!(
            "wrote {} ({}), verified\n",
            data["file"].as_str().unwrap_or(""),
            kb(data["fileSize"].as_u64().unwrap_or(0))
        ));
    }
    out.push_str("not in the file:\n");
    for line in data["notIncluded"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        out.push_str(&format!("  {line}\n"));
    }
    out
}

pub fn inspect(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = INSPECT.parse(rest)?;
    let _paths = enter_paths(host, None);
    let data = inspect::run(std::path::Path::new(flags.single(INSPECT_USAGE)?))?;
    let human = serde_json::to_string_pretty(&data).unwrap_or_default() + "\n";
    Ok(Output::new(data, human))
}

pub fn restore(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    restore_from(host, rest, &mut std::io::stdin().lock())
}

pub fn restore_from(
    host: &dyn Host,
    rest: &[String],
    input: &mut dyn BufRead,
) -> Result<Output, CtlError> {
    let flags = RESTORE.parse(rest)?;
    let mut opts = restore::RestoreOptions {
        file: PathBuf::from(flags.single(RESTORE_USAGE)?),
        apply: flags.on("--apply"),
        plan: flags.one("--plan").map(String::from),
        take: flags.all("--take").into_iter().collect(),
        ..Default::default()
    };
    if let Some(text) = flags.one("--parts") {
        opts.parts = Some(parse_parts(text).map_err(CtlError::usage)?);
    }
    if flags.on("--passphrase-stdin") {
        opts.passphrase = Some(read_passphrase(input)?);
    }
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(setup_error)?;
    let data = restore::run(&ctx, &opts)?;
    let human = rows_text(&data);
    Ok(Output::new(data, human))
}

pub fn undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNDO.parse(rest)?;
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(setup_error)?;
    let run = flags.operands().first().map(String::as_str);
    let data = restore::undo(&ctx, run, flags.on("--dry-run"))?;
    let human = serde_json::to_string_pretty(&data).unwrap_or_default() + "\n";
    Ok(Output::new(data, human))
}

pub fn check(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CHECK.parse(rest)?;
    let _paths = enter_paths(host, None);
    let ctx = Ctx::new(host).map_err(setup_error)?;
    let strings = |name: &str| flags.all(name);
    let data = check::run(
        &ctx,
        &check::Options {
            roots: roots_of(&flags),
            tick: strings("--tick"),
            untick: strings("--untick"),
            ack: strings("--ack"),
        },
    )?;
    let mut human = format!(
        "{} repositories, {} blockers, {} to weigh\n",
        data["repos"],
        data["blockers"].as_array().map_or(0, Vec::len),
        data["amber"].as_array().map_or(0, Vec::len)
    );
    for group in ["blockers", "amber"] {
        for f in data[group].as_array().into_iter().flatten() {
            human.push_str(&format!(
                "  [{group}] {}: {}\n",
                f["repo"].as_str().unwrap_or(""),
                f["note"].as_str().unwrap_or("")
            ));
        }
    }
    Ok(Output::new(data, human))
}

fn rows_text(data: &Value) -> String {
    let mut out = String::new();
    for row in data["rows"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<9} {:<40} {}\n",
            row["state"].as_str().unwrap_or(""),
            row["label"].as_str().unwrap_or(""),
            row["note"].as_str().unwrap_or("")
        ));
    }
    for step in data["steps"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<4} {:<40} {}\n",
            if step["ok"] == true { "ok" } else { "FAIL" },
            step["label"].as_str().unwrap_or(""),
            step["note"].as_str().unwrap_or("")
        ));
    }
    if data["dryRun"] == true {
        out.push_str(&format!(
            "(dry run) apply with --apply --plan {}\n",
            data["plan"].as_str().unwrap_or("")
        ));
    }
    out
}
