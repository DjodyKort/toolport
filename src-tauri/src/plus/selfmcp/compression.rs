//! `compression_*` tools: thin adapters over the `plus::compression::manage` cores that
//! `toolportctl compression enable|disable|set-provider|use|sync|seal` run, and over the status
//! core behind `toolportctl compression status`. Every writing tool defaults `dry_run` to true.
//! `teardown`, `pin --install` and `--mcpm-root` are not exposed: they drive the engine or read
//! a caller-chosen directory.

use super::ToolError;
use crate::plus::args::{flag_or, str_nonempty};
use crate::plus::compression::manage::{
    self, parse_mode, parse_provider, parse_telemetry, with_system, CmdError, EnableReq, Kind,
    SealReq,
};
use serde_json::Value;

type Outcome = Result<Value, ToolError>;

impl From<CmdError> for ToolError {
    fn from(error: CmdError) -> Self {
        match error.kind {
            Kind::Usage => ToolError::new("invalid_arguments", error.message),
            Kind::Failed(_) => ToolError::backend(error.message),
        }
    }
}

fn dry_run(args: &Value) -> bool {
    flag_or(args, "dry_run", true)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_nonempty(args, key)
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

fn port(args: &Value) -> Result<Option<u16>, ToolError> {
    let Some(value) = args.get("port").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    value
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .map(Some)
        .ok_or_else(|| ToolError::new("invalid_arguments", "port must be between 1 and 65535"))
}

pub(super) fn status(_args: &Value) -> Outcome {
    with_system(|cx| Ok(manage::status(cx)?.data))
}

pub(super) fn enable(args: &Value) -> Outcome {
    let req = EnableReq {
        provider: str_nonempty(args, "provider")
            .map(parse_provider)
            .transpose()?,
        port: port(args)?,
        telemetry: str_nonempty(args, "telemetry")
            .map(parse_telemetry)
            .transpose()?,
        preset: str_nonempty(args, "preset").map(String::from),
        mode: str_nonempty(args, "mode").map(parse_mode).transpose()?,
        dry_run: dry_run(args),
    };
    with_system(|cx| Ok(manage::enable(cx, &req)?))
}

pub(super) fn disable(args: &Value) -> Outcome {
    with_system(|cx| Ok(manage::disable(cx, false, dry_run(args))?))
}

pub(super) fn set_provider(args: &Value) -> Outcome {
    let provider = parse_provider(required(args, "provider")?)?;
    with_system(|cx| Ok(manage::set_provider(cx, provider, dry_run(args))?))
}

pub(super) fn use_preset(args: &Value) -> Outcome {
    let name = required(args, "preset")?;
    with_system(|cx| Ok(manage::use_preset(cx, name, dry_run(args))?))
}

pub(super) fn sync(args: &Value) -> Outcome {
    with_system(|cx| Ok(manage::sync(cx, None, dry_run(args))?))
}

pub(super) fn seal(args: &Value) -> Outcome {
    let req = SealReq {
        preset: str_nonempty(args, "preset").map(String::from),
        apply: true,
        dry_run: dry_run(args),
    };
    with_system(|cx| Ok(manage::seal(cx, &req)?))
}
