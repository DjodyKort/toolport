use super::bundle::{Credential, PortableRoots};
use super::engine::{self, InitOptions, PullOptions, PushOptions, SyncContext};
use super::exec::SystemExec;
use super::gitsync::{git_sync, GitSyncOptions};
use crate::plus::args::{flag, list, str_nonempty as str_arg};
use crate::plus::skills::clock::SystemClock;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;

pub const MISSING_ARGUMENT: &str = "missing argument";

fn dirs(args: &Value) -> Result<(PathBuf, PathBuf), String> {
    let base =
        || crate::registry::conduit_dir().ok_or_else(|| "data directory unavailable".to_string());
    let config_dir = match str_arg(args, "configDir") {
        Some(p) => PathBuf::from(p),
        None => base()?,
    };
    let state_dir = match str_arg(args, "stateDir") {
        Some(p) => PathBuf::from(p),
        None => base()?.join("sync"),
    };
    Ok((config_dir, state_dir))
}

fn with_ctx<R: Serialize>(
    args: &Value,
    f: impl FnOnce(&SyncContext<'_>) -> Result<R, super::SyncError>,
) -> Result<Value, String> {
    let (config_dir, state_dir) = dirs(args)?;
    let home = match str_arg(args, "home") {
        Some(h) => h.to_string(),
        None => dirs::home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .ok_or_else(|| "home directory unavailable".to_string())?,
    };
    let ctx = SyncContext {
        roots: PortableRoots {
            home,
            mcpm_home: config_dir.to_string_lossy().into_owned(),
        },
        config_dir,
        state_dir,
        clock: &SystemClock,
        exec: &SystemExec,
    };
    let out = f(&ctx).map_err(|e| e.to_string())?;
    serde_json::to_value(out).map_err(|e| e.to_string())
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key).ok_or_else(|| format!("{MISSING_ARGUMENT}: {key}"))
}

pub fn init_handler(args: Value) -> Result<Value, String> {
    let repo = required(&args, "repo")?;
    let passphrase = required(&args, "passphrase")?;
    with_ctx(&args, |ctx| {
        engine::init(
            ctx,
            &InitOptions {
                repo,
                branch: str_arg(&args, "branch").unwrap_or("main"),
                machine_id: str_arg(&args, "machineId"),
                passphrase,
                reconfigure: flag(&args, "reconfigure"),
            },
        )
    })
}

pub fn push_handler(args: Value) -> Result<Value, String> {
    with_ctx(&args, |ctx| {
        engine::push(
            ctx,
            &PushOptions {
                include_projects: flag(&args, "includeProjects"),
                dry_run: flag(&args, "dryRun"),
            },
        )
    })
}

pub fn pull_handler(args: Value) -> Result<Value, String> {
    with_ctx(&args, |ctx| {
        engine::pull(
            ctx,
            &PullOptions {
                include_projects: flag(&args, "includeProjects"),
                force: flag(&args, "force"),
                dry_run: flag(&args, "dryRun"),
                resolve: !flag(&args, "noResolve"),
                run_setup: flag(&args, "runSetup"),
            },
        )
    })
}

pub fn diff_handler(args: Value) -> Result<Value, String> {
    with_ctx(&args, engine::diff)
}

pub fn status_handler(args: Value) -> Result<Value, String> {
    with_ctx(&args, |ctx| Ok(engine::status(ctx)))
}

pub fn reset_handler(args: Value) -> Result<Value, String> {
    with_ctx(&args, |ctx| {
        engine::reset(ctx).map(|removed| json!({ "removed": removed }))
    })
}

pub fn rotate_handler(args: Value) -> Result<Value, String> {
    let new_passphrase = required(&args, "newPassphrase")?;
    with_ctx(&args, |ctx| engine::rotate_passphrase(ctx, new_passphrase))
}

pub fn add_project_handler(args: Value) -> Result<Value, String> {
    let name = required(&args, "name")?;
    let path = required(&args, "path")?;
    let files: Vec<String> = list(&args, "files")
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    with_ctx(&args, |ctx| {
        engine::add_project(ctx, name, std::path::Path::new(path), &files)
            .map(|replaced| json!({ "name": name, "replaced": replaced }))
    })
}

pub fn remove_project_handler(args: Value) -> Result<Value, String> {
    let name = required(&args, "name")?;
    with_ctx(&args, |ctx| {
        engine::remove_project(ctx, name).map(|removed| json!({ "name": name, "removed": removed }))
    })
}

pub fn git_sync_handler(args: Value) -> Result<Value, String> {
    with_ctx(&args, |ctx| {
        git_sync(
            ctx,
            &GitSyncOptions {
                repo: str_arg(&args, "repo").map(str::to_string),
                branch: str_arg(&args, "branch").map(str::to_string),
                auto: flag(&args, "auto"),
                status: flag(&args, "status"),
                clear: flag(&args, "clear"),
            },
        )
    })
}

pub fn migrate_handler(args: Value) -> Result<Value, String> {
    let dir = PathBuf::from(required(&args, "bundleDir")?);
    let cred = match (str_arg(&args, "passphrase"), str_arg(&args, "key")) {
        (Some(p), _) => Credential::Passphrase(p),
        (None, Some(k)) => Credential::Key(k),
        _ => return Err(format!("{MISSING_ARGUMENT}: passphrase")),
    };
    with_ctx(&args, |ctx| {
        engine::migrate_bundle(ctx, &dir, cred, flag(&args, "includeProjects")).map(|r| {
            json!({
                "written": r.written,
                "skipped": r.skipped,
                "serverOrigins": r.server_origins,
            })
        })
    })
}
