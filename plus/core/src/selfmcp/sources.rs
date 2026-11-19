//! `sources_ls`: the read-only map of where skills, commands, agents, rules and CLAUDE.md files
//! come from, an adapter over `crate::sources` that `toolportctl sources ls` renders as well.

use super::{paths, ToolError};
use crate::args::{flag, flag_or, nonempty_strings, str_nonempty};
use crate::sources::model::KINDS;
use crate::sources::{self, scan_host, source_roots, ScanOptions};
use serde_json::Value;
use std::path::PathBuf;
use toolport_plus_api::Host;

pub(super) fn ls(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let kind = str_nonempty(args, "kind").map(String::from);
    if let Some(kind) = &kind {
        if !KINDS.contains(&kind.as_str()) {
            return Err(ToolError::new(
                "invalid_arguments",
                format!("kind must be one of {}", KINDS.join(", ")),
            ));
        }
    }
    let items = flag(args, "items");
    let _guard = crate::fsx::enter(crate::fsx::Paths {
        home: None,
        data_dir: host.data_dir(),
    });
    let report = scan_host(&ScanOptions {
        source: str_nonempty(args, "source").map(String::from),
        kind,
        items,
        cwd: str_nonempty(args, "cwd").map(PathBuf::from),
        roots: nonempty_strings(args, "root")
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        deep: flag(args, "deep"),
        refresh: flag(args, "refresh"),
        ..ScanOptions::default()
    })
    .ok_or_else(|| ToolError::new("not_found", "home directory unknown"))?;
    Ok(report.to_json(items))
}

fn guard(host: &dyn Host) -> crate::fsx::PathsGuard {
    crate::fsx::enter(crate::fsx::Paths {
        home: None,
        data_dir: host.data_dir(),
    })
}

fn no_home() -> ToolError {
    ToolError::new("not_found", "home directory unknown")
}

pub(super) fn root_ls(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    let _guard = guard(host);
    let (roots, places, config) = sources::host_world().ok_or_else(no_home)?;
    Ok(source_roots::list(&roots, &places, &config))
}

pub(super) fn root_set(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let kind = match str_nonempty(args, "action") {
        Some("add") => source_roots::Change::Add,
        Some("rm") => source_roots::Change::Remove,
        _ => return Err(ToolError::new("invalid_arguments", "action is add or rm")),
    };
    let dir = str_nonempty(args, "dir")
        .ok_or_else(|| ToolError::new("invalid_arguments", "dir is required"))?;
    let _guard = guard(host);
    let dir = paths::not_root_or_home(dir)?;
    let (roots, places, _) = sources::host_world().ok_or_else(no_home)?;
    Ok(source_roots::change(
        &roots,
        &places,
        kind,
        &dir.to_string_lossy(),
        &roots.home,
        flag_or(args, "dry_run", true),
    )?)
}
