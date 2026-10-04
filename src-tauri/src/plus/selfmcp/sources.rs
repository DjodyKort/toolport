//! `sources_ls`: the read-only map of where skills, commands, agents, rules and CLAUDE.md files
//! come from, an adapter over `plus::sources` that `toolportctl sources ls` renders as well.

use super::ToolError;
use crate::plus::args::{flag, str_nonempty};
use crate::plus::sources::model::KINDS;
use crate::plus::sources::{scan_host, ScanOptions};
use serde_json::Value;
use std::path::PathBuf;

pub(super) fn ls(args: &Value) -> Result<Value, ToolError> {
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
    let report = scan_host(&ScanOptions {
        source: str_nonempty(args, "source").map(String::from),
        kind,
        items,
        cwd: str_nonempty(args, "cwd").map(PathBuf::from),
        deep: flag(args, "deep"),
        refresh: flag(args, "refresh"),
        ..ScanOptions::default()
    })
    .ok_or_else(|| ToolError::new("not_found", "home directory unknown"))?;
    Ok(report.to_json(items))
}
