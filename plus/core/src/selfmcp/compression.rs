//! `compression_*` tools: thin adapters over the `compression::manage` cores that
//! `toolportctl compression enable|disable|set-provider|use|sync|seal` run, and over the status
//! core behind `toolportctl compression status`. Every writing tool defaults `dry_run` to true.
//! `disable` takes `teardown` and `sync` takes `mcpm_root`, both through the same cores as the
//! CLI flags; `pin --install` is not exposed: it drives the engine.

use super::ToolError;
use crate::args::{flag, flag_or, str_trimmed};
use crate::compression::hooks;
use crate::compression::ledger::now_ms;
use crate::compression::manage::{
    self, parse_mode, parse_provider, parse_telemetry, with_system, CmdError, EnableReq, Kind,
    SealReq,
};
use crate::compression::measure;
use crate::compression::model::ProviderName;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::Host;

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

fn required_error(key: &str) -> ToolError {
    ToolError::new("invalid_arguments", format!("{key} is required"))
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_trimmed(args, key).ok_or_else(|| required_error(key))
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

pub(super) fn status(host: &dyn Host, _args: &Value) -> Outcome {
    with_system(host, |cx| Ok(manage::status(cx)?.data))
}

pub(super) fn measure(host: &dyn Host, args: &Value) -> Outcome {
    let paths = crate::compression::store::Paths::from_host(host)
        .ok_or_else(|| ToolError::backend("data directory could not be resolved"))?;
    let min_turns = match args.get("min_turns").filter(|v| !v.is_null()) {
        None => measure::MIN_TURNS,
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| {
                ToolError::new("invalid_arguments", "min_turns must be a whole number")
            })?,
    };
    let since_ms = str_trimmed(args, "since")
        .map(|text| {
            measure::parse_since(text).ok_or_else(|| {
                ToolError::new(
                    "invalid_arguments",
                    "since needs a date (2026-11-01) or an RFC 3339 timestamp",
                )
            })
        })
        .transpose()?;
    let options = measure::Options {
        root: crate::compression::verify::transcript_root(),
        min_turns,
        since_ms,
    };
    measure::report(&paths, &options).map_err(ToolError::backend)
}

fn hook_paths(host: &dyn Host) -> Result<crate::compression::store::Paths, ToolError> {
    crate::compression::store::Paths::from_host(host)
        .ok_or_else(|| ToolError::backend("data directory could not be resolved"))
}

fn hook_id(args: &Value) -> Result<Option<String>, ToolError> {
    let Some(id) = str_trimmed(args, "hook") else {
        return Ok(None);
    };
    if hooks::HOOK_PROVIDERS.iter().any(|p| p.id == id) {
        return Ok(Some(id.to_string()));
    }
    Err(ToolError::new(
        "invalid_arguments",
        format!(
            "hook must be {}",
            hooks::HOOK_PROVIDERS.map(|p| p.id).join(" or ")
        ),
    ))
}

pub(super) fn hooks_pause(host: &dyn Host, args: &Value) -> Outcome {
    let now = now_ms();
    let end = hooks::pause_end(str_trimmed(args, "for"), str_trimmed(args, "until"), now)
        .map_err(|e| ToolError::new("invalid_arguments", e))?;
    hooks::set_paused(&hook_paths(host)?, true, end, now).map_err(ToolError::backend)
}

pub(super) fn hooks_resume(host: &dyn Host, _args: &Value) -> Outcome {
    hooks::set_paused(&hook_paths(host)?, false, None, now_ms()).map_err(ToolError::backend)
}

pub(super) fn hooks_adopt_edit(host: &dyn Host, args: &Value) -> Outcome {
    let id = hook_id(args)?.ok_or_else(|| required_error("hook"))?;
    hooks::adopt_edit(host, &hook_paths(host)?, &id, flag(args, "revert"))
        .map_err(ToolError::backend)
}

fn refuse_egress(provider: Option<ProviderName>) -> Result<(), ToolError> {
    match provider {
        Some(p) if crate::compression::hooks::trust_of(p).needs_egress_consent() => {
            Err(ToolError::new(
                "invalid_arguments",
                format!(
                    "{0} has a closed engine and sends code off this machine: accept that with toolportctl compression enable --provider {0} --accept-egress {0}",
                    p.as_str()
                ),
            ))
        }
        _ => Ok(()),
    }
}

pub(super) fn enable(host: &dyn Host, args: &Value) -> Outcome {
    let provider = str_trimmed(args, "provider")
        .map(parse_provider)
        .transpose()?;
    refuse_egress(provider)?;
    let req = EnableReq {
        provider,
        port: port(args)?,
        telemetry: str_trimmed(args, "telemetry")
            .map(parse_telemetry)
            .transpose()?,
        preset: str_trimmed(args, "preset").map(String::from),
        mode: str_trimmed(args, "mode").map(parse_mode).transpose()?,
        dry_run: dry_run(args),
    };
    with_system(host, |cx| Ok(manage::enable(cx, &req)?))
}

pub(super) fn disable(host: &dyn Host, args: &Value) -> Outcome {
    with_system(host, |cx| {
        Ok(manage::disable(cx, flag(args, "teardown"), dry_run(args))?)
    })
}

pub(super) fn set_provider(host: &dyn Host, args: &Value) -> Outcome {
    let provider = parse_provider(required(args, "provider")?)?;
    refuse_egress(Some(provider))?;
    with_system(host, |cx| {
        Ok(manage::set_provider(cx, provider, dry_run(args))?)
    })
}

pub(super) fn use_preset(host: &dyn Host, args: &Value) -> Outcome {
    let name = required(args, "preset")?;
    with_system(host, |cx| Ok(manage::use_preset(cx, name, dry_run(args))?))
}

fn mcpm_root(args: &Value) -> Result<Option<PathBuf>, ToolError> {
    let Some(value) = args.get("mcpm_root").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let refused = |why: &str| {
        ToolError::new(
            "invalid_arguments",
            format!("mcpm_root must be an absolute path of an existing directory: {why}"),
        )
    };
    let text = value
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| refused("it is empty"))?;
    if !Path::new(text).is_absolute() {
        return Err(refused("the path is relative"));
    }
    let root = std::fs::canonicalize(text).map_err(|_| refused("the path does not exist"))?;
    if !root.is_dir() {
        return Err(refused("the path is not a directory"));
    }
    Ok(Some(root))
}

pub(super) fn sync(host: &dyn Host, args: &Value) -> Outcome {
    let root = mcpm_root(args)?;
    let mut data: Value = with_system::<_, ToolError>(host, |cx| {
        Ok(manage::sync(cx, root.as_deref(), dry_run(args))?)
    })?;
    if let Some(root) = root {
        data["mcpmRoot"] = json!(root.to_string_lossy());
    }
    Ok(data)
}

pub(super) fn seal(host: &dyn Host, args: &Value) -> Outcome {
    let req = SealReq {
        preset: str_trimmed(args, "preset").map(String::from),
        apply: true,
        dry_run: dry_run(args),
    };
    with_system(host, |cx| Ok(manage::seal(cx, &req)?))
}
