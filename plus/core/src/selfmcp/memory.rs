//! `memory_*` and `sessions_*`: the same operations as `toolportctl memory` and `sessions`, over
//! `crate::memory::ops`. Reads answer at tier 1. `memory_set` and the undo tools write at tier 2
//! and preview unless `dry_run=false`. `memory_clear` and `sessions_clear` only plan: an agent does
//! not clear memory or sessions (D-145), the person does it from the app.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, flag_or, str_arg, str_trimmed};
use crate::memory::ops::{self as api, ClearOpts, SessionsClear};
use crate::memory::store::Dest;
use crate::memory::switch::Position;

type Outcome = Result<Value, ToolError>;

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::new("invalid_arguments", message)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_trimmed(args, key).ok_or_else(|| invalid(format!("{key} is required")))
}

pub(super) fn ls(host: &dyn Host, args: &Value) -> Outcome {
    let all = flag(args, "all");
    let cwd = if all {
        None
    } else {
        Some(required(args, "cwd")?)
    };
    Ok(api::ls(host, cwd, all)?)
}

pub(super) fn status(host: &dyn Host, args: &Value) -> Outcome {
    Ok(api::status(host, Some(required(args, "cwd")?))?)
}

pub(super) fn set(host: &dyn Host, args: &Value) -> Outcome {
    let position = Position::parse(required(args, "position")?)
        .ok_or_else(|| invalid("position must be default, on or off"))?;
    let global = flag(args, "global");
    let cwd = str_trimmed(args, "cwd");
    if global == cwd.is_some() {
        return Err(invalid("give either cwd or global=true"));
    }
    Ok(api::set(
        host,
        cwd,
        global,
        position,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn clear(host: &dyn Host, args: &Value) -> Outcome {
    Ok(api::clear(
        host,
        Some(required(args, "cwd")?),
        &ClearOpts {
            forever: false,
            confirm: None,
            also_off: flag(args, "also_off"),
            dry_run: true,
        },
    )?)
}

pub(super) fn undo(host: &dyn Host, args: &Value) -> Outcome {
    Ok(api::undo(
        host,
        str_trimmed(args, "id"),
        str_trimmed(args, "cwd"),
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn overview(host: &dyn Host, args: &Value) -> Outcome {
    let all = flag(args, "all");
    let cwd = if all {
        None
    } else {
        Some(required(args, "cwd")?)
    };
    Ok(api::overview(
        host,
        cwd,
        all,
        str_trimmed(args, "sort").unwrap_or("name"),
        str_trimmed(args, "filter"),
    )?)
}

pub(super) fn sessions_ls(host: &dyn Host, args: &Value) -> Outcome {
    let all = flag(args, "all");
    let cwd = if all {
        None
    } else {
        Some(required(args, "cwd")?)
    };
    Ok(api::sessions_ls(host, cwd, all)?)
}

pub(super) fn sessions_clear(host: &dyn Host, args: &Value) -> Outcome {
    let ids = crate::args::nonempty_strings(args, "ids").unwrap_or_default();
    let dest = match str_arg(args, "to") {
        None | Some("trash") => Dest::Trash,
        Some("archive") => Dest::Archive,
        Some(_) => return Err(invalid("to must be trash or archive")),
    };
    Ok(api::sessions_clear(
        host,
        Some(required(args, "cwd")?),
        &SessionsClear {
            ids,
            older_than: args.get("older_than_days").and_then(Value::as_u64),
            dest,
            confirm: None,
            dry_run: true,
        },
    )?)
}

pub(super) fn sessions_undo(host: &dyn Host, args: &Value) -> Outcome {
    Ok(api::sessions_undo(
        host,
        str_trimmed(args, "id"),
        str_trimmed(args, "cwd"),
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn sessions_restore(host: &dyn Host, args: &Value) -> Outcome {
    Ok(api::sessions_restore(
        host,
        required(args, "id")?,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn sessions_retention(host: &dyn Host, _args: &Value) -> Outcome {
    Ok(api::sessions_retention(host, None, true)?)
}
