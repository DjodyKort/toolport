//! `setup_parts` (tier 1), `setup_export` (tier 3) and the other `setup_*` tools: the same
//! operations as `toolportctl setup ...`, over `crate::setup`. A sealed part needs a passphrase,
//! and a passphrase in a tool argument would sit in a transcript, so these tools never export
//! secrets, never write sessions and never apply a restore: a person does that in the app. The
//! `sessions` part only lists the repositories that have saved conversations, after the person
//! approves that call.

use std::path::PathBuf;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::{approval, ToolError};
use crate::args::{flag, flag_or, nonempty_strings, str_nonempty};
use crate::op::{ErrorKind, OpError};
use crate::setup::export::{self, Options};
use crate::setup::{parse_parts, Ctx, PartId};

const SESSIONS_PREVIEW_ONLY: &str = "the sessions are sealed with a passphrase that only a person types, so this tool only lists them; a person writes the file with toolportctl setup export --sessions --passphrase-stdin";
const PERSON_ONLY: &str = "secrets and sessions are sealed with a passphrase and only a person exports them, in the app or with toolportctl";

fn tool_error(error: OpError) -> ToolError {
    match error.kind {
        ErrorKind::Conflict => ToolError::new("conflict", error.message),
        ErrorKind::Failed("refused") => ToolError::new("refused", error.message),
        _ => ToolError::from(error),
    }
}

pub(super) fn with_ctx<R>(
    host: &dyn Host,
    run: impl FnOnce(&Ctx<'_>) -> Result<R, OpError>,
) -> Result<R, ToolError> {
    let _guard = crate::fsx::enter(crate::fsx::Paths {
        home: crate::fsx::home(),
        data_dir: host.data_dir(),
    });
    let ctx = Ctx::new(host).map_err(|e| ToolError::new("not_found", e))?;
    run(&ctx).map_err(tool_error)
}

fn roots(args: &Value) -> Vec<PathBuf> {
    nonempty_strings(args, "roots")
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

pub(super) fn parts(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_ctx(host, |ctx| export::list(ctx, &roots(args)))
}

pub(super) fn export(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let mut opts = Options::default();
    if let Some(text) = str_nonempty(args, "parts") {
        opts.parts = parse_parts(text).map_err(|e| ToolError::new("invalid_arguments", e))?;
    }
    if opts.parts.iter().any(|p| p.sealed()) {
        return Err(ToolError::new("refused", PERSON_ONLY));
    }
    let sessions = flag(args, "sessions");
    let session_repos = nonempty_strings(args, "session_repo").unwrap_or_default();
    if !session_repos.is_empty() && !sessions {
        return Err(ToolError::new(
            "invalid_arguments",
            "session_repo goes with sessions",
        ));
    }
    if sessions {
        opts.parts.insert(PartId::History);
        opts.session_repos = session_repos;
    }
    opts.roots = roots(args);
    opts.worktrees = !flag(args, "no_worktrees");
    opts.skip_no_remote = flag(args, "skip_no_remote");
    opts.library_zip = flag(args, "library_zip");
    opts.dry_run = flag_or(args, "dry_run", true);
    opts.out = str_nonempty(args, "out").map(PathBuf::from);
    opts.name = str_nonempty(args, "name").map(String::from);
    for sub in nonempty_strings(args, "without").unwrap_or_default() {
        if !export::valid_sub(&sub) {
            return Err(ToolError::new(
                "invalid_arguments",
                format!("without does not know {sub:?}"),
            ));
        }
        opts.without.insert(sub);
    }
    if let Some(text) = str_nonempty(args, "peek") {
        let id = PartId::parse(text)
            .filter(|id| !id.sealed())
            .ok_or_else(|| {
                ToolError::new("invalid_arguments", format!("peek does not know {text:?}"))
            })?;
        opts.parts.insert(id);
        opts.peek = Some(id);
        if !opts.dry_run {
            return Err(ToolError::new(
                "invalid_arguments",
                "peek goes with dry_run",
            ));
        }
    }
    if sessions && opts.peek.is_some() {
        return Err(ToolError::new(
            "invalid_arguments",
            "peek does not go with sessions",
        ));
    }
    if sessions && !opts.dry_run {
        return Err(ToolError::new("refused", SESSIONS_PREVIEW_ONLY));
    }
    if !opts.dry_run && opts.out.is_none() {
        return Err(ToolError::new(
            "invalid_arguments",
            "out is needed to write the file",
        ));
    }
    let mut data = with_ctx(host, |ctx| export::run(ctx, &opts, None))?;
    if sessions {
        approve_sessions_listing(host, &data)?;
        data["plan"]["warnings"] = json!([SESSIONS_PREVIEW_ONLY]);
    }
    Ok(data)
}

fn approve_sessions_listing(host: &dyn Host, data: &Value) -> Result<(), ToolError> {
    let part = data["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|part| part["id"] == PartId::History.id());
    let mut steps = vec![
        "list the repositories that have saved conversations, with their counts and sizes; no conversation text is read or shown"
            .to_string(),
    ];
    for repo in part
        .and_then(|part| part["info"]["repos"].as_array())
        .into_iter()
        .flatten()
    {
        steps.push(format!(
            "{}: {} conversations, {} bytes",
            repo["name"].as_str().unwrap_or(""),
            repo["sessions"],
            repo["fileSize"]
        ));
    }
    approval::ask(host, "setup_export sessions", steps, json!([]))
}

pub(super) fn inspect(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let file = str_nonempty(args, "file")
        .ok_or_else(|| ToolError::new("invalid_arguments", "file is required"))?;
    with_ctx(host, |_| {
        crate::setup::inspect::run(std::path::Path::new(file))
    })
}

pub(super) fn check(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_ctx(host, |ctx| {
        crate::setup::check::run(
            ctx,
            &crate::setup::check::Options {
                roots: roots(args),
                tick: nonempty_strings(args, "tick").unwrap_or_default(),
                untick: nonempty_strings(args, "untick").unwrap_or_default(),
                ack: Vec::new(),
            },
        )
    })
}

pub(super) fn undo(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let run = str_nonempty(args, "run");
    let dry_run = flag_or(args, "dry_run", true);
    with_ctx(host, |ctx| crate::setup::restore::undo(ctx, run, dry_run))
}

pub(super) fn restore(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let file = str_nonempty(args, "file")
        .ok_or_else(|| ToolError::new("invalid_arguments", "file is required"))?;
    if !flag_or(args, "dry_run", true) {
        return Err(ToolError::new(
            "refused",
            "a restore is applied by a person in the app or with toolportctl; this tool only plans",
        ));
    }
    let mut opts = crate::setup::restore::RestoreOptions {
        file: PathBuf::from(file),
        take: nonempty_strings(args, "take")
            .unwrap_or_default()
            .into_iter()
            .collect(),
        ..Default::default()
    };
    if let Some(text) = str_nonempty(args, "parts") {
        let parts = parse_parts(text).map_err(|e| ToolError::new("invalid_arguments", e))?;
        if parts.iter().any(|p| p.sealed()) {
            return Err(ToolError::new("refused", PERSON_ONLY));
        }
        opts.parts = Some(parts);
    }
    with_ctx(host, |ctx| crate::setup::restore::run(ctx, &opts))
}
