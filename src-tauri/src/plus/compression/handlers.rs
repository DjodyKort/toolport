//! `plus.compression.{status,enable,disable,setProvider,use,sync,pin,seal,env,doctor}`: the JSON
//! entry points over [`manage`]. Arguments use camelCase keys; every result is the same JSON
//! `toolportctl --json` prints as `data`.

use super::launch::SystemOps;
use super::manage::*;
use serde_json::Value;
use std::path::PathBuf;

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn port(args: &Value) -> Result<Option<u16>, CmdError> {
    let bad = || CmdError::usage("port needs a whole number");
    match args.get("port") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .map(Some)
            .ok_or_else(bad),
        Some(Value::String(s)) => s.parse().map(Some).map_err(|_| bad()),
        Some(_) => Err(bad()),
    }
}

fn answer(result: CmdResult) -> Result<Value, String> {
    result.map_err(|e| e.message)
}

pub fn enable_handler(args: Value) -> Result<Value, String> {
    answer((|| {
        let req = EnableReq {
            provider: text(&args, "provider").map(parse_provider).transpose()?,
            port: port(&args)?,
            telemetry: text(&args, "telemetry").map(parse_telemetry).transpose()?,
            preset: text(&args, "preset").map(String::from),
            mode: text(&args, "mode").map(parse_mode).transpose()?,
            dry_run: flag(&args, "dryRun"),
        };
        with_system(|cx| enable(cx, &req))
    })())
}

pub fn disable_handler(args: Value) -> Result<Value, String> {
    answer(with_system(|cx| {
        disable(cx, flag(&args, "teardown"), flag(&args, "dryRun"))
    }))
}

pub fn set_provider_handler(args: Value) -> Result<Value, String> {
    answer((|| {
        let provider = parse_provider(
            text(&args, "provider").ok_or_else(|| CmdError::usage("provider is required"))?,
        )?;
        with_system(|cx| set_provider(cx, provider, flag(&args, "dryRun")))
    })())
}

pub fn use_handler(args: Value) -> Result<Value, String> {
    answer((|| {
        let name = text(&args, "preset").ok_or_else(|| CmdError::usage("preset is required"))?;
        with_system(|cx| use_preset(cx, name, flag(&args, "dryRun")))
    })())
}

pub fn sync_handler(args: Value) -> Result<Value, String> {
    let root = text(&args, "mcpmRoot").map(PathBuf::from);
    answer(with_system(|cx| {
        sync(cx, root.as_deref(), flag(&args, "dryRun"))
    }))
}

pub fn pin_handler(args: Value) -> Result<Value, String> {
    let req = PinReq {
        version: text(&args, "version").map(String::from),
        install: flag(&args, "install"),
        refresh: flag(&args, "refresh"),
        dry_run: flag(&args, "dryRun"),
    };
    answer(with_system(|cx| pin(cx, &req)))
}

pub fn seal_handler(args: Value) -> Result<Value, String> {
    let req = SealReq {
        preset: text(&args, "preset").map(String::from),
        apply: flag(&args, "apply"),
        dry_run: flag(&args, "dryRun"),
    };
    answer(with_system(|cx| seal(cx, &req)))
}

pub fn env_handler(args: Value) -> Result<Value, String> {
    answer((|| {
        let cwd = match text(&args, "cwd") {
            Some(cwd) => cwd.to_string(),
            None => std::env::current_dir()
                .map_err(|e| CmdError::failed("cwd", e.to_string()))?
                .to_string_lossy()
                .into_owned(),
        };
        with_system(|cx| env(cx, &cwd))
    })())
}

pub fn doctor_handler(_args: Value) -> Result<Value, String> {
    answer(with_system(|cx| doctor(cx, &SystemOps::new())))
}

pub fn status_handler(_args: Value) -> Result<Value, String> {
    answer(with_system(|cx| status(cx).map(|report| report.data)))
}
